//! Indexed review storage. Legacy JSON is read once and retained unchanged for recovery.
use super::anchor::anchor_path;
use super::store::{SetData, StoreFile};
use crate::protocol::CommentThread;
use rusqlite::{Connection, OptionalExtension, params};
use std::path::Path;

pub(super) fn connect(file: &Path) -> anyhow::Result<Connection> {
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(false);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    drop(options.open(file)?);
    let mut db = Connection::open(file)?;
    db.busy_timeout(std::time::Duration::from_secs(10))?;
    db.execute_batch(
        "PRAGMA foreign_keys=ON;
        CREATE TABLE IF NOT EXISTS metadata (key TEXT PRIMARY KEY, value TEXT NOT NULL);
        CREATE TABLE IF NOT EXISTS reviews (key TEXT PRIMARY KEY, viewed TEXT NOT NULL);
        CREATE TABLE IF NOT EXISTS threads (review TEXT NOT NULL, id TEXT NOT NULL, path TEXT NOT NULL,
            resolved INTEGER NOT NULL, data TEXT NOT NULL, context TEXT,
            PRIMARY KEY(review,id), FOREIGN KEY(review) REFERENCES reviews(key));
        CREATE INDEX IF NOT EXISTS threads_path ON threads(review,path,resolved);
        CREATE VIRTUAL TABLE IF NOT EXISTS discussion_search USING fts5(review UNINDEXED,id UNINDEXED,body);",
    )?;
    let migrated: Option<String> =
        db.query_row("SELECT value FROM metadata WHERE key='migration'", [], |r| r.get(0)).optional()?;
    if migrated.is_none() {
        let legacy = file.with_extension("json");
        let data = match std::fs::read_to_string(&legacy) {
            Ok(text) => {
                let value: serde_json::Value = serde_json::from_str(&text)?;
                anyhow::ensure!(
                    value["version"] == 3 && value["sets"].is_object(),
                    "Unsupported review data format; original comments.json was preserved"
                );
                Some(serde_json::from_value::<StoreFile>(value)?)
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
            Err(e) => return Err(e.into()),
        };
        let tx = db.transaction()?;
        if let Some(data) = data {
            for (key, set) in data.sets {
                write(&tx, &key, &set)?;
            }
        }
        tx.execute("INSERT INTO metadata(key,value) VALUES('migration','1')", [])?;
        tx.commit()?;
    }
    Ok(db)
}

pub(super) fn read(db: &Connection, key: &str) -> anyhow::Result<SetData> {
    let viewed: Option<String> =
        db.query_row("SELECT viewed FROM reviews WHERE key=?1", [key], |r| r.get(0)).optional()?;
    let mut set = SetData::default();
    if let Some(viewed) = viewed {
        set.viewed = serde_json::from_str(&viewed)?;
    }
    let mut stmt = db.prepare("SELECT id,data,context FROM threads WHERE review=?1 ORDER BY rowid")?;
    for row in
        stmt.query_map([key], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, Option<String>>(2)?)))?
    {
        let (id, data, context) = row?;
        set.threads.push(serde_json::from_str(&data)?);
        if let Some(context) = context {
            set.contexts.insert(id, serde_json::from_str(&context)?);
        }
    }
    Ok(set)
}

pub(super) fn write(db: &Connection, key: &str, set: &SetData) -> anyhow::Result<()> {
    db.execute("INSERT INTO reviews(key,viewed) VALUES(?1,?2) ON CONFLICT(key) DO UPDATE SET viewed=excluded.viewed WHERE viewed!=excluded.viewed", params![key,serde_json::to_string(&set.viewed)?])?;
    let ids: std::collections::HashSet<&str> = set.threads.iter().map(|t| t.id.as_str()).collect();
    let previous: Vec<String> = db
        .prepare("SELECT id FROM threads WHERE review=?1")?
        .query_map([key], |r| r.get(0))?
        .collect::<Result<_, _>>()?;
    for id in previous.iter().filter(|id| !ids.contains(id.as_str())) {
        db.execute("DELETE FROM threads WHERE review=?1 AND id=?2", params![key, id])?;
        db.execute("DELETE FROM discussion_search WHERE review=?1 AND id=?2", params![key, id])?;
    }
    for thread in &set.threads {
        let changed = db.execute("INSERT INTO threads(review,id,path,resolved,data,context) VALUES(?1,?2,?3,?4,?5,?6)
            ON CONFLICT(review,id) DO UPDATE SET path=excluded.path,resolved=excluded.resolved,data=excluded.data,context=excluded.context
            WHERE data!=excluded.data OR context IS NOT excluded.context", params![key,thread.id,anchor_path(&thread.anchor),thread.resolved,serde_json::to_string(thread)?,set.contexts.get(&thread.id).map(serde_json::to_string).transpose()?])?;
        if changed != 0 {
            db.execute("DELETE FROM discussion_search WHERE review=?1 AND id=?2", params![key, thread.id])?;
            let body = thread.messages.iter().map(|m| m.body.as_str()).collect::<Vec<_>>().join("\n");
            db.execute(
                "INSERT INTO discussion_search(review,id,body) VALUES(?1,?2,?3)",
                params![key, thread.id, body],
            )?;
        }
    }
    Ok(())
}

pub(super) fn search(db: &Connection, query: &str) -> anyhow::Result<Vec<(String, CommentThread)>> {
    // Treat the user's input as a phrase, never as FTS syntax.
    let phrase = format!("\"{}\"", query.replace('"', "\"\""));
    let mut stmt = db.prepare("SELECT t.review,t.data FROM discussion_search s JOIN threads t ON t.review=s.review AND t.id=s.id WHERE discussion_search MATCH ?1 ORDER BY rank LIMIT 100")?;
    let mut out = Vec::new();
    for row in stmt.query_map([phrase], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))? {
        let (key, data) = row?;
        out.push((key, serde_json::from_str(&data)?));
    }
    Ok(out)
}

pub(super) fn threads(
    db: &Connection,
    key: &str,
    path: Option<&str>,
    resolved: Option<bool>,
    id: Option<&str>,
) -> anyhow::Result<Vec<CommentThread>> {
    let mut sql = String::from("SELECT data FROM threads WHERE review=?");
    let mut values = vec![rusqlite::types::Value::Text(key.to_owned())];
    if let Some(path) = path {
        sql.push_str(" AND path=?");
        values.push(path.to_owned().into());
    }
    if let Some(resolved) = resolved {
        sql.push_str(" AND resolved=?");
        values.push((resolved as i64).into());
    }
    if let Some(id) = id {
        sql.push_str(" AND id=?");
        values.push(id.to_owned().into());
    }
    let mut stmt = db.prepare(&sql)?;
    let mut out = Vec::new();
    for row in stmt.query_map(rusqlite::params_from_iter(values), |r| r.get::<_, String>(0))? {
        out.push(serde_json::from_str(&row?)?);
    }
    Ok(out)
}
