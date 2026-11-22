//! Reusable Git-write primitives; these are deliberately not standalone HTTP routes.
pub mod advanced;
mod execute;
mod prepare;
pub mod runner;
mod state;
mod types;
use anyhow::{Context, Result};
pub use execute::execute;
pub use prepare::{prepare, validate_url};
use sha2::{Digest, Sha256};
pub use state::{changes, identity, inspect, patch, path_fingerprint, remote_head, validate_path};
use std::{path::Path, process::Stdio};
use tokio::io::AsyncReadExt;
pub use types::*;
pub(crate) fn hash(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}
// Status/diff can invoke clean/process filters even with --no-textconv. Enumerate
// only config keys, then suppress every configured driver for read-only previews.
async fn raw_read(root: &Path, args: &[&str], allow_absent: bool) -> Result<Vec<u8>> {
    let _slot = crate::scheduler::acquire().await;
    let mut command = super::exec::command(root, args);
    for key in [
        "GIT_DIR",
        "GIT_WORK_TREE",
        "GIT_INDEX_FILE",
        "GIT_COMMON_DIR",
        "GIT_NAMESPACE",
        "GIT_PREFIX",
        "GIT_GLOB_PATHSPECS",
        "GIT_NOGLOB_PATHSPECS",
        "GIT_ICASE_PATHSPECS",
    ] {
        command.env_remove(key);
    }
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.as_std_mut().process_group(0);
    }
    let mut child = command
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_NO_LAZY_FETCH", "1")
        .env("GIT_LITERAL_PATHSPECS", "1")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()?;
    struct Group(Option<u32>);
    impl Drop for Group {
        fn drop(&mut self) {
            #[cfg(unix)]
            if let Some(pid) = self.0 {
                unsafe {
                    libc::kill(-(pid as i32), libc::SIGKILL);
                }
            }
        }
    }
    let mut group = Group(child.id());
    let stdout = child.stdout.take().unwrap();
    let mut bytes = Vec::new();
    stdout.take(2 * 1024 * 1024 + 1).read_to_end(&mut bytes).await?;
    anyhow::ensure!(bytes.len() <= 2 * 1024 * 1024, "Git operation preview exceeds 2 MiB; use an external Git client");
    let result = child.wait().await;
    group.0 = None;
    let status = result?;
    anyhow::ensure!(
        status.success() || (allow_absent && status.code() == Some(1)),
        "Git could not prepare this operation. Check its target, local refs and configuration."
    );
    Ok(bytes)
}
pub(crate) async fn read(root: &Path, args: &[&str]) -> Result<Vec<u8>> {
    let job = async {
        if !args.iter().any(|arg| matches!(*arg, "status" | "diff")) {
            return raw_read(root, args, false).await;
        }
        let keys = raw_read(
            root,
            &["config", "--null", "--name-only", "--get-regexp", r"^filter\..*\.(clean|smudge|process|required)$"],
            true,
        )
        .await?;
        let mut names = std::collections::BTreeSet::new();
        for key in keys.split(|b| *b == 0).filter(|key| !key.is_empty()) {
            let key = std::str::from_utf8(key)?;
            if let Some(name) = key.strip_prefix("filter.").and_then(|key| key.rsplit_once('.')).map(|(name, _)| name) {
                anyhow::ensure!(
                    name.len() <= 256 && names.len() < 64,
                    "Too many filter drivers for a safe preview; use an external Git client"
                );
                names.insert(name);
            }
        }
        let mut safe =
            vec!["-c".to_owned(), "core.fsmonitor=false".into(), "-c".into(), "core.hooksPath=/dev/null".into()];
        for name in names {
            for (field, value) in [("clean", ""), ("smudge", ""), ("process", ""), ("required", "false")] {
                safe.push("-c".into());
                safe.push(format!("filter.{name}.{field}={value}"));
            }
        }
        safe.extend(args.iter().map(|s| s.to_string()));
        let refs = safe.iter().map(String::as_str).collect::<Vec<_>>();
        raw_read(root, &refs, false).await
    };
    tokio::time::timeout(std::time::Duration::from_secs(10), job).await.context("Git operation preview timed out")?
}
pub(crate) fn text(bytes: Vec<u8>) -> Result<String> {
    Ok(String::from_utf8(bytes)?.trim_end_matches('\n').into())
}
pub async fn revalidate(plan: &Plan) -> Result<()> {
    anyhow::ensure!(
        inspect(&plan.before.identity.root, &plan.action).await? == plan.before,
        "Repository, refs, index or selected files changed. Refresh and preview again."
    );
    for (path, expected) in &plan.extra {
        anyhow::ensure!(
            &state::path_fingerprint(path).await? == expected,
            "Destination or selected worktree changed. Preview again."
        );
    }
    if let Some(remote) = &plan.remote {
        anyhow::ensure!(
            state::remote_url(&plan.before.identity.root, &remote.remote).await?.1 == remote.url_hash,
            "Remote destination changed. Preview again."
        );
        if !matches!(plan.action, Action::Fetch { .. }) {
            anyhow::ensure!(
                remote_head(&plan.before.identity.root, &remote.remote, &remote.branch).await? == remote.expected,
                "Remote branch changed. Fetch and preview again."
            );
        }
    }
    Ok(())
}

