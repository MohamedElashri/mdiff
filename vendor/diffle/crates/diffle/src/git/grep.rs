//! Searching the new side, bounded at the process boundary.
use std::collections::{HashMap, HashSet};
use std::time::Duration;

use super::exec::{ExecOptions, RecordOptions, exec_records, split_z};
use super::{GitRepo, WORKTREE};
use crate::concurrency::map_limit;
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

fn pathspec_batches(paths: &[String]) -> Vec<Vec<String>> {
    let mut paths: Vec<_> = paths.iter().collect();
    paths.sort_unstable();
    paths.dedup();
    let mut batches = Vec::new();
    let mut batch = Vec::new();
    let mut bytes = 0;
    for path in paths {
        let literal = format!(":(literal){path}");
        let size = literal.len() + 1;
        if !batch.is_empty() && bytes + size > MAX_PATHSPEC_BYTES {
            batches.push(std::mem::take(&mut batch));
            bytes = 0;
        }
        batch.push(literal);
        bytes += size;
    }
    batches.push(batch);
    batches
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
        let results = map_limit(pathspec_batches(paths), 2, |batch| async move {
            let mut args = search_args(vec!["grep", "--name-only", "--threads=2"], query, rev, opts);
            args.extend(batch.iter().map(String::as_str));
            let options = ExecOptions { ok_codes: &[1], ..Default::default() };
            let out = tokio::time::timeout(GREP_TIMEOUT, self.exec(&args, options))
                .await
                .map_err(|_| GitError::refused("Search timed out", &args))??;
            Ok::<_, GitError>(split_z(&out))
        })
        .await;
        let mut hits = HashSet::new();
        for result in results {
            hits.extend(result?.iter().filter_map(|path| strip_revision(path, rev)).map(str::to_owned));
        }
        Ok(hits)
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
        let args = search_args(command, query, rev, opts);
        let parse = |record: &str| parse_record(record, rev != WORKTREE);
        let accept = opts.ranges.map(|ranges| {
            move |record: &str| {
                let m = parse(record);
                ranges.get(&m.path).is_some_and(|r| r.iter().any(|&(start, end)| m.line >= start && m.line <= end))
            }
        });
        let filter = accept.as_ref().map(|f| f as &(dyn Fn(&str) -> bool + Sync));
        let mut matches = Vec::new();
        for batch in pathspec_batches(opts.paths.unwrap_or_default()) {
            let mut args = args.clone();
            args.extend(batch.iter().map(String::as_str));
            let remaining = limit - matches.len();
            let records = exec_records(
                &self.root,
                &args,
                remaining + 1,
                RecordOptions { ok_codes: &[0, 1], timeout: GREP_TIMEOUT, accept: filter },
            )
            .await?;
            matches.extend(records.iter().take(remaining).map(|r| parse(r)));
            if records.len() > remaining {
                return Ok(GrepResult { matches, truncated: true });
            }
        }
        Ok(GrepResult { matches, truncated: false })
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
