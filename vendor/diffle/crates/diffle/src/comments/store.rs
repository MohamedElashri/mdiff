//! Indexed, transactional threads and viewed marks in `<gitDir>/diffle/comments.sqlite`.
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::anchor::compare_threads;
use crate::error::{Error, Result};
use crate::iterations::now_ms;
use crate::persist::with_file_lock;
use crate::protocol::{CommentAnchor, CommentMessage, CommentThread, ThreadQuery, ThreadState, ViewedEntry};

/// Viewed marks kept per path: the current one plus the newest previous, so `restale` is derivable.
const VIEWED_HISTORY: usize = 2;

#[derive(Default, Serialize, Deserialize)]
pub(super) struct SetData {
    #[serde(default)]
    pub(super) contexts: BTreeMap<String, super::anchor::Context>,
    #[serde(default)]
    pub(super) threads: Vec<CommentThread>,
    #[serde(default)]
    pub(super) viewed: Vec<ViewedEntry>,
}

#[derive(Serialize, Deserialize)]
pub(super) struct StoreFile {
    pub(super) version: u8,
    pub(super) sets: BTreeMap<String, SetData>,
}

/// One review's data. Writers serialize with a cross-process lock and commit changed rows in
/// one SQLite transaction; readers use indexed queries and never parse other reviews.
pub struct CommentStore {
    pub(super) file: PathBuf,
    pub(super) key: String,
}

impl CommentStore {
    pub async fn open(git_dir: &Path, key: &str) -> Result<Self> {
        Ok(Self { file: git_dir.join("diffle").join("comments.sqlite"), key: key.to_owned() })
    }

    pub fn key(&self) -> &str {
        &self.key
    }

    pub fn file(&self) -> &Path {
        &self.file
    }

    /// Threads matching `q` (every thread by default), ordered by path, line, then creation.
    pub async fn threads(&self, q: &ThreadQuery) -> Result<Vec<CommentThread>> {
        let resolved = match q.state.unwrap_or(ThreadState::All) {
            ThreadState::Open => Some(false),
            ThreadState::Resolved => Some(true),
            ThreadState::All => None,
        };
        let mut threads = self.query_threads(q.path.clone(), resolved, None).await?;
        threads.sort_by(compare_threads);
        Ok(threads)
    }

    async fn query_threads(
        &self,
        path: Option<String>,
        resolved: Option<bool>,
        id: Option<String>,
    ) -> Result<Vec<CommentThread>> {
        let (file, key) = (self.file.clone(), self.key.clone());
        with_file_lock(&self.file, || async move {
            tokio::task::spawn_blocking(move || {
                super::database::threads(
                    &super::database::connect(&file)?,
                    &key,
                    path.as_deref(),
                    resolved,
                    id.as_deref(),
                )
            })
            .await?
        })
        .await
        .map_err(Into::into)
    }

    pub async fn get(&self, id: &str) -> Result<Option<CommentThread>> {
        Ok(self.query_threads(None, None, Some(id.to_owned())).await?.into_iter().next())
    }

    pub async fn add_thread(&self, anchor: CommentAnchor, body: String) -> Result<CommentThread> {
        self.mutate(|set| {
            let t = new_thread(anchor, body);
            set.threads.push(t.clone());
            Ok((t, true))
        })
        .await
    }

    pub async fn reply(&self, thread_id: &str, body: String) -> Result<CommentThread> {
        self.mutate(|set| {
            let t = find_thread(set, thread_id)?;
            t.messages.push(new_message(body));
            Ok((t.clone(), true))
        })
        .await
    }

    pub async fn edit_message(&self, thread_id: &str, message_id: &str, body: String) -> Result<CommentThread> {
        self.mutate(|set| {
            let t = find_thread(set, thread_id)?;
            let m =
                t.messages.iter_mut().find(|m| m.id == message_id).ok_or_else(|| Error::NotFound(message_id.into()))?;
            m.body = body;
            m.updated_at = now_ms();
            Ok((t.clone(), true))
        })
        .await
    }

    /// Removes the message, and the thread with its last message (returning none then).
    pub async fn remove_message(&self, thread_id: &str, message_id: &str) -> Result<Option<CommentThread>> {
        self.mutate(|set| {
            let t = find_thread(set, thread_id)?;
            let before = t.messages.len();
            t.messages.retain(|m| m.id != message_id);
            if t.messages.len() == before {
                return Err(Error::NotFound(message_id.into()));
            }
            if !t.messages.is_empty() {
                return Ok((Some(t.clone()), true));
            }
            set.threads.retain(|t| t.id != thread_id);
            Ok((None, true))
        })
        .await
    }

    pub async fn set_resolved(&self, thread_id: &str, resolved: bool) -> Result<CommentThread> {
        self.mutate(|set| {
            let t = find_thread(set, thread_id)?;
            t.resolved = resolved;
            t.resolved_at = resolved.then(now_ms);
            Ok((t.clone(), true))
        })
        .await
    }

