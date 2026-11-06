//! All Git execution. Calls run with cwd = the repository root, except reading a submodule's HEAD.
pub mod auth;
mod catfile;
mod changes;
mod commits;
pub mod exec;
mod files;
mod grep;
mod patch;
pub(crate) mod patch_cache;
mod refs;
#[cfg(unix)]
mod worktree;

use std::path::{Path, PathBuf};

use catfile::CatFileBatch;
pub use catfile::CatFileObject;
pub use changes::is_binary;
pub use grep::{GrepMatch, GrepOptions, GrepResult};
pub use refs::{Remote, Upstream};

use crate::error::{GitError, Result};
use exec::{ExecOptions, exec_git, text};

/// Git's `core.bigFileThreshold` default: a file this large is diffed as binary without reading
/// it, which also bounds what sizing an untracked file loads.
pub const BIG_FILE_THRESHOLD: u64 = 512 * 1024 * 1024;
/// Concurrent worktree reads while sizing or hashing files.
const READ_CONCURRENCY: usize = 8;

/// The uncommitted tree, accepted on either side of a comparison.
pub const WORKTREE: &str = "worktree";

pub struct GitRepo {
    pub root: PathBuf,
    pub git_dir: PathBuf,
    /// Shared dir for worktrees (refs, packed-refs). Equals `git_dir` for the main worktree.
    pub common_dir: PathBuf,
    /// Where this run fetches pull requests; unique, so two servers never share refs.
    pub review_refs: String,
    git_auth: Option<auth::GitAuth>,
    #[cfg(unix)]
    worktree: std::sync::Arc<std::fs::File>,
    cat_file: CatFileBatch,
    pub(crate) hashed_files: std::sync::atomic::AtomicUsize,
    pub(crate) patches: patch_cache::PatchCache,
}

impl GitRepo {
    pub async fn open(dir: &Path) -> Result<Self> {
        let out = exec_git(
            dir,
            &["rev-parse", "--show-toplevel", "--absolute-git-dir", "--git-common-dir"],
            ExecOptions::default(),
        )
        .await?;
        let out = text(out);
        let mut lines = out.trim().lines();
        let (Some(root), Some(git_dir)) = (lines.next(), lines.next()) else {
            return Err(GitError {
                code: Some(128),
                ..GitError::refused(format!("not a git repository: {}", dir.display()), &[])
            }
            .into());
        };
        // Git prints POSIX separators even on Windows; normalise them to native ones.
        let root = native(root);
        let git_dir = native(git_dir);
        let common_dir = lines.next().map_or_else(|| git_dir.clone(), |common| native_path(&root.join(native(common))));
        Ok(Self {
            #[cfg(unix)]
            worktree: std::sync::Arc::new(std::fs::File::open(&root)?),
            git_auth: None,
            cat_file: CatFileBatch::new(root.clone()),
            hashed_files: std::sync::atomic::AtomicUsize::new(0),
            patches: Default::default(),
            review_refs: format!("refs/diffle/{}", uuid::Uuid::new_v4()),
            root,
            git_dir,
            common_dir,
        })
    }

    /// Clone into an empty directory; full history preserves merge bases.
    pub async fn clone_into(url: &str, dir: &Path) -> Result<Self> {
        Self::clone_with_auth(url, dir, None).await
    }

    pub fn with_git_auth(mut self, auth: Option<auth::GitAuth>) -> Self {
        self.git_auth = auth;
        self
    }

    pub(crate) fn has_git_auth(&self) -> bool {
        self.git_auth.is_some()
    }

    pub async fn clone_with_auth(url: &str, dir: &Path, auth: Option<auth::GitAuth>) -> Result<Self> {
        let opts = ExecOptions { env: &[("GIT_TERMINAL_PROMPT", "0")], auth: auth.as_ref(), ..Default::default() };
        exec_git(dir, &["clone", "--quiet", "--no-tags", "--", url, "."], opts).await?;
        Ok(Self::open(dir).await?.with_git_auth(auth))
    }

    /// How many `cat-file --batch` processes this repository has started. Diagnostics and tests.
    pub fn cat_file_spawns(&self) -> usize {
        self.cat_file.spawns()
    }

    /// Worktree entries hashed, for comparing incremental and full refresh work.
    pub fn worktree_hashes(&self) -> usize {
        self.hashed_files.load(std::sync::atomic::Ordering::Relaxed)
    }

    /// Retained patch bytes and cache hits, for performance diagnostics.
    pub fn patch_cache_stats(&self) -> (usize, usize) {
        self.patches.stats()
    }

    async fn exec(&self, args: &[&str], opts: ExecOptions<'_>) -> Result<Vec<u8>, GitError> {
        exec_git(&self.root, args, opts).await
    }

    async fn text(&self, args: &[&str]) -> Result<String, GitError> {
        Ok(text(self.exec(args, ExecOptions::default()).await?))
    }

    async fn text_with(&self, args: &[&str], opts: ExecOptions<'_>) -> Result<String, GitError> {
        Ok(text(self.exec(args, opts).await?))
    }
}

fn native(path: &str) -> PathBuf {
    native_path(Path::new(path))
}

/// Separators as the platform writes them, and `.`/`..` folded, so paths compare equal to joined ones.
fn native_path(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for part in path.components() {
        match part {
            std::path::Component::ParentDir => {
                out.pop();
            }
            std::path::Component::CurDir => {}
            other => out.push(other),
        }
    }
    out
}

/// Git reverses the commit-to-worktree diff when the worktree is the old endpoint.
fn diff_range<'a>(old: &'a str, new: &'a str) -> Vec<&'a str> {
    if old == WORKTREE {
        vec!["-R", new]
    } else if new == WORKTREE {
        vec![old]
    } else {
        vec![old, new]
    }
}

/// `path` under `root`, or none when it would leave the repository.
fn inside(root: &Path, path: &str) -> Option<PathBuf> {
    let joined = native_path(&root.join(path));
    joined.starts_with(root).then_some(joined)
}
