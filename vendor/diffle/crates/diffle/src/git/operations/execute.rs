use super::*;
use anyhow::{Context, Result};
use std::path::{Path, PathBuf};
use tokio::sync::watch;
struct IndexLease {
    file: std::fs::File,
    path: PathBuf,
}
impl IndexLease {
    fn acquire(git: &Path) -> Result<Self> {
        let path = git.join("index.lock");
        let file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .context("Git index is locked; finish the other Git operation before retrying")?;
        Ok(Self { file, path })
    }
}
impl Drop for IndexLease {
    fn drop(&mut self) {
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            if let (Ok(ours), Ok(current)) = (self.file.metadata(), std::fs::symlink_metadata(&self.path))
                && ours.dev() == current.dev()
                && ours.ino() == current.ino()
            {
                let _ = std::fs::remove_file(&self.path);
            }
        }
    }
}
/// Caller must own repository/worktree admission for the entire future. Native jobs
/// keep it alive independently of IPC, cancellation and window lifetime.
pub async fn execute(plan: Plan, cancel: watch::Receiver<bool>) -> Result<Outcome> {
    anyhow::ensure!(cfg!(unix), "Git write primitives currently support macOS and Linux");
    anyhow::ensure!(!*cancel.borrow(), "Cancelled before Git started");
    revalidate(&plan).await?;
    anyhow::ensure!(!*cancel.borrow(), "Cancelled before Git started");
    let root = &plan.before.identity.root;
    let mut index_lease = None;
    let mut alternate = None;
    let private_index = plan.action.index_only() || matches!(plan.action, Action::ResolveText { .. });
    if private_index {
        index_lease = Some(IndexLease::acquire(&plan.before.identity.git)?);
        revalidate(&plan).await?;
        let file = tempfile::NamedTempFile::new_in(&plan.before.identity.git)?;
        let index = plan.before.identity.git.join("index");
        if index.exists() {
            tokio::fs::copy(index, file.path()).await?;
        } else {
            tokio::fs::remove_file(file.path()).await?;
        }
        alternate = Some(file);
    }
    if let (Some(expected), Some(path)) = (&plan.patch, plan.action.path()) {
        let staged = matches!(plan.action, Action::Unstage { .. } | Action::Hunk { staged: true, .. });
        let fresh = super::patch(root, path, staged).await?;
        if matches!(plan.action, Action::Hunk { .. }) {
            anyhow::ensure!(fresh.partial, "File attributes or patch eligibility changed; preview again");
        }
        anyhow::ensure!(
            fresh.fingerprint == expected.fingerprint,
            "Canonical patch changed; refresh and preview again"
        );
    }
    if matches!(plan.action, Action::WorktreeRemove { .. }) {
        let fresh = super::prepare(root, plan.action.clone()).await?;
        anyhow::ensure!(
            fresh.before == plan.before && fresh.extra == plan.extra,
            "Worktree changed before removal; preview again"
        );
    }
    if let Some(path) = &plan.destination {
        tokio::fs::create_dir(path).await.context("Destination changed; refusing to reuse an existing folder")?;
    }
    let editor = super::advanced::sequence_editor(&plan.action)?;
    if let Action::ResolveText { path, content, expected } = &plan.action {
        let fresh = super::advanced::conflict(root, path).await?;
        anyhow::ensure!(fresh.supported && fresh.expected == *expected, "Conflict changed before writing; reload");
        anyhow::ensure!(!*cancel.borrow(), "Cancelled before writing resolution");
        let root = root.clone();
        let path = path.clone();
        let content = content.clone();
        let inode = plan.before.identity.inodes[0];
        tokio::task::spawn_blocking(move || super::advanced::write_result(&root, &path, content.as_bytes(), inode))
            .await??;
    }
    let mut status = "completed";
    let mut message = "Git completed. Current repository state was re-read.".to_string();
    if let Action::Discard { path } = &plan.action {
        anyhow::ensure!(!*cancel.borrow(), "Cancelled before file deletion");
        revalidate(&plan).await?;
        validate_path(root, path).await?;
        anyhow::ensure!(!*cancel.borrow(), "Cancelled before deleting the file");
        let root = root.clone();
        let path = path.clone();
        let expected = *plan.before.identity.inodes.first().context("Missing checkout identity")?;
        tokio::task::spawn_blocking(move || unlink(&root, &path, expected)).await??;
    } else {
        for (i, step) in plan.steps.iter().enumerate() {
            if i > 0 && matches!(plan.action, Action::StashPop { .. }) {
                let latest = text(read(root, &["rev-parse", "--verify", "refs/stash"]).await?)?;
                if !matches!(&plan.action,Action::StashPop{oid} if oid==&latest) {
                    status = "needsInspection";
                    message =
                        "Stash applied, but the stash list changed. Nothing was dropped; inspect it externally.".into();
                    break;
                }
            }
            let result = runner::run_with_editor(
                root,
                &step.args,
                step.input.clone(),
                alternate.as_ref().map(|file| file.path()),
                cancel.clone(),
                plan.action.network(),
                editor.as_ref().map(|(_, path)| path.as_path()),
            )
            .await;
            match result {
                Ok(exit) if !exit.interrupted && exit.code == Some(0) => (),
                Ok(exit) => {
                    status = "needsInspection";
                    message = if exit.interrupted {
                        "Git was interrupted. Some changes may have completed; inspect current state before retrying."
                            .into()
                    } else {
                        format!(
                            "Git exited with code {:?}. Check hooks, signing, credentials, conflicts and Git locks in your external client. Completed earlier steps are not rolled back.",
                            exit.code
                        )
                    };
                    break;
                }
                Err(error) => {
                    status = "needsInspection";
                    message = error.to_string();
                    break;
                }
            }
        }
    }
    if status == "completed" && private_index && *cancel.borrow() {
        status = "needsInspection";
        message="Cancelled before installing the staged index; the original index was kept. Configured filters may already have run.".into();
    }
    if status == "completed"
        && let (Some(lease), Some(alternate)) = (&mut index_lease, &alternate)
    {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let mut bytes = Vec::new();
        tokio::fs::File::open(alternate.path()).await?.take(64 * 1024 * 1024 + 1).read_to_end(&mut bytes).await?;
        anyhow::ensure!(
            bytes.len() <= 64 * 1024 * 1024,
            "Resulting index exceeds its 64 MiB limit; original index was kept"
        );
        let mut file = tokio::fs::File::from_std(lease.file.try_clone()?);
        file.set_len(0).await?;
        file.write_all(&bytes).await?;
        file.sync_all().await?;
        tokio::fs::rename(&lease.path, plan.before.identity.git.join("index")).await?;
    }
    // Fresh, read-only evidence is useful even after interruption. No mutation is retried.
    let after = inspect(root, &plan.action).await.ok();
    if status == "completed" && plan.action.advanced() {
        if after.is_none() {
            status = "needsInspection";
            message="Git exited successfully, but its resulting state could not be read. Inspect externally before continuing.".into();
        } else if !matches!(plan.action, Action::ResolveText { .. })
            && after.as_ref().is_some_and(|state| state.conflicts || !state.operations.is_empty())
        {
            status = "needsInspection";
            message="Git stopped with remaining operation state or conflicts. Open Advanced and recovery; no automatic continuation or rollback was attempted.".into();
        }
    }
    if status == "completed"
        && matches!(plan.action, Action::StashSave { .. })
        && after.as_ref().is_some_and(|after| after.stash_fingerprint == plan.before.stash_fingerprint)
    {
        status = "noChanges";
        message = "Git found no changes eligible for a stash; no new stash entry was created.".into();
    }
    Ok(Outcome { status: status.into(), message, after })
}

