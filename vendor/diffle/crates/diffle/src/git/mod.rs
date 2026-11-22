//! All Git execution. Calls run with cwd = the repository root, except reading a submodule's HEAD.
pub mod auth;
mod catfile;
mod changes;
mod commits;
pub mod exec;
mod files;
mod grep;
pub mod operations;
mod patch;
pub(crate) mod patch_cache;
mod refs;
#[cfg(unix)]
mod worktree;
pub mod worktrees;

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
    /// Persistence parent (containing `diffle/`). Defaults to this worktree's Git dir;
    /// managed remote reviews inject a durable root independent of checkout lifetime.
    pub data_git_dir: PathBuf,
    /// Optional stable range identity for managed remote reviews / pinned reconstruction.
    pub range_identity: Option<String>,
    pub resume_iterations: bool,
    /// Where this run fetches pull requests; unique, so two servers never share refs.
    pub review_refs: String,
    pub(crate) retained_refs: tokio::sync::Mutex<Vec<Option<String>>>,
    git_auth: Option<auth::GitAuth>,
    #[cfg(unix)]
    worktree: std::sync::Arc<std::fs::File>,
    cat_file: CatFileBatch,
    pub(crate) hashed_files: std::sync::atomic::AtomicUsize,
    pub(crate) patches: patch_cache::PatchCache,
}

impl GitRepo {
    pub async fn open(dir: &Path) -> Result<Self> {
        let (root, git_dir, common_dir) = repository_paths(dir).await?;
        Ok(Self {
            #[cfg(unix)]
            worktree: std::sync::Arc::new(std::fs::File::open(&root)?),
            git_auth: None,
            cat_file: CatFileBatch::new(root.clone()),
            hashed_files: std::sync::atomic::AtomicUsize::new(0),
            patches: Default::default(),
            retained_refs: tokio::sync::Mutex::new(Vec::new()),
            review_refs: format!("refs/diffle/{}", uuid::Uuid::new_v4()),
            root,
            data_git_dir: git_dir.clone(),
            range_identity: None,
            resume_iterations: false,
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

    pub fn with_data_root(mut self, root: PathBuf) -> Self {
        self.data_git_dir = root;
        self
    }

    /// Another owned checkout of these objects; credentials remain in memory.
    pub async fn reopen_at(&self, root: &Path) -> Result<Self> {
        Ok(Self::open(root).await?.with_git_auth(self.git_auth.clone()))
    }

    pub(crate) fn has_git_auth(&self) -> bool {
        self.git_auth.is_some()
    }

    pub async fn clone_with_auth(url: &str, dir: &Path, auth: Option<auth::GitAuth>) -> Result<Self> {
        Self::clone_repository(url, dir, auth, &[]).await
    }

    /// Keep the default checkout and its history. PR resolution fetches the base and head explicitly.
    pub(crate) async fn clone_for_pr(url: &str, dir: &Path, auth: Option<auth::GitAuth>) -> Result<Self> {
        Self::clone_repository(url, dir, auth, &["--single-branch"]).await
    }

    async fn clone_repository(url: &str, dir: &Path, auth: Option<auth::GitAuth>, options: &[&str]) -> Result<Self> {
        let opts = ExecOptions { env: &[("GIT_TERMINAL_PROMPT", "0")], auth: auth.as_ref(), ..Default::default() };
        let mut args = vec!["clone", "--quiet", "--no-tags"];
        args.extend_from_slice(options);
        args.extend(["--", url, "."]);
        exec_git(dir, &args, opts).await?;
        Ok(Self::open(dir).await?.with_git_auth(auth))
    }

    pub async fn clear_payloads(&self) {
        self.patches.clear();
        self.cat_file.close().await;
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

#[cfg(not(unix))]
fn native(path: &str) -> PathBuf {
    native_path(Path::new(path))
}

/// Preserve raw repository paths. The common fast path remains one process; unusual
/// line breaks use separate fields because rev-parse has no NUL path-output mode.
pub async fn repository_paths(dir: &Path) -> Result<(PathBuf, PathBuf, PathBuf)> {
    async fn discover(dir: &Path) -> Result<(PathBuf, PathBuf, PathBuf)> {
        let bytes = exec_git(
            dir,
            &["rev-parse", "--show-toplevel", "--absolute-git-dir", "--git-common-dir"],
            ExecOptions::default(),
        )
        .await?;
        let fields = bytes.split(|b| *b == b'\n').collect::<Vec<_>>();
        let (root, git, common) = if fields.len() == 4 && fields[3].is_empty() && !bytes.contains(&b'\r') {
            (path_bytes(fields[0])?, path_bytes(fields[1])?, path_bytes(fields[2])?)
        } else {
            let mut paths = vec![];
            for flag in ["--show-toplevel", "--absolute-git-dir", "--git-common-dir"] {
                let bytes = exec_git(dir, &["rev-parse", flag], ExecOptions::default()).await?;
                paths.push(path_bytes(bytes.strip_suffix(b"\n").unwrap_or(&bytes))?);
            }
            (paths.remove(0), paths.remove(0), paths.remove(0))
        };
        Ok((native_path(&root), native_path(&git), native_path(&dir.canonicalize()?.join(common))))
    }
    tokio::time::timeout(std::time::Duration::from_secs(10), discover(dir))
        .await
        .map_err(|_| GitError::refused("Repository discovery timed out", &["rev-parse"]))?
}
fn path_bytes(bytes: &[u8]) -> Result<PathBuf> {
    if bytes.is_empty() || bytes.contains(&0) {
        return Err(GitError::refused("Invalid repository path", &["rev-parse"]).into());
    }
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStringExt;
        Ok(PathBuf::from(std::ffi::OsString::from_vec(bytes.to_vec())))
    }
    #[cfg(not(unix))]
    {
        Ok(native(
            std::str::from_utf8(bytes)
                .map_err(|_| GitError::refused("Repository path is not UTF-8", &["rev-parse"]))?,
        ))
    }
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
