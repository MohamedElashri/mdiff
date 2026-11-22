use anyhow::{Context, Result};
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{Arc, Mutex, Weak},
};
use tokio::sync::{Mutex as AsyncMutex, RwLock};
#[derive(Default)]
pub struct Locks {
    refs: Mutex<HashMap<PathBuf, Weak<RwLock<()>>>>,
    indexes: Mutex<HashMap<PathBuf, Weak<AsyncMutex<()>>>>,
}
fn resource<T: Default>(map: &Mutex<HashMap<PathBuf, Weak<T>>>, path: &Path) -> Arc<T> {
    let mut values = map.lock().unwrap();
    values.retain(|_, v| v.strong_count() > 0);
    if let Some(lock) = values.get(path).and_then(Weak::upgrade) {
        lock
    } else {
        let lock = Arc::new(T::default());
        values.insert(path.into(), Arc::downgrade(&lock));
        lock
    }
}
pub struct Held {
    _read: Option<tokio::sync::OwnedRwLockReadGuard<()>>,
    _write: Option<tokio::sync::OwnedRwLockWriteGuard<()>>,
    _index: tokio::sync::OwnedMutexGuard<()>,
    _files: Vec<std::fs::File>,
}
async fn file(path: &Path, shared: bool) -> Result<std::fs::File> {
    tokio::fs::create_dir_all(path.parent().context("Missing operation lock parent")?).await?;
    let mut options = std::fs::OpenOptions::new();
    options.read(true).write(true).create(true).truncate(false);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let file = options.open(path)?;
    let until = std::time::Instant::now() + std::time::Duration::from_secs(10);
    loop {
        let result = if shared {
            file.try_lock_shared()
        } else {
            file.try_lock()
        };
        match result {
            Ok(()) => return Ok(file),
            Err(std::fs::TryLockError::WouldBlock) => (),
            Err(std::fs::TryLockError::Error(error)) => return Err(error.into()),
        }
        anyhow::ensure!(
            std::time::Instant::now() < until,
            "Another mdiff process owns this operation resource"
        );
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
}
impl Locks {
    pub async fn acquire(
        &self,
        identity: &diffle::git::operations::Identity,
        index_only: bool,
    ) -> Result<Held> {
        let refs = resource(&self.refs, &identity.common);
        let index = resource(&self.indexes, &identity.git);
        let (read, write) = if index_only {
            (Some(refs.read_owned().await), None)
        } else {
            (None, Some(refs.write_owned().await))
        };
        let index = index.lock_owned().await;
        let common = file(
            &identity.common.join("diffle/operations-refs.lock"),
            index_only,
        )
        .await?;
        let worktree = file(&identity.git.join("diffle/operations-index.lock"), false).await?;
        Ok(Held {
            _read: read,
            _write: write,
            _index: index,
            _files: vec![common, worktree],
        })
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn identity(root: &Path, common: &Path) -> diffle::git::operations::Identity {
        diffle::git::operations::Identity {
            root: root.into(),
            git: root.join("git"),
            common: common.into(),
            inodes: vec![],
        }
    }
    #[tokio::test]
    async fn linked_indexes_progress_independently_while_refs_serialize_and_other_repositories_progress(
    ) {
        let temp = tempfile::tempdir().unwrap();
        let common = temp.path().join("common");
        let one = identity(&temp.path().join("one"), &common);
        let two = identity(&temp.path().join("two"), &common);
        let other = identity(
            &temp.path().join("other"),
            &temp.path().join("other-common"),
        );
        let locks = Locks::default();
        let first = locks.acquire(&one, true).await.unwrap();
        let second =
            tokio::time::timeout(std::time::Duration::from_secs(1), locks.acquire(&two, true))
                .await
                .unwrap()
                .unwrap();
        assert!(tokio::time::timeout(
            std::time::Duration::from_millis(50),
            locks.acquire(&one, false)
        )
        .await
        .is_err());
        let unrelated = tokio::time::timeout(
            std::time::Duration::from_secs(1),
            locks.acquire(&other, false),
        )
        .await
        .unwrap()
        .unwrap();
        drop((first, second, unrelated));
        assert!(locks.acquire(&one, false).await.is_ok());
    }
    #[tokio::test]
    async fn independent_coordinators_honor_the_same_cross_process_lease() {
        let temp = tempfile::tempdir().unwrap();
        let id = identity(&temp.path().join("one"), &temp.path().join("common"));
        let first = Locks::default();
        let second = Locks::default();
        let held = first.acquire(&id, false).await.unwrap();
        let wait = second.acquire(&id, true);
        tokio::pin!(wait);
        assert!(
            tokio::time::timeout(std::time::Duration::from_millis(50), &mut wait)
                .await
                .is_err()
        );
        drop(held);
        assert!(
            tokio::time::timeout(std::time::Duration::from_secs(1), wait)
                .await
                .unwrap()
                .is_ok()
        );
    }
}
