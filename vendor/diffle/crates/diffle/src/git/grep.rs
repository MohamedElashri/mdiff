//! Searching the new side, bounded at the process boundary.
use std::collections::{HashMap, HashSet};
use std::time::Duration;

use super::exec::{ExecOptions, RecordOptions, exec_records, split_z};
use super::{GitRepo, WORKTREE};
use crate::error::GitError;

/// A search that runs this long is a runaway regex, not a slow repository.
const GREP_TIMEOUT: Duration = Duration::from_secs(10);
const MAX_PATHSPEC_BYTES: usize = 24 * 1024;

#[derive(Clone, Copy, Default)]
pub struct GrepOptions<'a> {
    /// Whole words only (vim's `*`).
    pub word: bool,
    pub ignore_case: bool,
    /// An extended regex instead of a fixed string.
    pub regex: bool,
    /// Restricts files; an empty list matches nothing.
    pub paths: Option<&'a [String]>,
    /// Restricts new-side lines, inclusive, before the limit applies; an empty map matches nothing.
    pub ranges: Option<&'a HashMap<String, Vec<(i64, i64)>>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GrepMatch {
    pub path: String,
    pub line: i64,
    pub text: String,
}

pub struct GrepResult {
    pub matches: Vec<GrepMatch>,
    pub truncated: bool,
}

fn pathspecs(paths: &[String]) -> Option<Vec<String>> {
    let bytes = paths.iter().map(|path| path.len() + 11).sum::<usize>();
    (bytes <= MAX_PATHSPEC_BYTES).then(|| paths.iter().map(|path| format!(":(literal){path}")).collect())
}

fn search_args<'a>(mut args: Vec<&'a str>, query: &'a str, rev: &'a str, opts: GrepOptions<'_>) -> Vec<&'a str> {
    args.extend(["-I", if opts.regex { "-E" } else { "-F" }, "-z"]);
    if opts.word {
        args.push("-w");
    }
    if opts.ignore_case {
        args.push("-i");
    }
    args.extend(["-e", query, if rev == WORKTREE { "--untracked" } else { rev }, "--"]);
    args
}

fn strip_revision<'a>(path: &'a str, rev: &str) -> Option<&'a str> {
    if rev == WORKTREE { Some(path) } else { path.strip_prefix(rev)?.strip_prefix(':') }
}

impl GitRepo {
    /// Find files with a possible hit without loading or parsing their diffs.
    pub async fn grep_paths(
        &self,
        query: &str,
        rev: &str,
        paths: &[String],
        opts: GrepOptions<'_>,
    ) -> Result<HashSet<String>, GitError> {
        if query.is_empty() || paths.is_empty() {
            return Ok(HashSet::new());
        }
        let literals = pathspecs(paths);
        let allowed = literals.is_none().then(|| paths.iter().map(String::as_str).collect::<HashSet<_>>());
        let mut args = search_args(vec!["grep", "--name-only"], query, rev, opts);
        args.extend(literals.iter().flatten().map(String::as_str));
        let options = ExecOptions { ok_codes: &[1], ..Default::default() };
        let out = tokio::time::timeout(GREP_TIMEOUT, self.exec(&args, options))
            .await
            .map_err(|_| GitError::refused("Search timed out", &args))??;
        Ok(split_z(&out)
            .iter()
            .filter_map(|path| strip_revision(path, rev))
            .filter(|path| allowed.as_ref().is_none_or(|allowed| allowed.contains(path)))
            .map(str::to_owned)
            .collect())
    }

    /// Search the new side, case-sensitive unless asked otherwise. Worktree searches include
    /// untracked files. Git is stopped once `limit + 1` records have arrived, so a common query in
    /// a large repository cannot flood the process.
    pub async fn grep(
        &self,
        query: &str,
        rev: &str,
        limit: usize,
        opts: GrepOptions<'_>,
    ) -> Result<GrepResult, GitError> {
        // An explicit empty path list means "search nothing": with no pathspec git would search everything.
        if query.is_empty()
            || opts.paths.is_some_and(<[String]>::is_empty)
            || opts.ranges.is_some_and(HashMap::is_empty)
        {
            return Ok(GrepResult { matches: Vec::new(), truncated: false });
        }
        // --no-column: `grep.column=true` would splice a column field into the -z record.
        let mut command = vec!["grep", "-n", "--no-column"];
        // Filtering must precede the limit: early matches outside hunks must not hide later visible hits.
        let max_count = format!("--max-count={}", limit + 1);
        if opts.ranges.is_none() {
            command.push(&max_count);
        }
        let mut args = search_args(command, query, rev, opts);
        let paths = opts.paths.unwrap_or_default();
        let literals = pathspecs(paths);
        // Large allowlists are filtered during the scan so they do not exceed Git's argv limit.
        let allowed = literals.is_none().then(|| paths.iter().map(String::as_str).collect::<HashSet<_>>());
        args.extend(literals.iter().flatten().map(String::as_str));
        let parse = |record: &str| parse_record(record, rev != WORKTREE);
        let accept = |record: &str| {
            let m = parse(record);
            allowed.as_ref().is_none_or(|allowed| allowed.contains(m.path.as_str()))
                && opts.ranges.is_none_or(|ranges| {
                    ranges.get(&m.path).is_some_and(|r| r.iter().any(|&(start, end)| m.line >= start && m.line <= end))
                })
        };
        let filter = (allowed.is_some() || opts.ranges.is_some()).then_some(&accept as &(dyn Fn(&str) -> bool + Sync));
        let records = exec_records(
            &self.root,
            &args,
            limit + 1,
            RecordOptions { ok_codes: &[0, 1], timeout: GREP_TIMEOUT, accept: filter },
        )
        .await?;
        let truncated = records.len() > limit;
        Ok(GrepResult { matches: records.iter().take(limit).map(|r| parse(r)).collect(), truncated })
    }
}

/// `-z`: "path\0line\0text" per match; with a rev the path is "rev:path".
fn parse_record(record: &str, with_rev: bool) -> GrepMatch {
    let mut fields = record.splitn(3, '\0');
    let raw = fields.next().unwrap_or_default();
    let path = if with_rev { raw.split_once(':').map_or(raw, |(_, p)| p) } else { raw };
    let line = fields.next().and_then(|l| l.parse().ok()).unwrap_or(0);
    let text = fields.next().unwrap_or_default().chars().take(300).collect();
    GrepMatch { path: path.to_owned(), line, text }
}