pub async fn catalog(root: &Path) -> Result<serde_json::Value> {
    let (state, files) = state::inspect_parts(root, &Action::Commit { message: String::new(), amend: false }).await?;
    let branches = text(read(root, &["for-each-ref", "--format=%(refname:short)", "refs/heads/"]).await?)?;
    let tags = text(read(root, &["for-each-ref", "--format=%(refname:strip=2)", "refs/tags/"]).await?)?;
    let remotes = text(read(root, &["remote"]).await?)?;
    let stash = text(read(root, &["stash", "list", "--max-count=100", "--format=%H%x09%gs"]).await?)?;
    let stashes = stash
        .lines()
        .filter_map(|line| line.split_once('\t'))
        .map(|(oid, label)| serde_json::json!({"oid":oid,"label":label}))
        .collect::<Vec<_>>();
    let worktrees=super::worktrees::list(root).await?.into_iter().map(|row|serde_json::json!({"path":row.path.to_string_lossy(),"branch":row.branch,"locked":row.locked,"prunable":row.prunable,"bare":row.bare,"canModify":row.path.to_str().is_some()})).collect::<Vec<_>>();
    let locks = ["index.lock", "HEAD.lock"]
        .into_iter()
        .filter(|name| state.identity.git.join(name).exists())
        .collect::<Vec<_>>();
    Ok(
        serde_json::json!({"state":state,"files":files,"branches":branches.lines().collect::<Vec<_>>(),"tags":tags.lines().collect::<Vec<_>>(),"remotes":remotes.lines().collect::<Vec<_>>(),"stashes":stashes,"worktrees":worktrees,"locks":locks,"recovery":advanced::recovery(root).await?}),
    )
}

pub async fn validate_branch(root: &Path, name: &str) -> Result<()> {
    anyhow::ensure!(
        !name.is_empty() && name.len() <= 255 && !name.starts_with('-') && !name.contains(['\0', '\r', '\n']),
        "Invalid branch name"
    );
    read(root, &["check-ref-format", &format!("refs/heads/{name}")]).await?;
    Ok(())
}

/// Local upstream counts only; does not scan files, fetch, or start helpers.
pub async fn upstream(root: &Path) -> Result<serde_json::Value> {
    let upstream = read(root, &["rev-parse", "--abbrev-ref", "--symbolic-full-name", "@{upstream}"]).await;
    let Ok(upstream) = upstream else {
        return Ok(serde_json::json!({"upstream":null,"ahead":null,"behind":null}));
    };
    let upstream = text(upstream)?;
    let counts = text(read(root, &["rev-list", "--left-right", "--count", &format!("HEAD...{upstream}")]).await?)?;
    let mut fields = counts.split_whitespace();
    let ahead = fields.next().context("Missing ahead count")?.parse::<u64>()?;
    let behind = fields.next().context("Missing behind count")?.parse::<u64>()?;
    Ok(serde_json::json!({"upstream":upstream,"ahead":ahead,"behind":behind}))
}

/// Inspect the recorded network destination without requiring a clean local checkout.
pub async fn reconcile_remote(root: &Path, proof: &RemoteProof) -> Result<Option<String>> {
    anyhow::ensure!(
        !proof.remote.is_empty()
            && proof.remote.len() <= 255
            && !proof.remote.starts_with('-')
            && proof.remote.bytes().all(|c| c.is_ascii_alphanumeric() || b"-_.".contains(&c)),
        "Invalid recorded remote"
    );
    validate_branch(root, &proof.branch).await?;
    anyhow::ensure!(
        state::remote_url(root, &proof.remote).await?.1 == proof.url_hash,
        "Remote destination changed; inspect the original remote externally"
    );
    remote_head(root, &proof.remote, &proof.branch).await
}
