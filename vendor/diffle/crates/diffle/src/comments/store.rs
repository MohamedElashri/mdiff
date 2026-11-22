//! Indexed, transactional threads and viewed marks in `<gitDir>/diffle/comments.sqlite`.
use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};

use rusqlite::{Connection, OptionalExtension};
use serde::{Deserialize, Serialize};

use super::anchor::compare_threads;
use super::database;
use crate::error::{Error, Result};
use crate::iterations::now_ms;
use crate::persist::{with_file_lock, with_file_lock_blocking};
use crate::protocol::{CommentAnchor, CommentMessage, CommentThread, ThreadQuery, ThreadState, ViewedEntry};

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
    /// Merge a legacy checkout's data into a durable root. Read a transactionally consistent
    /// snapshot under its existing lock; preserve the source for recovery. Existing target
    /// records win conflicts, so retrying migration cannot undo newer edits.
    pub async fn migrate(from: &Path, to: &Path) -> Result<()> {
        Self::import(from, to, None, None).await
    }
    /// Legacy MR checkouts contained just one logical review but keyed each resolved range
    /// separately. Merge their saved discussions into that MR's durable identity; keep originals.
    pub async fn migrate_ranges(root: &Path, key: String) -> Result<()> {
        Self::import(root, root, Some(key), None).await
    }
    pub async fn migrate_ranges_current(root: &Path, key: String, current: String) -> Result<()> {
        Self::import(root, root, Some(key), Some(current)).await
    }
    async fn import(from: &Path, to: &Path, range_key: Option<String>, preferred: Option<String>) -> Result<()> {
        let source = from.join("diffle/comments.sqlite");
        let target = to.join("diffle/comments.sqlite");
        if !source.exists() && !source.with_extension("json").exists() {
            return Ok(());
        }
        let migration = format!("import:{}:{}", source.display(), range_key.as_deref().unwrap_or("all"));
        let mut paths = vec![source.clone(), target.clone()];
        paths.sort();
        paths.dedup();
        let mut locks = vec![];
        for path in paths {
            locks.push(crate::persist::lock_file(&path).await?);
        }
        tokio::task::spawn_blocking(move ||->anyhow::Result<()> {
            // Both locks stay with this task if its async caller disappears. SQLite copies
            // indexed rows directly: discussions are never materialized as one giant Vec.
            let _locks=locks;
            drop(database::connect(&source)?);
            let mut db=database::connect(&target)?;
            let schema=if source==target {"main"}else {db.execute("ATTACH DATABASE ?1 AS legacy",[source.to_str().ok_or_else(||anyhow::anyhow!("Non-UTF8 data path"))?])?;"legacy"};
            let tx=db.transaction()?;
            let imported:Option<String>=tx.query_row("SELECT value FROM metadata WHERE key=?1",[&migration],|r|r.get(0)).optional()?;
            if imported.is_some(){return Ok(())}
            const LIMIT:i64=16*1024*1024;
            let largest:i64=tx.query_row(&format!("SELECT COALESCE(MAX(length(data)),0) FROM {schema}.threads"),[],|r|r.get(0))?;
            anyhow::ensure!(largest<=LIMIT,"Legacy discussion exceeds the migration limit; source data was preserved");
            {
                let mut stmt=tx.prepare(&format!("SELECT key,length(viewed),viewed FROM {schema}.reviews ORDER BY key=?1 DESC"))?;
                let mut rows=stmt.query([preferred.unwrap_or_default()])?;
                while let Some(row)=rows.next()? {
                    let source_key:String=row.get(0)?;
                    let key=match &range_key {Some(key) if source_key.starts_with("range:")&&!source_key.contains(":pair:")&&!source_key.contains(":interdiff:")=>key.clone(),Some(_)=>continue,None=>source_key.clone()};
                    let size:i64=row.get(1)?;
                    let existing:i64=tx.query_row("SELECT length(viewed) FROM reviews WHERE key=?1",[&key],|r|r.get(0)).optional()?.unwrap_or(0);
                    anyhow::ensure!(size<=LIMIT&&existing<=LIMIT,"Legacy viewed state exceeds the migration limit; source data was preserved");
                    let text:String=row.get(2)?;let incoming:Vec<ViewedEntry>=serde_json::from_str(&text)?;
                    let mut viewed=database::viewed(&tx,&key)?;let paths:HashSet<_>=viewed.iter().map(|v|v.path.clone()).collect();
                    viewed.extend(incoming.into_iter().filter(|v|!paths.contains(&v.path)));
                    database::write_viewed(&tx,&key,&viewed)?;
                    // Populate FTS only for records that will be new; target edits win.
                    tx.execute(&format!("INSERT INTO discussion_search(review,id,body) SELECT ?1,s.id,s.body FROM {schema}.discussion_search s WHERE s.review=?2 AND NOT EXISTS(SELECT 1 FROM main.threads t WHERE t.review=?1 AND t.id=s.id)"),rusqlite::params![key,source_key])?;
                    tx.execute(&format!("INSERT OR IGNORE INTO threads(review,id,path,resolved,data,context) SELECT ?1,id,path,resolved,data,context FROM {schema}.threads WHERE review=?2"),rusqlite::params![key,source_key])?;
                }
            }
            tx.execute("INSERT INTO metadata(key,value) VALUES(?1,'1')",[migration])?;tx.commit()?;Ok(())
        }).await.map_err(anyhow::Error::from)??;
        Ok(())
    }

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
        self.with_database(move |db, key| database::threads(db, key, path.as_deref(), resolved, id.as_deref())).await
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
        self.with_database(|db, key| database::viewed(db, key)).await
    }

    /// Marks or unmarks a path as viewed at a given new-side blob. The newest previous entry for the
    /// path is kept so a later blob can be recognised as re-touched.
    pub async fn set_viewed(&self, path: &str, blob: &str, viewed: bool) -> Result<Vec<ViewedEntry>> {
        let entry = ViewedEntry { path: path.into(), blob: blob.into(), viewed };
        self.update_viewed(move |marks| {
            let previous = marks.iter().rfind(|mark| mark.path == entry.path && mark.blob != entry.blob).cloned();
            marks.retain(|mark| mark.path != entry.path);
            marks.extend(previous);
            marks.push(entry);
        })
        .await
    }

    /// Replaces the marks for many paths at once; their history is dropped.
    pub async fn set_viewed_many(&self, entries: Vec<ViewedEntry>) -> Result<Vec<ViewedEntry>> {
        self.update_viewed(move |marks| {
            let paths: HashSet<_> = entries.iter().map(|entry| entry.path.as_str()).collect();
            marks.retain(|mark| !paths.contains(mark.path.as_str()));
            marks.extend(entries);
        })
        .await
    }

    async fn update_viewed(
        &self,
        change: impl FnOnce(&mut Vec<ViewedEntry>) + Send + 'static,
    ) -> Result<Vec<ViewedEntry>> {
        self.with_database(move |db, key| {
            let transaction = db.transaction()?;
            let mut marks = database::viewed(&transaction, key)?;
            change(&mut marks);
            database::write_viewed(&transaction, key, &marks)?;
            transaction.commit()?;
            Ok(marks)
        })
        .await
    }

    async fn with_database<R: Send + 'static>(
        &self,
        f: impl FnOnce(&mut Connection, &str) -> anyhow::Result<R> + Send + 'static,
    ) -> Result<R> {
        let (file, key) = (self.file.clone(), self.key.clone());
        with_file_lock_blocking(&self.file, move || f(&mut database::connect(&file)?, &key)).await.map_err(Into::into)
    }

    /// Search discussions across saved reviews; each result retains its review key for resuming.
    pub async fn search_history(&self, query: &str) -> Result<Vec<(String, CommentThread)>> {
        let query = query.to_owned();
        self.with_database(move |db, _| database::search(db, &query)).await
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
