//! Read-only, NUL-delimited worktree inventory and selected-checkout status.
use anyhow::{Context, Result, bail};
use serde::Serialize;
use std::{
    path::{Path, PathBuf},
    process::Stdio,
    time::Duration,
};
use tokio::io::AsyncReadExt;
const LIMIT: usize = 2 * 1024 * 1024;
pub const MAX_WORKTREES: usize = 256;
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Worktree {
    pub path: PathBuf,
    pub head: Option<String>,
    pub branch: Option<String>,
    pub detached: bool,
    pub bare: bool,
    pub locked: Option<String>,
    pub prunable: Option<String>,
}
#[derive(Clone, Debug, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    pub head: Option<String>,
    pub branch: Option<String>,
    pub upstream: Option<String>,
    pub ahead: Option<u64>,
    pub behind: Option<u64>,
    pub staged: usize,
    pub unstaged: usize,
    pub untracked: usize,
    pub conflicts: usize,
    pub shallow: bool,
}
async fn git(root: &Path, args: &[&str]) -> Result<Vec<u8>> {
    let job = async {
        let _permit = crate::scheduler::acquire().await;
        let mut child = super::exec::command(root, args)
            .env("GIT_TERMINAL_PROMPT", "0")
            .env("GIT_NO_LAZY_FETCH", "1")
            .env("GIT_NO_REPLACE_OBJECTS", "1")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;
        async fn collect(input: impl tokio::io::AsyncRead + Unpin, limit: usize) -> Result<Vec<u8>> {
            let mut bytes = vec![];
            input.take(limit as u64 + 1).read_to_end(&mut bytes).await?;
            if bytes.len() > limit {
                bail!("Worktree metadata exceeds its read limit");
            }
            Ok(bytes)
        }
        let stdout = child.stdout.take().unwrap();
        let stderr = child.stderr.take().unwrap();
        let (stdout, _, status) = tokio::try_join!(collect(stdout, LIMIT), collect(stderr, 64 * 1024), async {
            Ok::<_, anyhow::Error>(child.wait().await?)
        })?;
        if !status.success() {
            bail!("Git could not read this worktree. Refresh after fixing missing folders, permissions or local refs.");
        }
        Ok(stdout)
    };
    tokio::time::timeout(Duration::from_secs(10), job)
        .await
        .context("Worktree query timed out; refresh or use your Git client for a larger repository")?
}
pub async fn list(root: &Path) -> Result<Vec<Worktree>> {
    parse(&git(root, &["worktree", "list", "--porcelain", "-z"]).await?)
}
pub fn parse(bytes: &[u8]) -> Result<Vec<Worktree>> {
    anyhow::ensure!(
        bytes.len() <= LIMIT && (bytes.is_empty() || bytes.ends_with(b"\0\0")),
        "Truncated or oversized worktree inventory"
    );
    let mut result = vec![];
    let mut current: Option<Worktree> = None;
    for field in bytes.split(|b| *b == 0) {
        if field.is_empty() {
            if let Some(row) = current.take() {
                if result.len() >= MAX_WORKTREES {
                    bail!("Worktree inventory exceeds 256 entries; use your Git client to narrow it");
                }
                result.push(row);
            }
            continue;
        }
        if let Some(path) = field.strip_prefix(b"worktree ") {
            if current.is_some() {
                bail!("Malformed worktree inventory");
            }
            current = Some(Worktree {
                path: super::path_bytes(path)?,
                head: None,
                branch: None,
                detached: false,
                bare: false,
                locked: None,
                prunable: None,
            });
            continue;
        }
        let row = current.as_mut().context("Missing worktree path")?;
        let field = std::str::from_utf8(field).context("Worktree metadata is not UTF-8")?;
        if let Some(sha) = field.strip_prefix("HEAD ") {
            if sha.len() != 40 && sha.len() != 64 || !sha.bytes().all(|b| b.is_ascii_hexdigit()) {
                bail!("Invalid worktree head");
            }
            row.head = (!sha.bytes().all(|b| b == b'0')).then(|| sha.to_owned());
        } else if let Some(branch) = field.strip_prefix("branch ") {
            row.branch = Some(branch.strip_prefix("refs/heads/").unwrap_or(branch).into());
        } else if field == "bare" {
            row.bare = true;
        } else if field == "detached" {
            row.detached = true;
        } else if field == "locked" || field.starts_with("locked ") {
            row.locked = Some(field.strip_prefix("locked ").unwrap_or("").into());
        } else if field == "prunable" || field.starts_with("prunable ") {
            row.prunable = Some(field.strip_prefix("prunable ").unwrap_or("").into());
        }
        // Future porcelain fields do not change the native path/HEAD authority.
    }
    if current.is_some() {
        bail!("Unterminated worktree inventory");
    }
    Ok(result)
}
pub async fn status(root: &Path) -> Result<Status> {
    let bytes = super::operations::read(
        root,
        &[
            "-c",
            "core.fsmonitor=false",
            "-c",
            "core.hooksPath=/dev/null",
            "-c",
            "submodule.recurse=false",
            "status",
            "--porcelain=v2",
            "--branch",
            "--ahead-behind",
            "-z",
            "--untracked-files=normal",
            "--ignore-submodules=all",
            "--no-renames",
        ],
    )
    .await?;
    let mut status = parse_status(&bytes)?;
    let (_, git, common) = super::repository_paths(root).await?;
    status.shallow = git.join("shallow").exists() || common.join("shallow").exists();
    Ok(status)
}
pub fn parse_status(bytes: &[u8]) -> Result<Status> {
    let mut status = Status::default();
    let mut original = false;
    for record in bytes.split(|b| *b == 0).filter(|r| !r.is_empty()) {
        if original {
            original = false;
            continue;
        }
        if let Some(value) = record.strip_prefix(b"# ") {
            let field = std::str::from_utf8(value)?;
            if let Some(value) = field.strip_prefix("branch.oid ") {
                status.head = (value != "(initial)").then(|| value.into());
            }
            if let Some(value) = field.strip_prefix("branch.head ") {
                status.branch = (value != "(detached)").then(|| value.into());
            }
            if let Some(value) = field.strip_prefix("branch.upstream ") {
                status.upstream = Some(value.into());
            }
            if let Some(value) = field.strip_prefix("branch.ab ") {
                let (a, b) = value.split_once(' ').context("Invalid ahead/behind status")?;
                status.ahead = Some(a.trim_start_matches('+').parse()?);
                status.behind = Some(b.trim_start_matches('-').parse()?);
            }
        } else {
            match record[0] {
                b'1' | b'2' => {
                    if record.len() < 5 || record[1] != b' ' || record[4] != b' ' {
                        bail!("Malformed worktree status");
                    }
                    status.staged += usize::from(record[2] != b'.');
                    status.unstaged += usize::from(record[3] != b'.');
                    original = record[0] == b'2';
                }
                b'u' => status.conflicts += 1,
                b'?' => status.untracked += 1,
                b'!' => (),
                _ => bail!("Unsupported worktree status record"),
            }
        }
    }
    if original {
        bail!("Truncated rename status");
    }
    Ok(status)
}
