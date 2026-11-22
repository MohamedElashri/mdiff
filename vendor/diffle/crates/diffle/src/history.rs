//! Read-only repository history. A pinned streaming walk owns an indexed, bounded disk spool;
//! neither paging nor retries rerun `log --skip`. No Session, watcher, credentials or LSP.
use crate::git::exec::command;
use anyhow::{Context as _, bail};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    process::Stdio,
    sync::{Arc, OnceLock},
    time::Duration,
};
use tokio::{
    io::{AsyncBufReadExt, AsyncReadExt, AsyncSeekExt, AsyncWriteExt, BufReader},
    sync::{Mutex, mpsc, oneshot, watch},
};

pub const PAGE_SIZE: usize = 100;
const SCAN_PAGE: usize = 5_000;
const MAX_SCAN: usize = 200_000;
const MAX_SPOOL: u64 = 32 * 1024 * 1024;
const MAX_LANES: usize = 32;
const IDLE: Duration = Duration::from_secs(30);
#[derive(Clone, Serialize, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Ref {
    pub name: String,
    pub sha: String,
}
#[derive(Clone, Serialize, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Info {
    pub root: String,
    pub git_dir: String,
    pub common_dir: String,
    pub head: Option<String>,
    pub branch: Option<String>,
    pub refs: Vec<Ref>,
    pub refs_truncated: bool,
    pub shallow: bool,
    pub partial: bool,
    pub stamp: String,
}
#[derive(Clone, Serialize, Deserialize, Debug)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Query {
    pub root: String,
    #[serde(default)]
    pub generation: String,
    #[serde(default)]
    pub search: String,
    #[serde(default)]
    pub author: String,
    pub since: Option<i64>,
    pub until: Option<i64>,
}
impl Query {
    fn filtered(&self) -> bool {
        !self.search.is_empty() || !self.author.is_empty() || self.since.is_some() || self.until.is_some()
    }
}
#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct Edge {
    pub from: usize,
    pub to: usize,
    pub color: usize,
}
#[derive(Clone, Serialize, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Row {
    pub sha: String,
    pub parents: Vec<String>,
    pub author: String,
    pub date: i64,
    pub subject: String,
    pub refs: Vec<String>,
    pub lane: Option<usize>,
    pub incoming: bool,
    pub edges: Vec<Edge>,
    pub boundary: bool,
    pub index: usize,
}
#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct Page {
    pub generation: String,
    pub page: usize,
    pub rows: Vec<Row>,
    pub next: Option<usize>,
    pub scanned: usize,
    pub limited: bool,
    pub notice: Option<String>,
}
#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct Detail {
    pub sha: String,
    pub tree: String,
    pub parents: Vec<String>,
    pub author: String,
    pub date: i64,
    pub message: String,
}
#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct TreeEntry {
    pub name: String,
    pub oid: String,
    pub kind: String,
}
#[derive(Serialize, Deserialize, Debug)]
pub struct Tree {
    pub entries: Vec<TreeEntry>,
    pub truncated: bool,
}
struct Handle {
    id: String,
    send: mpsc::Sender<(usize, oneshot::Sender<anyhow::Result<Page>>)>,
    stop: watch::Sender<bool>,
}
impl Drop for Handle {
    fn drop(&mut self) {
        self.stop.send_replace(true);
    }
}
pub struct Repository {
    pub root: PathBuf,
    git_dir: PathBuf,
    common_dir: PathBuf,
    query: Mutex<Option<Handle>>,
    admission: OnceLock<Arc<tokio::sync::Semaphore>>,
}