    pub async fn remove_thread(&self, id: &str) -> Result<()> {
        self.mutate(|set| {
            let before = set.threads.len();
            set.threads.retain(|t| t.id != id);
            if set.threads.len() == before {
                return Err(Error::NotFound(id.into()));
            }
            Ok(((), true))
        })
        .await
    }

    pub async fn clear(&self) -> Result<()> {
        self.mutate(|set| {
            set.threads.clear();
            Ok(((), true))
        })
        .await
    }

    /// Drops every stale thread and returns how many went.
    pub async fn remove_stale(&self) -> Result<usize> {
        self.mutate(|set| {
            let before = set.threads.len();
            set.threads.retain(|t| !t.stale);
            let gone = before - set.threads.len();
            Ok((gone, gone > 0))
        })
        .await
    }

    pub async fn viewed(&self) -> Result<Vec<ViewedEntry>> {
        Ok(self.read().await?.viewed)
    }

    /// Marks or unmarks a path as viewed at a given new-side blob. The newest previous entry for the
    /// path is kept so a later blob can be recognised as re-touched.
    pub async fn set_viewed(&self, path: &str, blob: &str, viewed: bool) -> Result<Vec<ViewedEntry>> {
        self.mutate(|set| {
            let (mine, others): (Vec<ViewedEntry>, Vec<ViewedEntry>) =
                set.viewed.drain(..).partition(|v| v.path == path);
            let history: Vec<ViewedEntry> = mine.into_iter().filter(|v| v.blob != blob).collect();
            let keep = history.len().saturating_sub(VIEWED_HISTORY - 1);
            set.viewed = others;
            set.viewed.extend(history.into_iter().skip(keep));
            set.viewed.push(ViewedEntry { path: path.into(), blob: blob.into(), viewed });
            Ok((set.viewed.clone(), true))
        })
        .await
    }

    /// Replaces the marks for many paths at once; their history is dropped.
    pub async fn set_viewed_many(&self, entries: Vec<ViewedEntry>) -> Result<Vec<ViewedEntry>> {
        self.mutate(|set| {
            set.viewed.retain(|v| !entries.iter().any(|e| e.path == v.path));
            set.viewed.extend(entries);
            Ok((set.viewed.clone(), true))
        })
        .await
    }

    /// Read one review under the same lock used by writers and legacy migration.
    pub(super) async fn read(&self) -> Result<SetData> {
        with_file_lock(&self.file, || async { read_set(&self.file, &self.key).await }).await.map_err(Into::into)
    }

    /// Search discussions across saved reviews; each result retains its review key for resuming.
    pub async fn search_history(&self, query: &str) -> Result<Vec<(String, CommentThread)>> {
        let file = self.file.clone();
        let query = query.to_owned();
        with_file_lock(&self.file, || async move {
            tokio::task::spawn_blocking(move || super::database::search(&super::database::connect(&file)?, &query))
                .await?
        })
        .await
        .map_err(Into::into)
    }

    /// Locked read-modify-write: `f` edits this key's set as read from disk under the lock, other
    /// keys remain untouched, and changed rows commit atomically when `f` reports a change. A failing
    /// `f` writes nothing; a failed write fails its caller only.
    async fn mutate<R>(&self, f: impl FnOnce(&mut SetData) -> Result<(R, bool)>) -> Result<R> {
        with_file_lock(&self.file, || async {
            let mut set = read_set(&self.file, &self.key).await?;
            let (r, changed) = match f(&mut set) {
                Ok(done) => done,
                Err(e) => return Ok(Err(e)),
            };
            if changed {
                write_set(&self.file, &self.key, set).await?;
            }
            Ok(Ok(r))
        })
        .await?
    }
}

fn find_thread<'a>(set: &'a mut SetData, id: &str) -> Result<&'a mut CommentThread> {
    set.threads.iter_mut().find(|t| t.id == id).ok_or_else(|| Error::NotFound(id.into()))
}

pub(super) async fn read_set(file: &Path, key: &str) -> anyhow::Result<SetData> {
    let (file, key) = (file.to_owned(), key.to_owned());
    tokio::task::spawn_blocking(move || super::database::read(&super::database::connect(&file)?, &key)).await?
}

pub(super) async fn write_set(file: &Path, key: &str, set: SetData) -> anyhow::Result<()> {
    let (file, key) = (file.to_owned(), key.to_owned());
    tokio::task::spawn_blocking(move || {
        let mut db = super::database::connect(&file)?;
        let tx = db.transaction()?;
        super::database::write(&tx, &key, &set)?;
        tx.commit()?;
        Ok(())
    })
    .await?
}

fn new_message(body: String) -> CommentMessage {
    let now = now_ms();
    CommentMessage { id: uuid::Uuid::new_v4().to_string(), body, created_at: now, updated_at: now }
}

pub(super) fn new_thread(anchor: CommentAnchor, body: String) -> CommentThread {
    CommentThread {
        id: uuid::Uuid::new_v4().to_string(),
        anchor,
        messages: vec![new_message(body)],
        resolved: false,
        resolved_at: None,
        stale: false,
        stale_from_line: None,
    }
}
