//! Revisioned workspace metadata. Loading it never opens a repository or starts a runtime.
use crate::{
    engine::{Engine, OpenOptions, OpenedReview, SavedReview},
    repository::Repositories,
};
use anyhow::{bail, Context, Result};
use diffle::{
    git::GitRepo,
    persist::{with_file_lock, write_file_atomic},
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::{HashMap, HashSet},
    path::PathBuf,
    sync::atomic::{AtomicBool, AtomicU64, Ordering},
};
use tauri::{Emitter, Manager, State, WebviewWindow};
use tokio::sync::Mutex;
const MAX_META: u64 = 4 * 1024 * 1024;
const MAX_VIEW: u64 = 2 * 1024 * 1024;
const MAX_TABS: usize = 128;
pub(crate) fn id() -> String {
    static NEXT: AtomicU64 = AtomicU64::new(1);
    crate::storage::digest(&format!(
        "{}:{:?}:{}",
        std::process::id(),
        std::time::SystemTime::now(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ))
}
fn valid_id(id: &str) -> bool {
    id.len() == 64 && id.bytes().all(|b| b.is_ascii_hexdigit())
}
fn name(value: &str) -> Result<String> {
    let value = value.trim();
    anyhow::ensure!(
        !value.is_empty() && value.chars().count() <= 100 && !value.contains(['\0', '\n', '\r']),
        "Use a name of 1–100 characters"
    );
    Ok(value.into())
}
#[derive(Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Source {
    pub path: String,
    pub request: Value,
    pub label: String,
}
impl Source {
    fn clean(mut self) -> Result<Self> {
        anyhow::ensure!(
            self.path.len() <= 4096 && !self.path.contains("://") && !self.path.contains('\0'),
            "Invalid repository path"
        );
        if let Some(url) = self
            .request
            .get("url")
            .and_then(Value::as_str)
            .filter(|_| self.request["kind"] == "gitlab")
        {
            let (origin, project, iid) = crate::gitlab::parse(url)?;
            let url = format!("{origin}{project}/-/merge_requests/{iid}");
            self.request = json!({"kind":"gitlab","url":url});
            self.label = url;
        } else {
            let request: diffle::protocol::EnterRequest = serde_json::from_value(self.request)?;
            self.request = serde_json::to_value(request)?;
            if let Some(input) = self
                .request
                .get("pr")
                .and_then(Value::as_str)
                .map(str::to_owned)
            {
                if input.contains("://") {
                    let mut url = url::Url::parse(&input)?;
                    anyhow::ensure!(
                        url.scheme() == "https"
                            && url.username().is_empty()
                            && url.password().is_none(),
                        "Use an HTTPS review URL without embedded credentials"
                    );
                    url.set_query(None);
                    url.set_fragment(None);
                    self.request["pr"] = url.to_string().into();
                    self.label = url.to_string();
                }
            }
        }
        anyhow::ensure!(
            serde_json::to_vec(&self.request)?.len() <= 8192,
            "Comparison is too large"
        );
        if let Some(args) = self.request.get("args").and_then(Value::as_array) {
            anyhow::ensure!(
                args.len() <= 16
                    && args
                        .iter()
                        .all(|v| v.as_str().is_some_and(|s| s.len() <= 1024
                            && !s.contains(['\0', '\n', '\r'])
                            && !s.contains("://"))),
                "Invalid revision arguments"
            );
        }
        self.label = self.label.chars().take(160).collect();
        if self.label.is_empty() {
            self.label = "Review".into();
        }
        Ok(self)
    }
    fn remote(&self) -> bool {
        self.request["kind"] == "pr" || self.request["kind"] == "gitlab"
    }
}
#[derive(Clone, Copy, Serialize, Deserialize, PartialEq, Debug)]
#[serde(rename_all = "camelCase")]
pub enum Policy {
    Follow,
    Pinned,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Tab {
    pub id: String,
    pub source: Source,
    pub group: Option<String>,
    pub policy: Policy,
    pub saved: Option<SavedReview>,
    #[serde(default)]
    pub drafts: usize,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Group {
    pub id: String,
    pub name: String,
    pub color: String,
    pub collapsed: bool,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Workspace {
    pub id: String,
    pub name: String,
    pub tabs: Vec<Tab>,
    pub groups: Vec<Group>,
    pub active: Option<String>,
    pub recent: Vec<Source>,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Closed {
    pub workspace: String,
    pub tab: Tab,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Document {
    pub version: u8,
    pub revision: u64,
    pub active_workspace: String,
    pub workspaces: Vec<Workspace>,
    pub closed: Vec<Closed>,
}
impl Default for Document {
    fn default() -> Self {
        let workspace = Workspace {
            id: id(),
            name: "Default".into(),
            tabs: vec![],
            groups: vec![],
            active: None,
            recent: vec![],
        };
        Self {
            version: 1,
            revision: 0,
            active_workspace: workspace.id.clone(),
            workspaces: vec![workspace],
            closed: vec![],
        }
    }
}
impl Document {
    fn workspace_mut(&mut self, id: &str) -> Result<&mut Workspace> {
        self.workspaces
            .iter_mut()
            .find(|w| w.id == id)
            .context("Workspace no longer exists")
    }
    fn tab(&self, id: &str) -> Result<&Tab> {
        self.workspaces
            .iter()
            .flat_map(|w| &w.tabs)
            .find(|t| t.id == id)
            .context("Tab no longer exists")
    }
    fn tab_mut(&mut self, id: &str) -> Result<&mut Tab> {
        self.workspaces
            .iter_mut()
            .flat_map(|w| &mut w.tabs)
            .find(|t| t.id == id)
            .context("Tab no longer exists")
    }
    fn validate(&self) -> Result<()> {
        anyhow::ensure!(
            self.version == 1,
            "Unsupported workspace version; original file was preserved"
        );
        anyhow::ensure!(
            !self.workspaces.is_empty() && self.workspaces.len() <= 32 && self.closed.len() <= 148,
            "Workspace limit exceeded"
        );
        anyhow::ensure!(
            self.workspaces
                .iter()
                .any(|w| w.id == self.active_workspace),
            "Invalid active workspace"
        );
        let mut ids = HashSet::new();
        let mut count = 0;
        for w in &self.workspaces {
            anyhow::ensure!(
                valid_id(&w.id) && ids.insert(w.id.clone()),
                "Invalid workspace identity"
            );
            name(&w.name)?;
            anyhow::ensure!(
                w.groups.len() <= 16 && w.recent.len() <= 12,
                "Group or recent limit exceeded"
            );
            for g in &w.groups {
                anyhow::ensure!(
                    valid_id(&g.id)
                        && ids.insert(g.id.clone())
                        && colors().contains(&g.color.as_str()),
                    "Invalid group"
                );
                name(&g.name)?;
            }
            for t in &w.tabs {
                count += 1;
                check_tab(t, &mut ids)?;
                anyhow::ensure!(
                    t.group
                        .as_ref()
                        .is_none_or(|id| w.groups.iter().any(|g| g.id == *id)),
                    "Tab group no longer exists"
                );
            }
            anyhow::ensure!(
                w.active
                    .as_ref()
                    .is_none_or(|id| w.tabs.iter().any(|t| t.id == *id)),
                "Invalid active tab"
            );
        }
        for c in &self.closed {
            check_tab(&c.tab, &mut ids)?;
            anyhow::ensure!(
                self.workspaces.iter().any(|w| w.id == c.workspace),
                "Invalid closed tab workspace"
            );
        }
        anyhow::ensure!(count <= MAX_TABS, "At most 128 open tabs are supported");
        Ok(())
    }
}
fn check_tab(t: &Tab, ids: &mut HashSet<String>) -> Result<()> {
    anyhow::ensure!(
        valid_id(&t.id) && ids.insert(t.id.clone()),
        "Invalid tab identity"
    );
    t.source.clone().clean()?;
    if let Some(saved) = &t.saved {
        anyhow::ensure!(
            saved.key.starts_with(&format!("workspace/{}/", t.id)),
            "Invalid saved review identity"
        );
    }
    Ok(())
}
async fn retire_pins(previous: Option<&SavedReview>, keep: Option<&SavedReview>, tab: &str) {
    let Some(previous) = previous else { return };
    if keep.is_some_and(|s| s.pins == previous.pins) {
        return;
    }
    let prefix = format!("refs/diffle/iterations/workspaces/{tab}/");
    let pins: Vec<_> = previous
        .pins
        .iter()
        .filter(|p| p.starts_with(&prefix))
        .map(|p| (p.clone(), None))
        .collect();
    let at = previous.archive.as_ref().unwrap_or(&previous.root);
    if let Ok(repo) = diffle::git::GitRepo::open(if previous.archive.is_some() {
        &previous.root
    } else {
        at
    })
    .await
    {
        let _ = repo.pin(&pins).await;
    } else {
        for (pin, _) in pins {
            let _ =
                diffle::git::exec::exec_git(at, &["update-ref", "-d", &pin], Default::default())
                    .await;
        }
    }
}
fn colors() -> [&'static str; 6] {
    ["blue", "green", "amber", "purple", "rose", "gray"]
}
#[derive(Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum Operation {
    CreateWorkspace {
        name: String,
    },
    SelectWorkspace {
        id: String,
    },
    RenameWorkspace {
        id: String,
        name: String,
    },
    DeleteWorkspace {
        id: String,
    },
    AddTab {
        workspace: String,
        source: Source,
    },
    CloseTab {
        id: String,
    },
    ReopenTab,
    MoveWorkspace {
        id: String,
        workspace: String,
    },
    MoveTab {
        id: String,
        before: Option<String>,
    },
    RenameTab {
        id: String,
        label: String,
    },
    SetGroup {
        id: String,
        group: Option<String>,
    },
    SetPolicy {
        id: String,
        policy: Policy,
    },
    CreateGroup {
        workspace: String,
        name: String,
        color: String,
    },
    EditGroup {
        id: String,
        name: Option<String>,
        color: Option<String>,
        collapsed: Option<bool>,
    },
    DeleteGroup {
        id: String,
    },
}
fn apply(doc: &mut Document, op: Operation) -> Result<()> {
    match op {
        Operation::CreateWorkspace { name: n } => {
            let w = Workspace {
                id: id(),
                name: name(&n)?,
                tabs: vec![],
                groups: vec![],
                active: None,
                recent: vec![],
            };
            doc.active_workspace = w.id.clone();
            doc.workspaces.push(w);
        }
        Operation::SelectWorkspace { id } => {
            doc.workspace_mut(&id)?;
            doc.active_workspace = id;
        }
        Operation::RenameWorkspace { id, name: n } => doc.workspace_mut(&id)?.name = name(&n)?,
        Operation::DeleteWorkspace { id } => {
            anyhow::ensure!(
                !doc.workspaces
                    .iter()
                    .filter(|w| w.id == id)
                    .flat_map(|w| &w.tabs)
                    .any(|t| t.drafts > 0)
                    && !doc
                        .closed
                        .iter()
                        .any(|c| c.workspace == id && c.tab.drafts > 0),
                "This workspace has unsent drafts. Move or finish those tabs before deleting it"
            );
            anyhow::ensure!(doc.workspaces.len() > 1, "Keep at least one workspace");
            doc.workspace_mut(&id)?;
            doc.workspaces.retain(|w| w.id != id);
            doc.closed.retain(|c| c.workspace != id);
            if doc.active_workspace == id {
                doc.active_workspace = doc.workspaces[0].id.clone();
            }
        }
        Operation::AddTab { workspace, source } => {
            let source = source.clean()?;
            let w = doc.workspace_mut(&workspace)?;
            let existing = w
                .tabs
                .iter()
                .find(|t| {
                    (t.source.path == source.path
                        || t.saved
                            .as_ref()
                            .is_some_and(|s| s.root == std::path::Path::new(&source.path)))
                        && t.source.request == source.request
                })
                .map(|t| t.id.clone());
            if let Some(id) = existing {
                w.active = Some(id);
            } else {
                let policy = if source.remote() || source.request["kind"] == "comparison" {
                    Policy::Pinned
                } else {
                    Policy::Follow
                };
                let t = Tab {
                    id: id(),
                    source,
                    policy,
                    group: None,
                    saved: None,
                    drafts: 0,
                };
                w.active = Some(t.id.clone());
                w.tabs.push(t);
            }
        }
        Operation::CloseTab { id } => {
            let w = doc
                .workspaces
                .iter_mut()
                .find(|w| w.tabs.iter().any(|t| t.id == id))
                .context("Tab no longer exists")?;
            let index = w.tabs.iter().position(|t| t.id == id).unwrap();
            let tab = w.tabs.remove(index);
            if w.active.as_ref() == Some(&id) {
                w.active = w
                    .tabs
                    .get(index.min(w.tabs.len().saturating_sub(1)))
                    .map(|t| t.id.clone());
            }
            doc.closed.insert(
                0,
                Closed {
                    workspace: w.id.clone(),
                    tab,
                },
            );
            let mut empty = 0;
            doc.closed.retain(|c| {
                if c.tab.drafts > 0 {
                    true
                } else {
                    empty += 1;
                    empty <= 20
                }
            });
        }
        Operation::ReopenTab => {
            if let Some(mut closed) = doc.closed.first().cloned() {
                doc.closed.remove(0);
                let w = doc.workspace_mut(&closed.workspace)?;
                if closed
                    .tab
                    .group
                    .as_ref()
                    .is_some_and(|id| !w.groups.iter().any(|g| g.id == *id))
                {
                    closed.tab.group = None;
                }
                w.active = Some(closed.tab.id.clone());
                w.tabs.push(closed.tab);
                doc.active_workspace = w.id.clone();
            } else {
                bail!("No recently closed tabs");
            }
        }
        Operation::MoveWorkspace { id, workspace } => {
            doc.workspace_mut(&workspace)?;
            let old = doc
                .workspaces
                .iter_mut()
                .find(|w| w.tabs.iter().any(|t| t.id == id))
                .context("Tab no longer exists")?;
            let index = old.tabs.iter().position(|t| t.id == id).unwrap();
            let mut tab = old.tabs.remove(index);
            tab.group = None;
            if old.active.as_ref() == Some(&id) {
                old.active = old.tabs.first().map(|t| t.id.clone());
            }
            let target = doc.workspace_mut(&workspace)?;
            target.active = Some(id);
            target.tabs.push(tab);
            doc.active_workspace = workspace;
        }
        Operation::MoveTab { id, before } => {
            let w = doc
                .workspaces
                .iter_mut()
                .find(|w| w.tabs.iter().any(|t| t.id == id))
                .context("Tab no longer exists")?;
            if before.as_ref() == Some(&id) {
                return Ok(());
            }
            let index = w.tabs.iter().position(|t| t.id == id).unwrap();
            let mut tab = w.tabs.remove(index);
            let at = if let Some(before) = &before {
                w.tabs
                    .iter()
                    .position(|t| &t.id == before)
                    .context("Destination tab no longer exists")?
            } else {
                w.tabs.len()
            };
            if before.is_some() {
                tab.group = w.tabs.get(at).and_then(|t| t.group.clone());
            }
            w.tabs.insert(at, tab);
        }
        Operation::RenameTab { id, label } => doc.tab_mut(&id)?.source.label = name(&label)?,
        Operation::SetGroup { id, group } => doc.tab_mut(&id)?.group = group,
        Operation::SetPolicy { id, policy } => {
            let t = doc.tab_mut(&id)?;
            anyhow::ensure!(
                !(policy == Policy::Follow
                    && t.source.remote()
                    && !t.saved.as_ref().is_some_and(SavedReview::mutable)),
                "Remote tabs keep a pinned revision; use Refresh source to fetch updates"
            );
            anyhow::ensure!(
                !(policy == Policy::Pinned
                    && (t.saved.as_ref().is_some_and(|s| s.mutable())
                        || t.saved.is_none() && t.source.request["kind"] == "working")),
                "Working changes remain live; choose a commit comparison to pin revisions"
            );
            t.policy = policy;
        }
        Operation::CreateGroup {
            workspace,
            name: n,
            color,
        } => {
            anyhow::ensure!(colors().contains(&color.as_str()), "Invalid group color");
            doc.workspace_mut(&workspace)?.groups.push(Group {
                id: id(),
                name: name(&n)?,
                color,
                collapsed: false,
            });
        }
        Operation::EditGroup {
            id,
            name: n,
            color,
            collapsed,
        } => {
            let g = doc
                .workspaces
                .iter_mut()
                .flat_map(|w| &mut w.groups)
                .find(|g| g.id == id)
                .context("Group no longer exists")?;
            if let Some(n) = n {
                g.name = name(&n)?;
            }
            if let Some(color) = color {
                anyhow::ensure!(colors().contains(&color.as_str()), "Invalid group color");
                g.color = color;
            }
            if let Some(collapsed) = collapsed {
                g.collapsed = collapsed;
            }
        }
        Operation::DeleteGroup { id } => {
            for w in &mut doc.workspaces {
                w.groups.retain(|g| g.id != id);
                for t in &mut w.tabs {
                    if t.group.as_ref() == Some(&id) {
                        t.group = None;
                    }
                }
            }
        }
    }
    doc.validate()
}
struct Store {
    file: PathBuf,
}
impl Store {
    async fn read(&self) -> Result<Document> {
        let size = tokio::fs::metadata(&self.file).await?.len();
        anyhow::ensure!(
            size <= MAX_META,
            "Workspace file is too large; original was preserved"
        );
        let doc: Document = serde_json::from_slice(&tokio::fs::read(&self.file).await?)?;
        doc.validate()?;
        Ok(doc)
    }
    async fn load(&self, legacy: Vec<Source>) -> Result<Document> {
        with_file_lock(&self.file, || async {
            if self.file.exists() {
                return self.read().await;
            }
            let mut doc = Document::default();
            doc.workspaces[0].recent = legacy
                .into_iter()
                .filter_map(|s| s.clean().ok())
                .take(12)
                .collect();
            self.write(&doc).await?;
            Ok(doc)
        })
        .await
    }
    async fn write(&self, doc: &Document) -> Result<()> {
        doc.validate()?;
        let text = serde_json::to_string(doc)?;
        anyhow::ensure!(
            text.len() as u64 <= MAX_META,
            "Workspace metadata limit reached"
        );
        write_file_atomic(&self.file, &text).await
    }
    async fn edit(
        &self,
        revision: Option<u64>,
        f: impl FnOnce(&mut Document) -> Result<()>,
    ) -> Result<Document> {
        with_file_lock(&self.file, || async {
            let mut doc = self.read().await?;
            if let Some(revision) = revision {
                anyhow::ensure!(
                    revision == doc.revision,
                    "Workspace changed; reload and retry"
                );
            }
            f(&mut doc)?;
            doc.revision = doc
                .revision
                .checked_add(1)
                .context("Workspace revision exhausted")?;
            self.write(&doc).await?;
            Ok(doc)
        })
        .await
    }
    fn tab_lease(&self, tab: &str) -> Result<std::sync::Arc<std::fs::File>> {
        anyhow::ensure!(valid_id(tab), "Invalid tab identity");
        let directory = self.file.parent().unwrap().join("workspace-leases");
        std::fs::create_dir_all(&directory)?;
        let mut opts = std::fs::OpenOptions::new();
        opts.write(true).create(true).truncate(false);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            opts.mode(0o600);
        }
        let file = opts.open(directory.join(tab))?;
        file.try_lock().map_err(|_| {
            anyhow::anyhow!(
                "This tab is open in another app process. Unload it there before changing it here."
            )
        })?;
        Ok(std::sync::Arc::new(file))
    }
    fn view_file(&self, tab: &str) -> Result<PathBuf> {
        anyhow::ensure!(valid_id(tab), "Invalid tab identity");
        Ok(self
            .file
            .parent()
            .unwrap()
            .join("workspace-views")
            .join(format!("{tab}.json")))
    }
    async fn view(&self, tab: &str) -> Result<View> {
        let path = self.view_file(tab)?;
        if !path.exists() {
            return Ok(View::default());
        }
        anyhow::ensure!(
            tokio::fs::metadata(&path).await?.len() <= MAX_VIEW,
            "Saved tab state is too large"
        );
        let view: View = serde_json::from_slice(&tokio::fs::read(path).await?)?;
        view.validate()?;
        Ok(view)
    }
    async fn save_view(&self, tab: &str, revision: u64, mut view: View) -> Result<View> {
        let path = self.view_file(tab)?;
        with_file_lock(&path, || async {
            let old = self.view(tab).await?;
            anyhow::ensure!(
                old.revision == revision,
                "Tab state changed; reopen this tab before saving"
            );
            view.revision = revision.checked_add(1).context("Tab revision exhausted")?;
            view.validate()?;
            let text = serde_json::to_string(&view)?;
            anyhow::ensure!(
                text.len() as u64 <= MAX_VIEW,
                "Tab state limit reached; keep this tab open and copy its drafts"
            );
            write_file_atomic(&path, &text).await?;
            Ok(view)
        })
        .await
    }
}
#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct View {
    pub revision: u64,
    #[serde(default)]
    pub unsent: HashMap<String, String>,
    pub navigation: Option<Value>,
}
impl View {
    fn validate(&self) -> Result<()> {
        anyhow::ensure!(self.unsent.len() <= 1000, "Too many composers in one tab");
        if let Some(nav) = &self.navigation {
            anyhow::ensure!(
                nav.is_object() && serde_json::to_vec(nav)?.len() <= 128 * 1024,
                "Navigation state is too large"
            );
            anyhow::ensure!(
                nav.as_object().unwrap().keys().all(|k| [
                    "activePath",
                    "selection",
                    "collapsed",
                    "draft",
                    "paneScroll",
                    "jumps",
                    "jumpIndex",
                    "view",
                    "graph",
                    "viewport",
                    "snapshotKey",
                    "versions",
                    "fileView"
                ]
                .contains(&k.as_str())),
                "Invalid navigation state"
            );
        }
        Ok(())
    }
}
#[derive(Clone)]
struct Binding {
    owner: String,
    id: String,
    _lease: std::sync::Arc<std::fs::File>,
}
#[derive(Default)]
pub struct Sessions {
    pub(crate) quit_ready: AtomicBool,
    live: Mutex<HashMap<String, Binding>>,
    openers: std::sync::Mutex<HashMap<String, std::sync::Weak<Mutex<()>>>>,
    pub(crate) gate: std::sync::Arc<tokio::sync::RwLock<()>>,
    structure: Mutex<()>,
}
impl Sessions {
    pub(crate) async fn hosting_source(
        &self,
        app: &tauri::AppHandle,
        owner: &str,
        id: &str,
    ) -> Result<Source> {
        let tab = self
            .live
            .lock()
            .await
            .iter()
            .find(|(_, b)| b.owner == owner && b.id == id)
            .map(|(t, _)| t.clone())
            .ok_or_else(|| anyhow::anyhow!("Review is not bound to this window"))?;
        let doc = store(app)?.read().await?;
        doc.workspaces
            .iter()
            .flat_map(|w| &w.tabs)
            .find(|t| t.id == tab)
            .map(|t| t.source.clone())
            .ok_or_else(|| anyhow::anyhow!("Review tab is unavailable"))
    }
    pub(crate) async fn opening(&self, owner: &str) -> tokio::sync::OwnedMutexGuard<()> {
        let lock = {
            let mut locks = self.openers.lock().unwrap();
            locks.retain(|_, v| v.strong_count() > 0);
            let lock = locks
                .get(owner)
                .and_then(std::sync::Weak::upgrade)
                .unwrap_or_else(|| std::sync::Arc::new(Mutex::new(())));
            locks.insert(owner.into(), std::sync::Arc::downgrade(&lock));
            lock
        };
        lock.lock_owned().await
    }
    async fn lease(
        &self,
        store: &Store,
        tab: &str,
        owner: &str,
    ) -> Result<std::sync::Arc<std::fs::File>> {
        if let Some(binding) = self.live.lock().await.get(tab).cloned() {
            anyhow::ensure!(binding.owner == owner, "Tab belongs to another window");
            return Ok(binding._lease);
        }
        store.tab_lease(tab)
    }
    pub(crate) async fn close_owner(&self, owner: &str) {
        self.live.lock().await.retain(|_, b| b.owner != owner);
    }
}
fn authorize_operation(
    app: &tauri::AppHandle,
    owner: &str,
    doc: &Document,
    op: &Operation,
) -> Result<(), String> {
    let current = project(app, owner, doc.clone())?.active_workspace;
    let check = |workspace: &str| -> Result<(), String> {
        if let Some(other) = app
            .state::<crate::windows::Windows>()
            .owner(workspace)
            .filter(|w| w != owner)
        {
            if let Some(window) = app.get_webview_window(&other) {
                let _ = window.set_focus();
            }
            return Err("This workspace is open in another window".into());
        }
        Ok(())
    };
    match op {
        Operation::SelectWorkspace { id }
        | Operation::DeleteWorkspace { id }
        | Operation::RenameWorkspace { id, .. } => check(id),
        Operation::AddTab { workspace, .. } | Operation::CreateGroup { workspace, .. } => {
            if workspace != &current {
                return Err("Select this workspace before changing it".into());
            }
            check(workspace)
        }
        Operation::CloseTab { id }
        | Operation::RenameTab { id, .. }
        | Operation::SetGroup { id, .. }
        | Operation::SetPolicy { id, .. }
        | Operation::MoveTab { id, .. } => tab_owner(app, owner, doc, id),
        Operation::MoveWorkspace { id, workspace } => {
            tab_owner(app, owner, doc, id)?;
            check(workspace)
        }
        Operation::EditGroup { id, .. } | Operation::DeleteGroup { id } => {
            let w = doc
                .workspaces
                .iter()
                .find(|w| w.groups.iter().any(|g| g.id == *id))
                .ok_or("Group no longer exists")?;
            if w.id != current {
                return Err("Group belongs to another workspace".into());
            }
            Ok(())
        }
        _ => Ok(()),
    }
}
pub(crate) async fn worktree_associations(
    app: &tauri::AppHandle,
) -> Result<Vec<crate::worktree_tools::Association>> {
    let doc = store(app)?.read().await?;
    Ok(doc
        .workspaces
        .into_iter()
        .flat_map(|workspace| {
            workspace
                .tabs
                .into_iter()
                .map(move |tab| crate::worktree_tools::Association {
                    path: tab
                        .saved
                        .as_ref()
                        .map(|saved| saved.root.to_string_lossy().into_owned())
                        .unwrap_or_else(|| tab.source.path.clone()),
                    tab: tab.id,
                    label: tab.source.label,
                    workspace: workspace.id.clone(),
                    workspace_name: workspace.name.clone(),
                })
        })
        .collect())
}
fn store(app: &tauri::AppHandle) -> Result<Store> {
    Ok(Store {
        file: app.path().app_config_dir()?.join("workspaces.json"),
    })
}
/// Metadata only: inbox polling must not acquire a review runtime or initialize LSPs.
pub(crate) async fn attention_document(app: &tauri::AppHandle) -> Result<Option<Document>> {
    let store = store(app)?;
    if !store.file.exists() {
        return Ok(None);
    }
    store.read().await.map(Some)
}
fn owner(window: &WebviewWindow) -> Result<&str> {
    crate::windows::authorize(window).map_err(anyhow::Error::msg)?;
    Ok(window.label())
}
fn project(app: &tauri::AppHandle, owner: &str, mut doc: Document) -> Result<Document, String> {
    doc.active_workspace = app.state::<crate::windows::Windows>().current(
        owner,
        &doc.active_workspace,
        &doc.workspaces
            .iter()
            .map(|w| w.id.clone())
            .collect::<Vec<_>>(),
    )?;
    doc.closed.retain(|c| c.workspace == doc.active_workspace);
    if let Some(window) = app.get_webview_window(owner) {
        if let Some(workspace) = doc.workspaces.iter().find(|w| w.id == doc.active_workspace) {
            let _ = window.set_title(&format!("mdiff — {}", workspace.name));
        }
    }
    Ok(doc)
}
fn tab_owner(app: &tauri::AppHandle, owner: &str, doc: &Document, tab: &str) -> Result<(), String> {
    let workspace = doc
        .workspaces
        .iter()
        .find(|w| w.tabs.iter().any(|t| t.id == tab))
        .map(|w| w.id.clone())
        .or_else(|| {
            doc.closed
                .iter()
                .find(|c| c.tab.id == tab)
                .map(|c| c.workspace.clone())
        })
        .ok_or("Tab no longer exists")?;
    let current = project(app, owner, doc.clone())?.active_workspace;
    if current != workspace {
        return Err("Tab belongs to another workspace or window".into());
    }
    Ok(())
}

fn emit(app: &tauri::AppHandle, doc: &Document) {
    let _ = app.emit("workspace-state", doc.revision);
}
fn error(e: anyhow::Error) -> String {
    format!("{e:#}")
}
#[tauri::command]
pub async fn workspace_load(
    app: tauri::AppHandle,
    window: WebviewWindow,
    legacy: Option<Value>,
) -> Result<Document, String> {
    owner(&window).map_err(error)?;
    let sessions = app.state::<Sessions>();
    let _work = sessions.gate.read().await;
    app.state::<crate::windows::Windows>()
        .allow_save(window.label())?;
    let doc = store(&app)
        .map_err(error)?
        .load(
            legacy
                .and_then(|v| v.as_array().cloned())
                .unwrap_or_default()
                .into_iter()
                .filter_map(|v| serde_json::from_value(v).ok())
                .collect(),
        )
        .await
        .map_err(error)?;
    project(&app, window.label(), doc)
}
#[tauri::command]
pub async fn workspace_update(
    app: tauri::AppHandle,
    window: WebviewWindow,
    state: State<'_, Sessions>,
    engine: State<'_, Engine>,
    revision: u64,
    mut operation: Operation,
) -> Result<Document, String> {
    let owner = owner(&window).map_err(error)?;
    app.state::<crate::windows::Windows>().writable()?;
    let _work = state.gate.read().await;
    let _opening = state.opening(owner).await;
    app.state::<crate::windows::Windows>().admit(owner)?;
    // Resolve only an explicit new local open; metadata startup never probes repositories.
    if let Operation::AddTab { source, .. } = &mut operation {
        *source = source.clone().clean().map_err(error)?;
        if !source.remote() {
            if let Ok(repo) = GitRepo::open(std::path::Path::new(&source.path)).await {
                source.path = repo.root.to_string_lossy().into_owned();
            }
        }
    }
    let _structure = state.structure.lock().await;
    let store = store(&app).map_err(error)?;
    let previous = store.read().await.map_err(error)?;
    let current = project(&app, owner, previous.clone())?.active_workspace;
    authorize_operation(&app, owner, &previous, &operation)?;
    if previous.revision != revision {
        return Err("Workspace changed; reload and retry".into());
    }
    let protected: Vec<String> = match &operation {
        Operation::CloseTab { id } | Operation::SetPolicy { id, .. } => vec![id.clone()],
        Operation::DeleteWorkspace { id } => previous
            .workspaces
            .iter()
            .filter(|w| w.id == *id)
            .flat_map(|w| w.tabs.iter().map(|t| t.id.clone()))
            .chain(
                previous
                    .closed
                    .iter()
                    .filter(|c| c.workspace == *id)
                    .map(|c| c.tab.id.clone()),
            )
            .collect(),
        _ => vec![],
    };
    let mut leases = vec![];
    let mut draft_counts = HashMap::new();
    for tab in protected {
        leases.push(state.lease(&store, &tab, owner).await.map_err(error)?);
        let count = store
            .view(&tab)
            .await
            .map_err(error)?
            .unsent
            .values()
            .filter(|s| !s.is_empty())
            .count();
        if matches!(operation, Operation::DeleteWorkspace { .. }) && count > 0 {
            return Err(
                "This workspace has unsent drafts. Move or finish those tabs before deleting it"
                    .into(),
            );
        }
        draft_counts.insert(tab, count);
    }
    let retire = match &operation {
        Operation::SetPolicy { id, .. } => Some(id.clone()),
        _ => None,
    };
    let doc = store
        .edit(Some(revision), |doc| {
            doc.active_workspace = current.clone();
            if matches!(operation, Operation::ReopenTab) {
                let at = doc
                    .closed
                    .iter()
                    .position(|c| c.workspace == current)
                    .context("No recently closed tabs in this workspace")?;
                let c = doc.closed.remove(at);
                doc.closed.insert(0, c);
            }
            for (tab, count) in draft_counts {
                if let Ok(t) = doc.tab_mut(&tab) {
                    t.drafts = count;
                } else if let Some(c) = doc.closed.iter_mut().find(|c| c.tab.id == tab) {
                    c.tab.drafts = count;
                }
            }
            apply(doc, operation)?;
            if app
                .state::<crate::windows::Windows>()
                .owner(&doc.active_workspace)
                .is_some_and(|other| other != owner)
            {
                let fallback = doc
                    .workspaces
                    .iter()
                    .find(|w| {
                        app.state::<crate::windows::Windows>()
                            .owner(&w.id)
                            .is_none()
                    })
                    .map(|w| w.id.clone());
                doc.active_workspace = if let Some(fallback) = fallback {
                    fallback
                } else {
                    let next = id();
                    doc.workspaces.push(Workspace {
                        id: next.clone(),
                        name: "Workspace".into(),
                        tabs: vec![],
                        groups: vec![],
                        active: None,
                        recent: vec![],
                    });
                    next
                };
            }
            Ok(())
        })
        .await
        .map_err(error)?;
    app.state::<crate::windows::Windows>()
        .select(owner, &doc.active_workspace)?;
    drop(_structure);
    let ids: HashSet<_> = doc
        .workspaces
        .iter()
        .flat_map(|w| &w.tabs)
        .map(|t| t.id.clone())
        .collect();
    let removed = {
        let mut live = state.live.lock().await;
        let tabs: Vec<_> = live
            .iter()
            .filter(|(tab, b)| {
                b.owner == owner
                    && (!ids.contains(*tab)
                        || retire.as_ref() == Some(*tab)
                        || doc.active_workspace != current)
            })
            .map(|(t, _)| t.clone())
            .collect();
        tabs.into_iter()
            .filter_map(|t| live.remove(&t))
            .collect::<Vec<_>>()
    };
    for b in removed {
        engine.close_workspace_runtime(&b.id, owner).await?;
    }
    let retained: HashSet<_> = doc
        .workspaces
        .iter()
        .flat_map(|w| &w.tabs)
        .chain(doc.closed.iter().map(|c| &c.tab))
        .map(|t| &t.id)
        .collect();
    for tab in previous
        .workspaces
        .iter()
        .flat_map(|w| &w.tabs)
        .chain(previous.closed.iter().map(|c| &c.tab))
        .filter(|t| !retained.contains(&t.id))
    {
        retire_pins(tab.saved.as_ref(), None, &tab.id).await;
    }
    crate::windows::changed(&app);
    emit(&app, &doc);
    project(&app, owner, doc)
}
#[derive(Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Credentials {
    pub(crate) token: Option<String>,
    pub(crate) token_host: Option<String>,
    pub(crate) gitlab_token: Option<String>,
    pub(crate) gitlab_host: Option<String>,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Activated {
    pub gitlab: bool,
    pub opened: OpenedReview,
    pub tab: Tab,
    pub view: View,
    pub notice: Option<String>,
}
#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub async fn workspace_activate(
    app: tauri::AppHandle,
    window: WebviewWindow,
    state: State<'_, Sessions>,
    engine: State<'_, Engine>,
    repositories: State<'_, Repositories>,
    tab: String,
    refresh: bool,
) -> Result<Activated, String> {
    let owner = owner(&window).map_err(error)?;
    let _work = state.gate.read().await;
    let _opening = state.opening(owner).await;
    app.state::<crate::windows::Windows>().admit(owner)?;
    let credentials = app
        .state::<crate::settings::MemorySettings>()
        .credentials()
        .await;
    let store = store(&app).map_err(error)?;
    let document = store.read().await.map_err(error)?;
    tab_owner(&app, owner, &document, &tab)?;
    let current = document.tab(&tab).map_err(error)?.clone();
    let view = store.view(&tab).await.map_err(error)?;
    let lease = state.lease(&store, &tab, owner).await.map_err(error)?;
    let bindings = state.live.lock().await.clone();
    for b in bindings.values().filter(|b| b.owner == owner) {
        let _ = engine.suspend_owned(&b.id, owner).await;
    }
    engine.reap_workspace_cold(owner).await;
    let existing = bindings.get(&tab).filter(|b| b.owner == owner);
    let live = if !refresh {
        if let Some(b) = existing {
            engine.activate_live(&b.id, owner).await?
        } else {
            None
        }
    } else {
        None
    };
    let opened = if let Some(opened) = live {
        opened
    } else {
        if let Some(b) = existing {
            engine.close_workspace_runtime(&b.id, owner).await?;
        }
        let restored = current.saved.as_ref().filter(|_| {
            !refresh || (!current.source.remote() && current.source.path.ends_with("objects.git"))
        });
        if let Some(saved) = restored {
            let config = app
                .path()
                .app_config_dir()
                .map_err(|e| e.to_string())?
                .join("review.json");
            engine
                .restore_workspace_review(
                    saved,
                    current.policy == Policy::Pinned,
                    owner,
                    config,
                    Some(window.clone()),
                )
                .await?
        } else {
            let request = if current.source.request["kind"] == "gitlab" {
                diffle::protocol::EnterRequest::Working
            } else {
                serde_json::from_value(current.source.request.clone()).map_err(|e| e.to_string())?
            };
            let options = OpenOptions {
                tab_scope: Some(tab.clone()),
                path: if current.source.path.is_empty() {
                    ".".into()
                } else {
                    current.source.path.clone()
                },
                request,
                gitlab_url: current
                    .source
                    .request
                    .get("url")
                    .and_then(Value::as_str)
                    .map(str::to_owned),
                token: credentials.token,
                token_host: credentials.token_host,
                gitlab_token: credentials.gitlab_token,
                gitlab_host: credentials.gitlab_host,
            };
            crate::engine::open_owned(app.clone(), window.clone(), &engine, &repositories, options)
                .await?
        }
    };
    let pinned = current.policy == Policy::Pinned || refresh && current.source.remote();
    if pinned {
        engine.pin_workspace_review(&opened.id, owner).await?;
    }
    let saved = engine
        .save_workspace_review(&opened.id, owner, &tab, current.saved.as_ref())
        .await?;
    fn endpoints(saved: &SavedReview) -> (&str, &str) {
        match &saved.review {
            diffle::protocol::Review::Range { old, new, .. } if !saved.mutable() => (old, new),
            _ => (&saved.old, &saved.new),
        }
    }
    let notice=current.saved.as_ref().filter(|old|endpoints(old)!=endpoints(&saved)).map(|old|{
        let (a,b)=endpoints(old);let (c,d)=endpoints(&saved);
        format!("Source revisions changed: {}..{} → {}..{}. The review now shows the refreshed source.",&a[..a.len().min(8)],&b[..b.len().min(8)],&c[..c.len().min(8)],&d[..d.len().min(8)])
    });
    let doc = match store
        .edit(None, |doc| {
            let t = doc.tab_mut(&tab)?;
            if saved.mutable() {
                t.policy = Policy::Follow;
            } else if pinned {
                t.policy = Policy::Pinned;
            }
            t.saved = Some(saved.clone());
            match &saved.archive {
                Some(archive) if !t.source.remote() => {
                    t.source.path = archive.to_string_lossy().into_owned()
                }
                None => t.source.path = saved.root.to_string_lossy().into_owned(),
                _ => {}
            }
            let source = t.source.clone();
            let w = doc
                .workspaces
                .iter_mut()
                .find(|w| w.tabs.iter().any(|t| t.id == tab))
                .unwrap();
            w.active = Some(tab.clone());
            w.recent
                .retain(|s| s.path != source.path || s.request != source.request);
            w.recent.insert(0, source);
            w.recent.truncate(12);
            Ok(())
        })
        .await
    {
        Ok(doc) => doc,
        Err(e) => {
            retire_pins(Some(&saved), current.saved.as_ref(), &tab).await;
            let _ = engine.close_workspace_runtime(&opened.id, owner).await;
            return Err(error(e));
        }
    };
    retire_pins(current.saved.as_ref(), Some(&saved), &tab).await;
    state.live.lock().await.insert(
        tab.clone(),
        Binding {
            owner: owner.into(),
            id: opened.id.clone(),
            _lease: lease,
        },
    );
    emit(&app, &doc);
    Ok(Activated {
        gitlab: engine
            .owned_runtime(&opened.id, owner)
            .await?
            .gitlab
            .is_some(),
        opened,
        tab: doc.tab(&tab).map_err(error)?.clone(),
        view,
        notice,
    })
}
#[tauri::command]
pub async fn workspace_capture(
    app: tauri::AppHandle,
    window: WebviewWindow,
    state: State<'_, Sessions>,
    engine: State<'_, Engine>,
    tab: String,
) -> Result<Document, String> {
    let owner = owner(&window).map_err(error)?;
    let _work = state.gate.read().await;
    let _opening = state.opening(owner).await;
    app.state::<crate::windows::Windows>().allow_save(owner)?;
    let binding = state
        .live
        .lock()
        .await
        .get(&tab)
        .filter(|b| b.owner == owner)
        .cloned()
        .ok_or("Tab is not open in this window")?;
    let previous = store(&app).map_err(error)?.read().await.map_err(error)?;
    tab_owner(&app, owner, &previous, &tab)?;
    if !engine.workspace_is_live(&binding.id, owner).await {
        return project(&app, owner, previous);
    }
    if previous.tab(&tab).map_err(error)?.policy == Policy::Pinned {
        engine.pin_workspace_review(&binding.id, owner).await?;
    }
    let saved = engine
        .save_workspace_review(
            &binding.id,
            owner,
            &tab,
            previous.tab(&tab).map_err(error)?.saved.as_ref(),
        )
        .await?;
    if serde_json::to_value(previous.tab(&tab).map_err(error)?.saved.as_ref()).ok()
        == serde_json::to_value(Some(&saved)).ok()
    {
        return project(&app, owner, previous);
    }
    let doc = store(&app)
        .map_err(error)?
        .edit(None, |doc| {
            let t = doc.tab_mut(&tab)?;
            if saved.mutable() {
                t.policy = Policy::Follow;
            }
            t.saved = Some(saved.clone());
            Ok(())
        })
        .await;
    let doc = match doc {
        Ok(doc) => doc,
        Err(e) => {
            retire_pins(
                Some(&saved),
                previous.tab(&tab).ok().and_then(|t| t.saved.as_ref()),
                &tab,
            )
            .await;
            return Err(error(e));
        }
    };
    retire_pins(
        previous.tab(&tab).ok().and_then(|t| t.saved.as_ref()),
        Some(&saved),
        &tab,
    )
    .await;
    emit(&app, &doc);
    project(&app, owner, doc)
}
#[tauri::command]
pub async fn workspace_save_view(
    app: tauri::AppHandle,
    state: State<'_, Sessions>,
    window: WebviewWindow,
    tab: String,
    revision: u64,
    view: View,
) -> Result<View, String> {
    let owner = owner(&window).map_err(error)?;
    let _work = state.gate.read().await;
    let _opening = state.opening(owner).await;
    app.state::<crate::windows::Windows>().allow_save(owner)?;
    let store = store(&app).map_err(error)?;
    let _lease = state
        .lease(&store, &tab, window.label())
        .await
        .map_err(error)?;
    let doc = store.read().await.map_err(error)?;
    tab_owner(&app, window.label(), &doc, &tab)?;
    if doc.tab(&tab).is_err() && !doc.closed.iter().any(|c| c.tab.id == tab) {
        return Err("Tab no longer exists".into());
    }
    let view = store.save_view(&tab, revision, view).await.map_err(error)?;
    let count = view.unsent.values().filter(|s| !s.is_empty()).count();
    if doc.tab(&tab).ok().map(|t| t.drafts).or_else(|| {
        doc.closed
            .iter()
            .find(|c| c.tab.id == tab)
            .map(|c| c.tab.drafts)
    }) != Some(count)
    {
        let doc = store
            .edit(None, |d| {
                if let Ok(t) = d.tab_mut(&tab) {
                    t.drafts = count;
                } else if let Some(c) = d.closed.iter_mut().find(|c| c.tab.id == tab) {
                    c.tab.drafts = count;
                }
                Ok(())
            })
            .await
            .map_err(error)?;
        emit(&app, &doc);
    }
    Ok(view)
}

#[tauri::command]
pub async fn workspace_read_view(
    app: tauri::AppHandle,
    window: WebviewWindow,
    tab: String,
) -> Result<View, String> {
    owner(&window).map_err(error)?;
    let store = store(&app).map_err(error)?;
    let doc = store.read().await.map_err(error)?;
    tab_owner(&app, window.label(), &doc, &tab)?;
    if doc.tab(&tab).is_err() && !doc.closed.iter().any(|c| c.tab.id == tab) {
        return Err("Tab no longer exists".into());
    }
    store.view(&tab).await.map_err(error)
}
#[tauri::command]
pub async fn workspace_release(
    window: WebviewWindow,
    state: State<'_, Sessions>,
    engine: State<'_, Engine>,
    tab: String,
) -> Result<(), String> {
    let owner = owner(&window).map_err(error)?;
    let _work = state.gate.read().await;
    let _opening = state.opening(owner).await;
    window
        .app_handle()
        .state::<crate::windows::Windows>()
        .admit(owner)?;
    let b = {
        let mut live = state.live.lock().await;
        if live.get(&tab).is_some_and(|b| b.owner != owner) {
            return Err("Tab belongs to another window".into());
        }
        live.remove(&tab)
    };
    if let Some(b) = b {
        engine.close_workspace_runtime(&b.id, owner).await?;
    }
    Ok(())
}

/// Explicit duplication copies the source/checkpoint, never unsent text, trust or credentials.
pub(crate) async fn duplicate_checkpoint(
    saved: &SavedReview,
    tab: &str,
) -> Result<SavedReview, String> {
    let mut copy = saved.clone();
    let logical = saved
        .key
        .splitn(3, '/')
        .nth(2)
        .ok_or("Invalid source identity")?;
    copy.key = format!("workspace/{tab}/{logical}");
    let at = saved.archive.as_ref().unwrap_or(&saved.root);
    let generation = id();
    let mut pins = vec![];
    for (i, old) in saved.pins.iter().enumerate() {
        let sha =
            diffle::git::exec::exec_git(at, &["rev-parse", "--verify", old], Default::default())
                .await
                .map_err(|e| e.to_string())?;
        let sha = String::from_utf8(sha).map_err(|e| e.to_string())?;
        let sha = sha.trim();
        if ![40, 64].contains(&sha.len()) || !sha.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err("Invalid pinned source object".into());
        }
        pins.push((
            format!("refs/diffle/iterations/workspaces/{tab}/{generation}/{i}"),
            sha.to_owned(),
        ));
    }
    // One Git transaction: a failed copy cannot publish half a pin set.
    let input = pins
        .iter()
        .map(|(name, sha)| format!("create {name} {sha}\n"))
        .collect::<String>();
    diffle::git::exec::exec_git(
        at,
        &["update-ref", "--stdin"],
        diffle::git::exec::ExecOptions {
            input: Some(input.as_bytes()),
            ..Default::default()
        },
    )
    .await
    .map_err(|e| e.to_string())?;
    copy.pins = pins.into_iter().map(|(name, _)| name).collect();
    if let Some(archive) = &copy.archive {
        // Duplication gets a fresh checkout; the original tab's dirty working directory stays its own.
        copy.root = archive
            .parent()
            .ok_or("Missing archive")?
            .join("checkouts")
            .join(format!("new-{tab}"));
    }
    Ok(copy)
}
#[tauri::command]
#[allow(clippy::too_many_arguments)] // Five arguments are native-injected; callers specify workspace, copy and revision.
pub async fn workspace_open_window(
    app: tauri::AppHandle,
    window: WebviewWindow,
    state: State<'_, Sessions>,
    engine: State<'_, Engine>,
    repositories: State<'_, Repositories>,
    workspace: String,
    duplicate_tab: Option<String>,
    revision: u64,
) -> Result<Document, String> {
    let owner = owner(&window).map_err(error)?;
    let _work = state.gate.read().await;
    let _opening = state.opening(owner).await;
    let windows = app.state::<crate::windows::Windows>();
    windows.admit(owner)?;
    let _structure = state.structure.lock().await;
    let store = store(&app).map_err(error)?;
    let previous = store.read().await.map_err(error)?;
    if previous.revision != revision {
        return Err("Workspace changed; reload and retry".into());
    }
    let current = project(&app, owner, previous.clone())?.active_workspace;
    if let Some(other) = windows.owner(&workspace).filter(|other| other != owner) {
        if let Some(other) = app.get_webview_window(&other) {
            other.unminimize().map_err(|e| e.to_string())?;
            other.set_focus().map_err(|e| e.to_string())?;
        }
        return project(&app, owner, previous);
    }
    if windows.list(owner).len() >= crate::windows::MAX_WINDOWS {
        return Err(
            "At most two workspace windows are supported. Focus or close the other window first"
                .into(),
        );
    }
    let mut duplicate = None;
    let destination = if let Some(tab) = duplicate_tab {
        tab_owner(&app, owner, &previous, &tab)?;
        let original = previous.tab(&tab).map_err(error)?;
        let next = id();
        let saved = match &original.saved {
            Some(saved) => Some(duplicate_checkpoint(saved, &next).await?),
            None => None,
        };
        let mut source = original.source.clone();
        source.label = format!("{} copy", source.label).chars().take(100).collect();
        let record = Tab {
            id: next,
            source,
            group: None,
            policy: original.policy,
            saved,
            drafts: 0,
        };
        let target = id();
        duplicate = Some((target.clone(), record));
        target
    } else {
        previous
            .workspaces
            .iter()
            .find(|w| w.id == workspace)
            .ok_or("Workspace no longer exists")?;
        workspace
    };
    let label = format!("workspace-{}", id());
    let doc = store
        .edit(Some(revision), |doc| {
            doc.active_workspace = current.clone();
            if let Some((target, tab)) = &duplicate {
                doc.workspaces.push(Workspace {
                    id: target.clone(),
                    name: name(&tab.source.label)?,
                    tabs: vec![tab.clone()],
                    groups: vec![],
                    active: Some(tab.id.clone()),
                    recent: vec![],
                });
            } else if current == destination {
                let fallback = doc
                    .workspaces
                    .iter()
                    .find(|w| w.id != destination && windows.owner(&w.id).is_none())
                    .map(|w| w.id.clone());
                doc.active_workspace = if let Some(fallback) = fallback {
                    fallback
                } else {
                    let next = id();
                    doc.workspaces.push(Workspace {
                        id: next.clone(),
                        name: "Workspace".into(),
                        tabs: vec![],
                        groups: vec![],
                        active: None,
                        recent: vec![],
                    });
                    next
                };
            } else {
                doc.active_workspace = current.clone();
            }
            Ok(())
        })
        .await;
    let doc = match doc {
        Ok(doc) => doc,
        Err(e) => {
            if let Some((_, t)) = &duplicate {
                retire_pins(t.saved.as_ref(), None, &t.id).await;
            }
            return Err(error(e));
        }
    };
    if duplicate.is_none() && current == destination {
        release_owner(&state, &engine, &repositories, owner).await;
        windows.select(owner, &doc.active_workspace)?;
    }
    windows.reserve(&label, &destination)?;
    let built =
        tauri::WebviewWindowBuilder::new(&app, &label, tauri::WebviewUrl::App("index.html".into()))
            .title("mdiff — workspace")
            .inner_size(1280.0, 820.0)
            .min_inner_size(640.0, 480.0)
            .build();
    if let Err(e) = built {
        windows.remove(&label);
        let _ = windows.select(owner, &current);
        emit(&app, &doc);
        crate::windows::changed(&app);
        return Err(format!(
            "Could not open workspace window; saved tabs are kept: {e}"
        ));
    }
    emit(&app, &doc);
    crate::windows::changed(&app);
    project(&app, owner, doc)
}
async fn release_owner(
    state: &Sessions,
    engine: &Engine,
    repositories: &Repositories,
    owner: &str,
) {
    repositories.release_owner(owner).await;
    let removed = {
        let mut live = state.live.lock().await;
        let tabs = live
            .iter()
            .filter(|(_, b)| b.owner == owner)
            .map(|(t, _)| t.clone())
            .collect::<Vec<_>>();
        tabs.into_iter()
            .filter_map(|t| live.remove(&t))
            .collect::<Vec<_>>()
    };
    for b in removed {
        let _ = engine.close_workspace_runtime(&b.id, owner).await;
    }
}
#[tauri::command]
pub async fn workspace_move_window(
    app: tauri::AppHandle,
    window: WebviewWindow,
    state: State<'_, Sessions>,
    engine: State<'_, Engine>,
    tab: String,
    target: String,
    revision: u64,
) -> Result<Document, String> {
    let owner = owner(&window).map_err(error)?;
    let _work = state.gate.read().await;
    let _opening = state.opening(owner).await;
    let windows = app.state::<crate::windows::Windows>();
    windows.admit(owner)?;
    let _structure = state.structure.lock().await;
    let destination = windows
        .list(owner)
        .into_iter()
        .find(|w| w.label == target && !w.current)
        .and_then(|w| w.workspace)
        .ok_or("Destination window is no longer open")?;
    let store = store(&app).map_err(error)?;
    let previous = store.read().await.map_err(error)?;
    tab_owner(&app, owner, &previous, &tab)?;
    let current = project(&app, owner, previous.clone())?.active_workspace;
    let doc = store
        .edit(Some(revision), |doc| {
            apply(
                doc,
                Operation::MoveWorkspace {
                    id: tab.clone(),
                    workspace: destination,
                },
            )?;
            doc.active_workspace = current;
            Ok(())
        })
        .await
        .map_err(error)?;
    let binding = { state.live.lock().await.remove(&tab) };
    if let Some(binding) = binding {
        engine.close_workspace_runtime(&binding.id, owner).await?;
    }
    emit(&app, &doc);
    project(&app, owner, doc)
}
#[tauri::command]
pub async fn workspace_register_ui(window: WebviewWindow) -> Result<(), String> {
    crate::windows::authorize(&window)?;
    window
        .app_handle()
        .state::<crate::windows::Windows>()
        .register(window.label())
}
#[tauri::command]
pub async fn workspace_finish_exit(
    app: tauri::AppHandle,
    window: WebviewWindow,
    action: String,
    generation: Option<u64>,
) -> Result<(), String> {
    crate::windows::authorize(&window)?;
    if !["close", "quit", "restart"].contains(&action.as_str()) {
        return Err("Invalid close action".into());
    }
    let windows = app.state::<crate::windows::Windows>();
    if windows.exiting() {
        if let Some(action) = windows.acknowledge(window.label(), generation)? {
            crate::windows::finish(&app, action).await;
        }
    } else if action == "close" {
        let sessions = app.state::<Sessions>();
        let _work = sessions.gate.read().await;
        let _opening = sessions.opening(window.label()).await;
        windows.prepare_close(window.label(), generation)?;
        if let Err(e) = app
            .state::<crate::terminal::Terminals>()
            .close_owner(window.label())
            .await
        {
            let _ = windows.register(window.label());
            return Err(error(e));
        }
        app.state::<Repositories>()
            .release_owner(window.label())
            .await;
        app.state::<Engine>().release_owner(window.label()).await;
        if let Err(e) = window.close() {
            let _ = windows.register(window.label());
            return Err(e.to_string());
        }
    } else {
        return Err("Exit request was cancelled".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn source(i: usize) -> Source {
        Source {
            path: format!("/missing/repository-{i}"),
            request: json!({"kind":"working"}),
            label: format!("Review {i}"),
        }
    }
    #[tokio::test]
    async fn unrelated_windows_prepare_independently_but_shutdown_waits_for_accepted_work() {
        let sessions = std::sync::Arc::new(Sessions::default());
        let work = sessions.gate.read().await;
        let first = sessions.opening("main").await;
        let other = tokio::time::timeout(
            std::time::Duration::from_millis(100),
            sessions.opening("workspace-other"),
        )
        .await
        .expect("another window must not wait on main's preparation");
        drop(other);
        let done = std::sync::Arc::new(AtomicBool::new(false));
        let task = {
            let sessions = sessions.clone();
            let done = done.clone();
            tokio::spawn(async move {
                let _finish = sessions.gate.write().await;
                done.store(true, Ordering::SeqCst);
            })
        };
        tokio::task::yield_now().await;
        assert!(!done.load(Ordering::SeqCst));
        drop(first);
        drop(work);
        task.await.unwrap();
        assert!(done.load(Ordering::SeqCst));
    }
    #[test]
    fn tab_ownership_excludes_an_independent_process_and_releases_after_exit() {
        use std::io::{BufRead, BufReader, Write};
        use std::process::{Command, Stdio};
        let root = tempfile::tempdir().unwrap();
        let store = Store {
            file: root.path().join("workspaces.json"),
        };
        let tab = id();
        drop(store.tab_lease(&tab).unwrap());
        let file = root.path().join("workspace-leases").join(&tab);
        let mut child = Command::new("python3").args(["-u", "-c", "import fcntl,sys; f=open(sys.argv[1],'r+'); fcntl.flock(f,fcntl.LOCK_EX); print('ready',flush=True); sys.stdin.readline()"])
            .arg(file).stdin(Stdio::piped()).stdout(Stdio::piped()).spawn().unwrap();
        let mut ready = String::new();
        BufReader::new(child.stdout.take().unwrap())
            .read_line(&mut ready)
            .unwrap();
        assert_eq!(ready.trim(), "ready");
        assert!(store.tab_lease(&tab).is_err());
        child.stdin.take().unwrap().write_all(b"done\n").unwrap();
        assert!(child.wait().unwrap().success());
        assert!(store.tab_lease(&tab).is_ok());
    }
    #[tokio::test]
    async fn twenty_cold_tabs_restore_metadata_groups_order_and_closed_drafts_without_repository_access(
    ) {
        let root = tempfile::tempdir().unwrap();
        let store = Store {
            file: root.path().join("workspaces.json"),
        };
        let initial = store.load(vec![source(99)]).await.unwrap();
        let workspace = initial.active_workspace.clone();
        let mut doc = initial;
        for i in 0..20 {
            doc = store
                .edit(Some(doc.revision), |d| {
                    apply(
                        d,
                        Operation::AddTab {
                            workspace: workspace.clone(),
                            source: source(i),
                        },
                    )
                })
                .await
                .unwrap();
        }
        let first = doc.workspaces[0].tabs[0].id.clone();
        let last = doc.workspaces[0].tabs[19].id.clone();
        doc = store
            .edit(Some(doc.revision), |d| {
                apply(
                    d,
                    Operation::CreateGroup {
                        workspace: workspace.clone(),
                        name: "Backend".into(),
                        color: "blue".into(),
                    },
                )
            })
            .await
            .unwrap();
        let group = doc.workspaces[0].groups[0].id.clone();
        doc = store
            .edit(Some(doc.revision), |d| {
                apply(
                    d,
                    Operation::SetGroup {
                        id: first.clone(),
                        group: Some(group.clone()),
                    },
                )?;
                apply(
                    d,
                    Operation::MoveTab {
                        id: last.clone(),
                        before: Some(first.clone()),
                    },
                )
            })
            .await
            .unwrap();
        assert_eq!(doc.workspaces[0].tabs[0].id, last);
        let view = View {
            revision: 0,
            unsent: HashMap::from([("working\0draft\0f\0file".into(), "unsent".into())]),
            navigation: Some(json!({"activePath":"f","paneScroll":120})),
        };
        store.save_view(&last, 0, view).await.unwrap();
        doc = store
            .edit(Some(doc.revision), |d| {
                d.tab_mut(&last)?.drafts = 1;
                apply(d, Operation::CloseTab { id: last.clone() })
            })
            .await
            .unwrap();
        doc = store
            .edit(Some(doc.revision), |d| apply(d, Operation::ReopenTab))
            .await
            .unwrap();
        let restored = Store {
            file: store.file.clone(),
        }
        .load(vec![source(88)])
        .await
        .unwrap();
        assert_eq!(restored.revision, doc.revision);
        assert_eq!(restored.workspaces[0].tabs.len(), 20);
        assert!(restored.workspaces[0]
            .tabs
            .iter()
            .all(|t| t.saved.is_none()));
        assert_eq!(restored.workspaces[0].groups[0].name, "Backend");
        assert_eq!(
            restored.workspaces[0].recent[0].path,
            "/missing/repository-99"
        );
        assert_eq!(
            store
                .view(&last)
                .await
                .unwrap()
                .unsent
                .values()
                .next()
                .unwrap(),
            "unsent"
        );
        assert!(!root.path().join("workspace-leases").exists());
    }
    #[tokio::test]
    async fn concurrent_metadata_writers_cannot_overwrite_each_other_or_corrupt_failed_updates() {
        let root = tempfile::tempdir().unwrap();
        let file = root.path().join("workspaces.json");
        let a = Store { file: file.clone() };
        let b = Store { file: file.clone() };
        let doc = a.load(vec![]).await.unwrap();
        let id = doc.active_workspace;
        let (one, two) = tokio::join!(
            a.edit(Some(0), |d| apply(
                d,
                Operation::RenameWorkspace {
                    id: id.clone(),
                    name: "One".into()
                }
            )),
            b.edit(Some(0), |d| apply(
                d,
                Operation::RenameWorkspace {
                    id: id.clone(),
                    name: "Two".into()
                }
            ))
        );
        assert_ne!(one.is_ok(), two.is_ok());
        let before = tokio::fs::read(&file).await.unwrap();
        assert!(a
            .edit(Some(1), |d| apply(d, Operation::DeleteWorkspace { id }))
            .await
            .is_err());
        assert_eq!(tokio::fs::read(&file).await.unwrap(), before);
        tokio::fs::write(&file, "{broken").await.unwrap();
        assert!(a.load(vec![]).await.is_err());
        assert_eq!(tokio::fs::read_to_string(&file).await.unwrap(), "{broken");
    }
    #[tokio::test]
    async fn tab_state_uses_revision_checks_and_keeps_originals_on_limit_or_version_errors() {
        let root = tempfile::tempdir().unwrap();
        let store = Store {
            file: root.path().join("workspaces.json"),
        };
        store.load(vec![]).await.unwrap();
        let tab = id();
        let view = View {
            unsent: HashMap::from([("draft".into(), "kept".into())]),
            ..View::default()
        };
        store.save_view(&tab, 0, view.clone()).await.unwrap();
        assert!(store.save_view(&tab, 0, View::default()).await.is_err());
        let too_large = View {
            unsent: HashMap::from([("draft".into(), "x".repeat(MAX_VIEW as usize))]),
            ..View::default()
        };
        assert!(store.save_view(&tab, 1, too_large).await.is_err());
        assert_eq!(store.view(&tab).await.unwrap().unsent["draft"], "kept");
        let mut doc = store.read().await.unwrap();
        doc.version = 99;
        let original = serde_json::to_string(&doc).unwrap();
        tokio::fs::write(&store.file, &original).await.unwrap();
        assert!(store.load(vec![]).await.is_err());
        assert_eq!(
            tokio::fs::read_to_string(&store.file).await.unwrap(),
            original
        );
    }
    #[test]
    fn local_source_roundtrips_unusual_worktree_paths_without_relaxing_revision_validation() {
        let path = "/tmp/linked 'quotes'\r\n— folder";
        let source = Source {
            path: path.into(),
            request: json!({"kind":"working"}),
            label: "Working changes".into(),
        }
        .clean()
        .unwrap();
        let restored: Source =
            serde_json::from_str(&serde_json::to_string(&source).unwrap()).unwrap();
        assert_eq!(restored.clean().unwrap().path, path);
        assert!(Source {
            path: path.into(),
            request: json!({"kind":"range","args":["HEAD\nunsafe"]}),
            label: "Invalid revision".into(),
        }
        .clean()
        .is_err());
    }
    #[test]
    fn source_migration_strips_extra_fields_and_rejects_embedded_credentials() {
        assert!(Source {
            path: String::new(),
            request: json!({"kind":"pr","pr":"https://user:password@github.com/a/b/pull/1"}),
            label: "private".into()
        }
        .clean()
        .is_err());
        let cleaned = Source {
            path: String::new(),
            request: json!({"kind":"working","token":"must not persist"}),
            label: "Working changes".into(),
        }
        .clean();
        if let Ok(cleaned) = cleaned {
            assert!(!serde_json::to_string(&cleaned)
                .unwrap()
                .contains("must not persist"));
        }
    }
}