/// Every history read disables lazy fetch and replacement refs. Unknown local objects fail locally.
async fn read(root: &Path, args: &[&str], limit: usize) -> anyhow::Result<Vec<u8>> {
    let _permit = crate::scheduler::acquire().await;
    let mut child = command(root, args)
        .env("GIT_NO_LAZY_FETCH", "1")
        .env("GIT_NO_REPLACE_OBJECTS", "1")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()?;
    let job = async {
        let mut bytes = Vec::new();
        child.stdout.take().unwrap().take(limit as u64 + 1).read_to_end(&mut bytes).await?;
        if bytes.len() > limit {
            bail!("Repository metadata exceeds the read limit; narrow the selection");
        }
        if !child.wait().await?.success() {
            bail!("Git could not read local objects. Fetch missing objects with your Git client, then refresh");
        }
        Ok(bytes)
    };
    tokio::time::timeout(Duration::from_secs(10), job).await.context("Repository read timed out")?
}
fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).trim().to_owned()
}
fn oid(value: &str) -> anyhow::Result<()> {
    if !matches!(value.len(), 40 | 64) || !value.bytes().all(|c| c.is_ascii_hexdigit()) {
        bail!("Expected a full object ID");
    }
    Ok(())
}
impl Repository {
    pub fn at(root: PathBuf, git_dir: PathBuf, common_dir: PathBuf) -> Arc<Self> {
        Arc::new(Self { root, git_dir, common_dir, query: Mutex::new(None), admission: OnceLock::new() })
    }
    /// Hosts with several repository contexts share a retained-process budget. Set
    /// before serving queries; standalone repositories retain their existing policy.
    pub fn set_admission(&self, limit: Arc<tokio::sync::Semaphore>) {
        let _ = self.admission.set(limit);
    }
    pub async fn open(path: &Path) -> anyhow::Result<Arc<Self>> {
        let (root, git_dir, common_dir) = crate::git::repository_paths(path).await?;
        let (root, git_dir, common_dir) = (root.canonicalize()?, git_dir.canonicalize()?, common_dir.canonicalize()?);
        Ok(Self::at(root, git_dir, common_dir))
    }
    pub async fn close(&self) {
        self.query.lock().await.take();
    }
    pub async fn close_generation(&self, id: &str) {
        let mut query = self.query.lock().await;
        if query.as_ref().is_some_and(|q| q.id == id) {
            query.take();
        }
    }
    pub async fn info(&self) -> anyhow::Result<Info> {
        let refs = read(
            &self.root,
            &[
                "for-each-ref",
                "--count=1025",
                "--format=%(refname)%00%(objectname)%00%(*objectname)%00%(objecttype)%00%(*objecttype)",
                "refs/heads/",
                "refs/remotes/",
                "refs/tags/",
            ],
            2 * 1024 * 1024,
        )
        .await?;
        let mut list = Vec::new();
        for line in refs.split(|c| *c == b'\n').filter(|s| !s.is_empty()) {
            let parts: Vec<_> = line.split(|c| *c == 0).collect();
            if parts.len() == 5 && (parts[3] == b"commit" || parts[4] == b"commit") {
                let peeled = text(parts[2]);
                list.push(Ref { name: text(parts[0]), sha: if peeled.is_empty() { text(parts[1]) } else { peeled } });
            }
        }
        let refs_truncated = refs.split(|byte| *byte == b'\n').filter(|line| !line.is_empty()).count() > 1024;
        list.truncate(1024);
        let head = read(&self.root, &["rev-parse", "--verify", "HEAD^{commit}"], 128).await.ok().map(|v| text(&v));
        let branch = read(&self.root, &["symbolic-ref", "--quiet", "HEAD"], 1024).await.ok().map(|v| text(&v));
        let shallow = self.git_dir.join("shallow").exists() || self.common_dir.join("shallow").exists();
        let partial = read(&self.root, &["config", "--get", "extensions.partialClone"], 1024).await.is_ok()
            || read(&self.root, &["config", "--get-regexp", "remote\\..*\\.promisor"], 4096).await.is_ok();
        let stamp = format!("{:?}{:?}", head, list.iter().map(|r| (&r.name, &r.sha)).collect::<Vec<_>>());
        Ok(Info {
            root: self.root.display().to_string(),
            git_dir: self.git_dir.display().to_string(),
            common_dir: self.common_dir.display().to_string(),
            head,
            branch,
            refs: list,
            refs_truncated,
            shallow,
            partial,
            stamp,
        })
    }
    pub async fn begin(self: &Arc<Self>, query: Query) -> anyhow::Result<Page> {
        if query.search.len() > 128 || query.author.len() > 128 {
            bail!("Search is limited to 128 bytes");
        }
        let info = self.info().await?;
        let roots: Vec<String> = if query.root == "all" {
            info.refs.iter().map(|r| r.sha.clone()).chain(info.head.clone()).collect()
        } else if query.root == "HEAD" {
            info.head.clone().into_iter().collect()
        } else {
            vec![
                info.refs
                    .iter()
                    .find(|r| r.name == query.root)
                    .context("Ref moved or disappeared; refresh refs")?
                    .sha
                    .clone(),
            ]
        };
        let id = if query.generation.is_empty() {
            uuid::Uuid::new_v4().to_string()
        } else {
            uuid::Uuid::parse_str(&query.generation).context("Invalid history generation")?.to_string()
        };
        let (send, receive) = mpsc::channel(4);
        let (stop, stopped) = watch::channel(false);
        let handle = Handle { id: id.clone(), send, stop };
        let (reply, answer) = oneshot::channel();
        handle.send.send((0, reply)).await?;
        self.query.lock().await.replace(handle);
        let root = self.root.clone();
        let admission = self.admission.get().cloned();
        tokio::spawn(async move {
            // Acquire before the shared Git scheduler so retained walks cannot
            // occupy every interactive slot. A new generation stops its predecessor.
            let slot = if let Some(limit) = admission {
                match tokio::time::timeout(Duration::from_secs(1), limit.acquire_owned()).await {
                    Ok(Ok(slot)) => Some(slot),
                    _ => {
                        let mut receive = receive;
                        if let Some((_, reply)) = receive.recv().await {
                            let _ = reply.send(Err(anyhow::anyhow!(
                                "History process budget reached; close another history view and reload"
                            )));
                        }
                        return;
                    }
                }
            } else {
                None
            };
            walk(root, info, query, roots, id, receive, stopped, slot).await;
        });
        answer.await.context("History query expired; reload history")?
    }
    pub async fn page(&self, generation: &str, page: usize) -> anyhow::Result<Page> {
        let send = {
            let active = self.query.lock().await;
            let h = active
                .as_ref()
                .filter(|h| h.id == generation)
                .context("History query changed or expired; reload history")?;
            h.send.clone()
        };
        let (reply, answer) = oneshot::channel();
        send.send((page, reply)).await.context("History query expired; reload history")?;
        answer.await.context("History query expired; reload history")?
    }
    pub async fn detail(&self, sha: &str) -> anyhow::Result<Detail> {
        oid(sha)?;
        // Read actual object headers: rev-walk parent lists are truncated at shallow boundaries.
        let bytes = read(&self.root, &["cat-file", "commit", sha], 256 * 1024).await?;
        let value = String::from_utf8_lossy(&bytes);
        let (headers, message) = value.split_once("\n\n").context("Invalid commit object")?;
        let mut tree = String::new();
        let mut parents = Vec::new();
        let mut author = String::new();
        let mut date = 0;
        for line in headers.lines() {
            if let Some(v) = line.strip_prefix("tree ") {
                tree = v.into();
            }
            if let Some(v) = line.strip_prefix("parent ") {
                parents.push(v.into());
            }
            if let Some(v) = line.strip_prefix("author ") {
                let (name, time) = v.rsplit_once('>').context("Invalid author")?;
                author = name.rsplit_once(" <").map_or(name, |(name, _)| name).to_owned();
                date = time.split_whitespace().next().context("Invalid author date")?.parse()?;
            }
        }
        Ok(Detail { sha: sha.to_owned(), tree, parents, author, date, message: message.trim_end().to_owned() })
    }
    pub async fn tree(&self, sha: &str) -> anyhow::Result<Tree> {
        oid(sha)?;
        let bytes = read(&self.root, &["ls-tree", "-z", sha], 2 * 1024 * 1024).await?;
        let mut entries = Vec::new();
        for record in bytes.split(|b| *b == 0).filter(|v| !v.is_empty()).take(513) {
            if let Some(at) = record.iter().position(|b| *b == b'\t') {
                let head = text(&record[..at]);
                let bits: Vec<_> = head.split_whitespace().collect();
                if bits.len() == 3 {
                    entries.push(TreeEntry {
                        name: String::from_utf8_lossy(&record[at + 1..]).into_owned(),
                        oid: bits[2].into(),
                        kind: bits[1].into(),
                    });
                }
            }
        }
        let truncated = entries.len() > 512;
        entries.truncate(512);
        Ok(Tree { entries, truncated })
    }
}