#[cfg(unix)]
fn unlink(root: &Path, path: &str, expected: (u64, u64)) -> Result<()> {
    use std::{
        ffi::CString,
        os::{
            fd::{AsRawFd, FromRawFd},
            unix::{
                ffi::OsStrExt,
                fs::{MetadataExt, OpenOptionsExt},
            },
        },
    };
    let mut folder = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(root)?;
    let metadata = folder.metadata()?;
    anyhow::ensure!((metadata.dev(), metadata.ino()) == expected, "Checkout identity changed before file deletion");
    let components = Path::new(path).components().collect::<Vec<_>>();
    let (leaf, parents) = components.split_last().context("Missing file name")?;
    for part in parents {
        let part = CString::new(part.as_os_str().as_bytes())?;
        let fd = unsafe {
            libc::openat(
                folder.as_raw_fd(),
                part.as_ptr(),
                libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
            )
        };
        if fd < 0 {
            return Err(std::io::Error::last_os_error()).context("A parent directory changed; nothing was deleted");
        }
        folder = unsafe { std::fs::File::from_raw_fd(fd) };
    }
    let leaf = CString::new(leaf.as_os_str().as_bytes())?;
    let mut metadata = std::mem::MaybeUninit::<libc::stat>::uninit();
    let result =
        unsafe { libc::fstatat(folder.as_raw_fd(), leaf.as_ptr(), metadata.as_mut_ptr(), libc::AT_SYMLINK_NOFOLLOW) };
    if result < 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    let metadata = unsafe { metadata.assume_init() };
    anyhow::ensure!(metadata.st_mode & libc::S_IFMT == libc::S_IFREG, "Selected path is no longer a regular file");
    if unsafe { libc::unlinkat(folder.as_raw_fd(), leaf.as_ptr(), 0) } < 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    Ok(())
}
#[cfg(not(unix))]
fn unlink(_root: &Path, _path: &str, _expected: (u64, u64)) -> Result<()> {
    anyhow::bail!("Git writes support macOS and Linux")
}
