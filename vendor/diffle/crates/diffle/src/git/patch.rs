//! Unified diffs for the viewer: one file, a batch, or every changed file.
use std::collections::HashSet;
use std::sync::LazyLock;

use futures::{StreamExt, TryStreamExt, stream};
use regex::{Captures, Regex};

use super::exec::ExecOptions;
use super::{GitRepo, READ_CONCURRENCY, WORKTREE, diff_range};
use crate::error::GitError;
use crate::protocol::{ChangeStatus, ChangedFile};

/// Git quotes each side of a header on its own, so the added side's quoting is mirrored rather than assumed absent.
static ADDED_HEADER: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"(?m)^diff --git "?a/dev/null"? ("?)b/(.*)$"#).unwrap());
static DELETED_HEADER: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"(?m)^diff --git ("?)a/(.*) "?b/dev/null"?$"#).unwrap());

/// Fixed header prefixes and no `diff.external`, so a patch is always a unified diff.
fn patch_args(old: &str) -> [&'static str; 3] {
    // -R swaps Git's prefixes too; keep the rendered old/new sides named a/b.
    if old == WORKTREE {
        ["--no-ext-diff", "--src-prefix=b/", "--dst-prefix=a/"]
    } else {
        ["--no-ext-diff", "--src-prefix=a/", "--dst-prefix=b/"]
    }
}

/// A file only one side has, as the worktree side would lack it from the index: added or removed.
fn worktree_only(old: &str, new: &str, file: &ChangedFile) -> bool {
    if old == WORKTREE { file.status == ChangeStatus::D } else { new == WORKTREE && file.status == ChangeStatus::A }
}

impl GitRepo {
    /// Raw `git diff` for one file. Untracked files diff against /dev/null; pass `untracked` when
    /// the caller already knows, else an added file is probed.
    pub async fn patch(
        &self,
        old: &str,
        new: &str,
        file: &ChangedFile,
        context: u32,
        untracked: Option<bool>,
    ) -> Result<String, GitError> {
        if old == WORKTREE && new == WORKTREE {
            return Ok(String::new());
        }
        let unified = format!("-U{context}");
        let reverse = old == WORKTREE;
        let untracked = match untracked {
            _ if !worktree_only(old, new, file) => false,
            Some(known) => known,
            None => !self.is_tracked(&file.path).await?,
        };
        if untracked {
            let mut args = vec!["diff", "--no-index"];
            if reverse {
                args.push("-R");
            }
            args.extend(patch_args(old));
            args.extend([unified.as_str(), "--", "/dev/null", file.path.as_str()]);
            let out =
                self.text_with(&args, ExecOptions { ok_codes: &[0, 1], preview: true, ..Default::default() }).await?;
            // Normalize the a/ side so parsers see a conventional added-file header.
            let header = if reverse { &DELETED_HEADER } else { &ADDED_HEADER };
            return Ok(header
                .replace(&out, |m: &Captures| format!("diff --git {q}a/{rest} {q}b/{rest}", q = &m[1], rest = &m[2]))
                .into_owned());
        }
        let mut args = vec!["diff", "-M"];
        args.extend(patch_args(old));
        args.push(&unified);
        args.extend(diff_range(old, new));
        args.push("--");
        args.extend(file.old_path.as_deref());
        args.push(&file.path);
        self.text_with(&args, ExecOptions { preview: true, ..Default::default() }).await
    }

    /// Patches for all changed files. Tracked files in one call; untracked appended.
    pub async fn patch_all(
        &self,
        old: &str,
        new: &str,
        files: &[ChangedFile],
        context: u32,
    ) -> Result<String, GitError> {
        if old == WORKTREE && new == WORKTREE {
            return Ok(String::new());
        }
        let unified = format!("-U{context}");
        let mut args = vec!["diff", "-M"];
        args.extend(patch_args(old));
        args.push(&unified);
        args.extend(diff_range(old, new));
        let extras = async {
            let added: Vec<&ChangedFile> = files.iter().filter(|f| worktree_only(old, new, f) && !f.binary).collect();
            if added.is_empty() {
                return Ok(String::new());
            }
            // The whole index rather than the added paths as arguments: a large diff would exceed argv.
            let known: HashSet<String> = self.ls_files().await?.into_iter().collect();
            let untracked = added.into_iter().filter(|f| !known.contains(&f.path)).cloned().collect();
            self.untracked_patches(old, new, untracked, context).await
        };
        let (mut out, extras) = tokio::try_join!(self.text(&args), extras)?;
        out.push_str(&extras);
        Ok(out)
    }

    /// Patches for `files` only: one index lookup for worktree additions, one tracked diff,
    /// and separate diffs for untracked paths. The caller bounds the batch so paths fit argv.
    pub async fn patch_many(
        &self,
        old: &str,
        new: &str,
        files: &[ChangedFile],
        context: u32,
    ) -> Result<String, GitError> {
        if old == WORKTREE && new == WORKTREE {
            return Ok(String::new());
        }
        let candidates: Vec<String> =
            files.iter().filter(|f| worktree_only(old, new, f)).map(|f| f.path.clone()).collect();
        let known = self.tracked_paths(&candidates).await?;
        let (untracked, batched): (Vec<_>, Vec<_>) =
            files.iter().partition(|f| worktree_only(old, new, f) && !known.contains(&f.path));
        let unified = format!("-U{context}");
        let tracked = async {
            if batched.is_empty() {
                return Ok(String::new());
            }
            let mut args = vec!["diff", "-M"];
            args.extend(patch_args(old));
            args.push(&unified);
            args.extend(diff_range(old, new));
            args.push("--");
            for f in &batched {
                args.extend(f.old_path.as_deref());
                args.push(&f.path);
            }
            self.text_with(&args, ExecOptions { preview: true, ..Default::default() }).await
        };
        let untracked = untracked.into_iter().cloned().collect();
        let (mut out, extras) = tokio::try_join!(tracked, self.untracked_patches(old, new, untracked, context))?;
        out.push_str(&extras);
        Ok(out)
    }

    async fn untracked_patches(
        &self,
        old: &str,
        new: &str,
        files: Vec<ChangedFile>,
        context: u32,
    ) -> Result<String, GitError> {
        stream::iter(files)
            .map(|file| async move { self.patch(old, new, &file, context, Some(true)).await })
            .buffered(READ_CONCURRENCY)
            .try_fold(String::new(), |mut out, patch| async move {
                out.push_str(&patch);
                Ok(out)
            })
            .await
    }

    async fn is_tracked(&self, path: &str) -> Result<bool, GitError> {
        Ok(self.tracked_paths(&[path.to_owned()]).await?.contains(path))
    }
}