#[derive(Default)]
struct Lanes {
    pending: Vec<(String, usize)>,
    color: usize,
}
impl Lanes {
    fn layout(&mut self, row: &mut Row) {
        let before = self.pending.clone();
        let at = before.iter().position(|(sha, _)| sha == &row.sha);
        let color = at.map(|i| before[i].1).unwrap_or_else(|| {
            self.color += 1;
            self.color % 6
        });
        let lane = at.or_else(|| (before.len() < MAX_LANES).then_some(before.len()));
        row.lane = lane;
        row.incoming = at.is_some();
        if let Some(at) = at {
            self.pending.remove(at);
        }
        for (i, parent) in row.parents.iter().enumerate() {
            if !self.pending.iter().any(|(sha, _)| sha == parent) {
                if self.pending.len() < MAX_LANES {
                    let pos = lane.unwrap_or(self.pending.len()).min(self.pending.len());
                    self.pending.insert(
                        pos,
                        (
                            parent.clone(),
                            if i == 0 {
                                color
                            } else {
                                self.color += 1;
                                self.color % 6
                            },
                        ),
                    );
                } else {
                    row.boundary = true;
                }
            }
        }
        for (from, (sha, color)) in before.iter().enumerate() {
            if sha != &row.sha
                && let Some(to) = self.pending.iter().position(|(s, _)| s == sha)
            {
                row.edges.push(Edge { from, to, color: *color });
            }
        }
        if let Some(from) = lane {
            for parent in &row.parents {
                if let Some(to) = self.pending.iter().position(|(s, _)| s == parent) {
                    row.edges.push(Edge { from, to, color });
                }
            }
        }
        row.boundary |= lane.is_none();
    }
}
async fn field(reader: &mut BufReader<tokio::process::ChildStdout>) -> anyhow::Result<Option<String>> {
    let mut bytes = Vec::new();
    (&mut *reader).take(64 * 1024).read_until(0, &mut bytes).await?;
    if bytes.is_empty() {
        return Ok(None);
    }
    if bytes.last() != Some(&0) {
        bail!("Commit metadata exceeds the 64 KiB field limit")
    }
    bytes.pop();
    Ok(Some(String::from_utf8_lossy(&bytes).trim_end().to_owned()))
}
#[allow(clippy::too_many_arguments)]
async fn walk(
    root: PathBuf,
    info: Info,
    query: Query,
    roots: Vec<String>,
    id: String,
    mut requests: mpsc::Receiver<(usize, oneshot::Sender<anyhow::Result<Page>>)>,
    stopped: watch::Receiver<bool>,
    slot: Option<tokio::sync::OwnedSemaphorePermit>,
) {
    let result = walk_inner(root, info, query, roots, id, &mut requests, stopped, slot).await;
    if let Err(error) = result
        && let Some((_, reply)) = requests.recv().await
    {
        let _ = reply.send(Err(error));
    }
}

