//! Durable remote review objects/data, with disposable, isolated worktrees per live handle.
use anyhow::{Context, Result};
use diffle::{
    comments::CommentStore,
    git::{
        exec::{exec_git, ExecOptions},
        GitRepo,
    },
    persist::{with_file_lock, write_file_atomic},
};
use sha2::{Digest, Sha256};
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, LazyLock, Mutex, Weak,
    },
};

pub fn digest(key: &str) -> String {
    format!("{:x}", Sha256::digest(key.as_bytes()))
}

/// The last reader (including History) owns worktree removal. Objects and saved data are
/// outside this lease and never deleted when a view closes or its payload is evicted.
pub struct Checkout {
    directory: Option<tempfile::TempDir>,
    archive: PathBuf,
    path: PathBuf,
    fresh: AtomicBool,
    editable: AtomicBool,
    head: Mutex<String>,
    process_lease: Option<std::fs::File>,
}
// Reuse in-process readers; the file lease also excludes other app processes and queued cleanup.
static LIVE: LazyLock<Mutex<HashMap<PathBuf, Weak<Checkout>>>> = LazyLock::new(Mutex::default);
fn live_checkout(path: &Path) -> Option<Arc<Checkout>> {
    LIVE.lock().unwrap().get(path).and_then(Weak::upgrade)
}
impl Checkout {
    fn lease(mut value: Self) -> Result<Arc<Self>> {
        value.path = std::fs::canonicalize(&value.path)?;
        let directory = value
            .archive
            .parent()
            .context("Missing archive parent")?
            .join("leases");
        std::fs::create_dir_all(&directory)?;
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create(true).truncate(false);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let lease = options.open(directory.join(digest(&value.path.to_string_lossy())))?;
        lease.try_lock().map_err(|_| {
            anyhow::anyhow!("Retained checkout is busy; close its other review and retry")
        })?;
        value.process_lease = Some(lease);
        let value = Arc::new(value);
        let mut live = LIVE.lock().unwrap();
        live.retain(|_, lease| lease.strong_count() > 0);
        live.insert(value.path.clone(), Arc::downgrade(&value));
        Ok(value)
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
    pub fn take_fresh(&self) -> bool {
        self.fresh.swap(false, Ordering::SeqCst)
    }
    pub fn set_head(&self, head: String) {
        *self.head.lock().unwrap() = head;
    }
    pub fn mark_editable(&self) {
        self.editable.store(true, Ordering::SeqCst);
    }
    pub fn archive(&self) -> &Path {
        &self.archive
    }
    pub fn data_root(&self) -> PathBuf {
        self.archive.parent().unwrap().join("data")
    }
}
impl Drop for Checkout {
    fn drop(&mut self) {
        // Git, not TempDir, decides whether a checkout is safe to remove. In particular,
        // dirty/untracked files, external editor sessions and moved detached HEADs survive.
        if let Some(directory) = self.directory.take() {
            let _ = directory.keep();
        }
        if self.editable.load(Ordering::SeqCst) {
            return;
        }
        let Some(process_lease) = self.process_lease.take() else {
            return;
        };
        let path = self.path.clone();
        let archive = self.archive.clone();
        let expected = self.head.lock().unwrap().clone();
        let editable = self.editable.load(Ordering::SeqCst);
        if let Ok(runtime) = tokio::runtime::Handle::try_current() {
            runtime.spawn(async move {
                let _process_lease = process_lease;
                let prepare = archive.parent().unwrap().join("prepare");
                // Restore and cleanup use the same admission lock. If restore won,
                // its new lease supersedes this dead lease before any Git removal.
                let cleanup = with_file_lock(&prepare, || async {
                    if live_checkout(&path).is_some() { return Ok(()); }
                    let head = git(&path, &["rev-parse", "HEAD"]).await?;
                    let head = std::str::from_utf8(&head)?.trim();
                    anyhow::ensure!(!editable && head == expected,
                        "Checkout retained for external edits or a moved HEAD");
                    git(&archive, &["worktree", "remove", "--",
                        path.to_str().context("Non-UTF8 checkout path")?]).await?;
                    Ok(())
                });
                if !matches!(tokio::time::timeout(std::time::Duration::from_secs(5), cleanup).await, Ok(Ok(()))) {
                    let recovery = archive.parent().unwrap().join(format!("recovery-{}.json", digest(&path.to_string_lossy())));
                    let _ = write_file_atomic(&recovery, &serde_json::json!({
                        "version":1,"path":path,"reason":"Checkout retained; inspect before removing"
                    }).to_string()).await;
                }
            });
        }
        // At runtime shutdown retain rather than risk deleting an uninspected checkout.
    }
}

async fn git(path: &Path, args: &[&str]) -> Result<Vec<u8>> {
    Ok(tokio::time::timeout(
        std::time::Duration::from_secs(180),
        exec_git(
            path,
            args,
            ExecOptions {
                env: &[
                    ("GIT_TERMINAL_PROMPT", "0"),
                    ("GIT_CONFIG_COUNT", "1"),
                    ("GIT_CONFIG_KEY_0", "core.hooksPath"),
                    ("GIT_CONFIG_VALUE_0", "/dev/null"),
                ],
                ..Default::default()
            },
        ),
    )
    .await
    .context("Managed repository preparation timed out")??)
}

/// Import a complete object database and its refs, never refs alone. Preparation is serialized
/// by logical checkout identity across processes. Source data is retained for recovery.
pub async fn adopt(root: &Path, key: &str, source: &GitRepo) -> Result<(GitRepo, Arc<Checkout>)> {
    let entry = root.join(digest(key));
    with_file_lock(&entry.join("prepare"), || async {
        let archive = entry.join("objects.git");
        let data = entry.join("data");
        if !archive.exists() {
            let stage = tempfile::Builder::new()
                .prefix("import-")
                .tempdir_in(&entry)?;
            git(
                stage.path(),
                &[
                    "clone",
                    "--mirror",
                    "--no-hardlinks",
                    "--dissociate",
                    "--",
                    source.root.to_str().context("Non-UTF8 checkout path")?,
                    ".",
                ],
            )
            .await?;
            // The disposable source must not become the archive's future network remote.
            git(
                stage.path(),
                &["config", "--remove-section", "remote.origin"],
            )
            .await?;
            for remote in source.remotes().await? {
                let url = if let Ok(mut url) = url::Url::parse(&remote.url) {
                    if matches!(url.scheme(), "http" | "https") {
                        let _ = url.set_username("");
                        let _ = url.set_password(None);
                    }
                    url.to_string()
                } else {
                    remote.url
                };
                git(
                    stage.path(),
                    &["config", &format!("remote.{}.url", remote.name), &url],
                )
                .await?;
            }
            tokio::fs::rename(stage.path(), &archive).await?;
        }
        let migrated = entry.join("migration-v1.json");
        if !migrated.exists() {
            CommentStore::migrate(&source.data_git_dir, &data).await?;
            let old_iterations = source.data_git_dir.join("diffle/iterations.json");
            with_file_lock(&old_iterations, || async {
                // Source writers pin objects before publishing metadata under this same lock.
                // Copy the refs and their metadata as one consistent migration snapshot.
                git(
                    &archive,
                    &[
                        "fetch",
                        "--no-tags",
                        "--no-write-fetch-head",
                        "--",
                        source.root.to_str().context("Non-UTF8 checkout path")?,
                        "+refs/diffle/iterations/*:refs/diffle/iterations/*",
                    ],
                )
                .await?;
                if old_iterations.exists() {
                    anyhow::ensure!(
                        tokio::fs::metadata(&old_iterations).await?.len() <= 16 * 1024 * 1024,
                        "Iteration metadata exceeds the migration limit; source was preserved"
                    );
                    let text = tokio::fs::read_to_string(&old_iterations).await?;
                    let value: serde_json::Value = serde_json::from_str(&text)?;
                    anyhow::ensure!(
                        value["version"] == 1 && value["ranges"].is_object(),
                        "Unsupported iteration data; source was preserved"
                    );
                    write_file_atomic(&data.join("diffle/iterations.json"), &text).await?;
                }
                Ok(())
            })
            .await?;
            write_file_atomic(&migrated, "{\"version\":1}\n").await?;
        }
        // Update imported public refs without touching iteration refs or another worktree's HEAD.
        git(
            &archive,
            &[
                "fetch",
                "--no-tags",
                "--no-write-fetch-head",
                "--",
                source.root.to_str().context("Non-UTF8 checkout path")?,
                "+refs/heads/*:refs/mdiff/import/heads/*",
                "+refs/remotes/*:refs/mdiff/import/remotes/*",
                "+refs/mdiff/*:refs/mdiff/import/mdiff/*",
            ],
        )
        .await?;
        let head = git(&source.root, &["rev-parse", "HEAD"]).await?;
        let head = std::str::from_utf8(&head)?.trim();
        git(
            &archive,
            &[
                "fetch",
                "--no-tags",
                "--no-write-fetch-head",
                "--",
                source.root.to_str().context("Non-UTF8 checkout path")?,
                head,
            ],
        )
        .await?;
        let checkouts = entry.join("checkouts");
        tokio::fs::create_dir_all(&checkouts).await?;
        // Prune registrations whose disposable directories have already gone away (e.g. a failed open).
        git(&archive, &["worktree", "prune", "--expire", "now"]).await?;
        let directory = tempfile::Builder::new()
            .prefix("review-")
            .tempdir_in(checkouts)?;
        git(
            &archive,
            &[
                "worktree",
                "add",
                "--detach",
                "--",
                directory
                    .path()
                    .to_str()
                    .context("Non-UTF8 checkout path")?,
                head,
            ],
        )
        .await?;
        let path = directory.path().to_owned();
        let checkout = Checkout::lease(Checkout {
            directory: Some(directory),
            archive,
            path,
            fresh: AtomicBool::new(true),
            editable: AtomicBool::new(false),
            head: Mutex::new(head.into()),
            process_lease: None,
        })?;
        let repo = source
            .reopen_at(checkout.path())
            .await?
            .with_data_root(data);
        Ok((repo, checkout))
    })
    .await
}

/// Reconstruct from retained objects without fetching, resolving moving refs or restoring trust.
pub async fn restore(
    archive: &Path,
    head: &str,
    preferred: Option<&Path>,
) -> Result<Arc<Checkout>> {
    anyhow::ensure!(
        [40, 64].contains(&head.len()) && head.bytes().all(|b| b.is_ascii_hexdigit()),
        "Saved revision is not pinned"
    );
    let entry = archive.parent().context("Missing archive parent")?;
    for attempt in 0..30 {
        let restored = with_file_lock(&entry.join("prepare"), || async {
            let preferred = match preferred {
                Some(path) => tokio::fs::canonicalize(path).await.ok(),
                None => None,
            };
            tokio::fs::create_dir_all(entry.join("checkouts")).await?;
            let owned = tokio::fs::canonicalize(entry.join("checkouts")).await?;
            if let Some(path) = preferred
                .as_deref()
                .filter(|p| p.starts_with(&owned) && p.join(".git").exists())
            {
                if let Some(lease) = live_checkout(path) {
                    anyhow::ensure!(
                        lease.archive() == archive,
                        "Retained checkout belongs to another repository"
                    );
                    return Ok(lease);
                }
                let found = git(path, &["rev-parse", "--git-common-dir"]).await?;
                let common = path.join(std::str::from_utf8(&found)?.trim());
                anyhow::ensure!(
                    tokio::fs::canonicalize(common).await?
                        == tokio::fs::canonicalize(archive).await?,
                    "Retained checkout belongs to another repository"
                );
                return Checkout::lease(Checkout {
                    directory: None,
                    path: path.into(),
                    archive: archive.into(),
                    fresh: AtomicBool::new(false),
                    editable: AtomicBool::new(true),
                    head: Mutex::new(head.into()),
                    process_lease: None,
                });
            }
            let checkouts = entry.join("checkouts");
            tokio::fs::create_dir_all(&checkouts).await?;
            let directory = tempfile::Builder::new()
                .prefix("review-")
                .tempdir_in(checkouts)?;
            git(archive, &["worktree", "prune", "--expire", "now"]).await?;
            git(
                archive,
                &[
                    "worktree",
                    "add",
                    "--detach",
                    "--",
                    directory
                        .path()
                        .to_str()
                        .context("Non-UTF8 checkout path")?,
                    head,
                ],
            )
            .await?;
            let path = directory.path().to_owned();
            Checkout::lease(Checkout {
                directory: Some(directory),
                path,
                archive: archive.into(),
                fresh: AtomicBool::new(true),
                editable: AtomicBool::new(false),
                head: Mutex::new(head.into()),
                process_lease: None,
            })
        })
        .await;
        match restored {
            Err(error)
                if error.to_string().contains("Retained checkout is busy") && attempt < 29 =>
            {
                tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            }
            result => return result,
        }
    }
    unreachable!()
}

#[cfg(test)]
mod tests {
    use super::*;
    use diffle::{
        iterations::IterationStore,
        protocol::{CommentAnchor, ThreadQuery},
    };
    fn run(path: &Path, args: &[&str]) -> String {
        let out = std::process::Command::new("git")
            .current_dir(path)
            .args(args)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8(out.stdout).unwrap().trim().into()
    }
    #[tokio::test]
    async fn dirty_checkouts_are_retained_and_reconstructed_without_discarding_edits() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("source");
        std::fs::create_dir(&source).unwrap();
        run(&source, &["init", "-q"]);
        run(&source, &["config", "user.name", "Test"]);
        run(&source, &["config", "user.email", "test@example.test"]);
        run(&source, &["config", "commit.gpgsign", "false"]);
        std::fs::write(source.join("f"), "committed\n").unwrap();
        run(&source, &["add", "."]);
        run(&source, &["commit", "-qm", "one"]);
        let repo = GitRepo::open(&source).await.unwrap();
        let (owned, lease) = adopt(&dir.path().join("durable"), "review", &repo)
            .await
            .unwrap();
        let path = owned.root.clone();
        let archive = lease.archive().to_owned();
        let head = run(&path, &["rev-parse", "HEAD"]);
        std::fs::write(path.join("f"), "unsaved editor changes\n").unwrap();
        drop(owned);
        drop(lease);
        tokio::time::timeout(std::time::Duration::from_secs(5), async {
            loop {
                if std::fs::read_dir(archive.parent().unwrap())
                    .unwrap()
                    .any(|e| {
                        e.unwrap()
                            .file_name()
                            .to_string_lossy()
                            .starts_with("recovery-")
                    })
                {
                    break;
                }
                tokio::time::sleep(std::time::Duration::from_millis(20)).await;
            }
        })
        .await
        .unwrap();
        assert_eq!(
            std::fs::read_to_string(path.join("f")).unwrap(),
            "unsaved editor changes\n"
        );
        let restored = restore(&archive, &head, Some(&path)).await.unwrap();
        assert_eq!(restored.path(), path);
        assert_eq!(
            std::fs::read_to_string(restored.path().join("f")).unwrap(),
            "unsaved editor changes\n"
        );
    }
    #[tokio::test]
    async fn deleting_checkouts_preserves_comments_iterations_and_objects_and_does_not_move_other_heads(
    ) {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("legacy");
        std::fs::create_dir(&source).unwrap();
        run(&source, &["init", "-q"]);
        run(&source, &["config", "user.name", "Test"]);
        run(&source, &["config", "user.email", "test@example.test"]);
        run(&source, &["config", "commit.gpgsign", "false"]);
        std::fs::write(source.join("f"), "one\n").unwrap();
        run(&source, &["add", "."]);
        run(&source, &["commit", "-qm", "one"]);
        let old = run(&source, &["rev-parse", "HEAD"]);
        std::fs::write(source.join("f"), "two\n").unwrap();
        run(&source, &["commit", "-qam", "two"]);
        let new = run(&source, &["rev-parse", "HEAD"]);
        let repo = Arc::new(GitRepo::open(&source).await.unwrap());
        let store = CommentStore::open(&repo.git_dir, "working").await.unwrap();
        store
            .add_thread(CommentAnchor::File { path: "f".into() }, "saved".into())
            .await
            .unwrap();
        IterationStore::new(repo.clone(), "range:review".into())
            .record(&old, &new)
            .await
            .unwrap();
        let root = dir.path().join("durable");
        let (first, lease) = adopt(&root, "gitlab:host/project!1", &repo).await.unwrap();
        let shared = restore(lease.archive(), &new, Some(lease.path()))
            .await
            .unwrap();
        assert!(Arc::ptr_eq(&lease, &shared));
        drop(shared);
        std::fs::write(source.join("f"), "three\n").unwrap();
        run(&source, &["commit", "-qam", "three"]);
        let (second, next) = adopt(&root, "gitlab:host/project!1", &repo).await.unwrap();
        let next_path = next.path().to_owned();
        let next_head = run(&next_path, &["rev-parse", "HEAD"]);
        assert_eq!(run(&first.root, &["rev-parse", "HEAD"]), new);
        assert_ne!(run(&second.root, &["rev-parse", "HEAD"]), new);
        let data = first.data_git_dir.clone();
        let archive = lease.archive.clone();
        drop(first);
        drop(second);
        drop(lease);
        drop(next);
        // Resume immediately, while the old lease's asynchronous cleanup may be queued.
        let resumed = restore(&archive, &next_head, Some(&next_path))
            .await
            .unwrap();
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        assert_eq!(run(resumed.path(), &["rev-parse", "HEAD"]), next_head);
        drop(resumed);
        // Simulate complete loss of the source and all disposable checkout directories.
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        std::fs::remove_dir_all(&source).unwrap();
        run(&archive, &["reflog", "expire", "--expire=now", "--all"]);
        run(&archive, &["gc", "--prune=now"]);
        run(&archive, &["cat-file", "-e", &format!("{old}^{{commit}}")]);
        let comments = CommentStore::open(&data, "working")
            .await
            .unwrap()
            .threads(&ThreadQuery::default())
            .await
            .unwrap();
        assert_eq!(comments[0].messages[0].body, "saved");
        assert!(data.join("diffle/iterations.json").exists());
        let recreated = dir.path().join("recreated");
        run(
            &archive,
            &[
                "worktree",
                "add",
                "--detach",
                recreated.to_str().unwrap(),
                &new,
            ],
        );
        let reopened = Arc::new(
            GitRepo::open(&recreated)
                .await
                .unwrap()
                .with_data_root(data),
        );
        assert_eq!(
            IterationStore::new(reopened, "range:review".into())
                .list()
                .await
                .unwrap()
                .len(),
            1
        );
    }
}