#[allow(clippy::too_many_arguments)]
async fn walk_inner(
    root: PathBuf,
    info: Info,
    query: Query,
    roots: Vec<String>,
    id: String,
    requests: &mut mpsc::Receiver<(usize, oneshot::Sender<anyhow::Result<Page>>)>,
    mut stopped: watch::Receiver<bool>,
    mut slot: Option<tokio::sync::OwnedSemaphorePermit>,
) -> anyhow::Result<()> {
    let permit = tokio::select! {_=stopped.changed()=>return Ok(()),permit=crate::scheduler::acquire()=>permit};
    let mut permit = Some(permit);
    let mut spool = tokio::fs::File::from_std(tempfile::tempfile()?);
    let mut index: Vec<(u64, usize)> = Vec::new();
    let mut spool_size = 0u64;
    let mut args = vec![
        "log",
        "-z",
        "--topo-order",
        "--no-decorate",
        "--format=%H%x00%P%x00%at%x00%<(96,trunc)%an%x00%<(256,trunc)%s",
    ];
    args.extend(roots.iter().map(String::as_str));
    args.push("--");
    let mut child = if roots.is_empty() {
        None
    } else {
        Some(
            command(&root, &args)
                .env("GIT_NO_LAZY_FETCH", "1")
                .env("GIT_NO_REPLACE_OBJECTS", "1")
                .stdin(Stdio::null())
                .stdout(Stdio::piped())
                .stderr(Stdio::null())
                .spawn()?,
        )
    };
    let mut reader = child.as_mut().map(|c| BufReader::new(c.stdout.take().unwrap()));
    let mut done = roots.is_empty();
    let mut scanned = 0usize;
    let mut lanes = Lanes::default();
    let mut labels: HashMap<String, Vec<String>> = info.refs.iter().fold(HashMap::new(), |mut m, r| {
        m.entry(r.sha.clone()).or_default().push(r.name.clone());
        m
    });
    if let Some(head) = &info.head {
        labels.entry(head.clone()).or_default().push("HEAD".into());
    }
    for names in labels.values_mut() {
        names.truncate(8);
    }
    loop {
        let next = tokio::select! {biased;_ = stopped.changed()=>break,_ = tokio::time::sleep(IDLE)=>break,item=requests.recv()=>item};
        let Some((page, mut reply)) = next else { break };
        if (done && !index.is_empty() && page == index.len()) || page > index.len() {
            let _ = reply.send(Err(anyhow::anyhow!("Invalid history cursor")));
            continue;
        }
        if page < index.len() {
            let (offset, len) = index[page];
            spool.seek(std::io::SeekFrom::Start(offset)).await?;
            let mut bytes = vec![0; len];
            spool.read_exact(&mut bytes).await?;
            let _ = reply.send(serde_json::from_slice(&bytes).map_err(Into::into));
            continue;
        }
        let job = async {
            let mut rows = Vec::new();
            let start = scanned;
            let mut limited = false;
            let mut notice = if info.refs_truncated {
                Some("Only the first 1024 refs are included. Choose a narrower root.".to_owned())
            } else {
                None
            };
            while !done && rows.len() < PAGE_SIZE && scanned - start < SCAN_PAGE {
                if scanned >= MAX_SCAN || spool_size >= MAX_SPOOL - 1024 * 1024 {
                    done = true;
                    limited = true;
                    notice = Some("History limit reached. Narrow the root or filter and reload.".into());
                    break;
                }
                let r = reader.as_mut().unwrap();
                let Some(sha) = field(r).await? else {
                    done = true;
                    if !child.as_mut().unwrap().wait().await?.success() {
                        limited = true;
                        notice=Some("History is incomplete: local objects may be missing. Fetch with your Git client and refresh.".into());
                    }
                    break;
                };
                let parents = field(r).await?.context("Missing parents")?;
                let date = field(r).await?.context("Missing date")?.parse()?;
                let author = field(r).await?.context("Missing author")?;
                let subject = field(r).await?.context("Missing subject")?;
                let mut row = Row {
                    refs: labels.get(&sha).cloned().unwrap_or_default(),
                    boundary: info.shallow,
                    sha,
                    parents: parents.split_whitespace().map(str::to_owned).collect(),
                    date,
                    author,
                    subject,
                    lane: None,
                    incoming: false,
                    edges: Vec::new(),
                    index: scanned,
                };
                scanned += 1;
                if row.parents.len() > 64 {
                    row.parents.truncate(64);
                    row.boundary = true;
                }
                lanes.layout(&mut row);
                let search = query.search.to_lowercase();
                let matches =
                    (search.is_empty() || row.subject.to_lowercase().contains(&search) || row.sha.starts_with(&search))
                        && row.author.to_lowercase().contains(&query.author.to_lowercase())
                        && query.since.is_none_or(|d| row.date >= d)
                        && query.until.is_none_or(|d| row.date <= d);
                if query.filtered() {
                    row.edges.clear();
                    row.lane = Some(0);
                    row.boundary = !row.parents.is_empty();
                }
                if matches {
                    rows.push(row);
                }
            }
            Ok::<Page, anyhow::Error>(Page {
                generation: id.clone(),
                page,
                rows,
                next: (!done).then_some(page + 1),
                scanned,
                limited,
                notice,
            })
        };
        let value = tokio::select! {biased;_=stopped.changed()=>break,_=reply.closed()=>break,result=tokio::time::timeout(Duration::from_secs(10),job)=>result.unwrap_or_else(|_|Err(anyhow::anyhow!("History traversal timed out; narrow the root")))};
        match value {
            Ok(mut page) => {
                if spool_size + serde_json::to_vec(&page)?.len() as u64 > MAX_SPOOL {
                    page.rows.clear();
                    page.next = None;
                    page.limited = true;
                    page.notice = Some("History spool limit reached. Narrow the selection and reload.".into());
                    done = true;
                }
                if page.next.is_none() {
                    reader.take();
                    child.take();
                    permit.take();
                    slot.take();
                }
                let bytes = serde_json::to_vec(&page)?;
                spool.seek(std::io::SeekFrom::Start(spool_size)).await?;
                spool.write_all(&bytes).await?;
                index.push((spool_size, bytes.len()));
                spool_size += bytes.len() as u64;
                let _ = reply.send(Ok(page));
            }
            Err(error) => {
                let _ = reply.send(Err(error));
                break;
            }
        }
    }
    Ok::<(), anyhow::Error>(())
}
