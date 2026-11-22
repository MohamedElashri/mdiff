use diffle::{
    cli::{
        lsp,
        repository::{open_review_repository, open_review_repository_with},
    },
    git::GitRepo,
    github::{
        client::HttpClient,
        hosts::{choose_instance, Github},
    },
    lsp::pool::LspPool,
    protocol::EnterRequest,
    server::{desktop_router, hub::Hub, ApiDeps},
    session::{Session, SessionOptions},
    user_config::UserConfigStore,
};
use serde::Serialize;
use std::{
    collections::{HashMap, HashSet},
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc, Weak,
    },
};
use tauri::{Emitter, Manager, State, WebviewWindow};
use tokio::sync::{
    watch, Mutex, OwnedMappedMutexGuard, OwnedMutexGuard, OwnedSemaphorePermit, Semaphore,
};

pub const MAX_LIVE_REVIEWS: usize = 2;
pub const MAX_DESCRIPTORS: usize = 64;
/// Native-created workspace checkpoint. No ephemeral runtime ID, auth client or trust.
#[derive(Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct SavedReview {
    pub key: String,
    pub root: PathBuf,
    pub data_root: PathBuf,
    pub archive: Option<PathBuf>,
    pub range_key: Option<String>,
    pub review: diffle::protocol::Review,
    pub source_review: diffle::protocol::Review,
    #[serde(default)]
    pub request: Option<EnterRequest>,
    pub has_iterations: bool,
    pub head: String,
    pub old: String,
    pub new: String,
    pub pins: Vec<String>,
}
impl SavedReview {
    pub(crate) fn mutable(&self) -> bool {
        matches!(&self.source_review,diffle::protocol::Review::Range{old,new,..} if old=="worktree"||new=="worktree")
    }
}
pub struct Engine {
    handles: Mutex<HashMap<String, Arc<ReviewHandle>>>,
    openers: Mutex<HashMap<String, Weak<Mutex<()>>>>,
    slots: Arc<Semaphore>,
    lsp_slots: Arc<Semaphore>,
    pub(crate) lifecycle: Mutex<()>, // Settings authority only; never held through preparation/network I/O.
    config: tokio::sync::OnceCell<Arc<UserConfigStore>>,
    clock: AtomicU64,
    credentials: Mutex<HashMap<String, String>>,
    shutting_down: AtomicBool,
    closed_windows: Mutex<HashSet<String>>,
}
impl Default for Engine {
    fn default() -> Self {
        Self {
            handles: Mutex::default(),
            openers: Mutex::default(),
            slots: Arc::new(Semaphore::new(MAX_LIVE_REVIEWS)),
            lsp_slots: Arc::new(Semaphore::new(1)),
            lifecycle: Mutex::new(()),
            config: tokio::sync::OnceCell::new(),
            clock: AtomicU64::new(0),
            credentials: Mutex::default(),
            shutting_down: AtomicBool::new(false),
            closed_windows: Mutex::default(),
        }
    }
}
pub(crate) struct ReviewHandle {
    pub owner: String,
    pub key: String,
    root: PathBuf,
    checkout: Option<Weak<crate::storage::Checkout>>,
    live: Arc<Mutex<Option<ReviewRuntime>>>,
    pub visible: AtomicBool,
    touched: AtomicU64,
    descriptor: Mutex<Option<ColdReview>>,
}
#[derive(Clone, Serialize)]
pub struct ColdReview {
    pub id: String,
    pub key: String,
    pub root: PathBuf,
    pub review: diffle::protocol::Review,
    pub has_iterations: bool,
    pub checkout_head: String,
    #[serde(skip)]
    pins: Vec<String>,
    pub data_root: PathBuf,
    pub range_key: Option<String>,
    pub archive: Option<PathBuf>,
    pub github_enabled: bool,
    pub state: &'static str,
}
pub struct ReviewRuntime {
    pub(crate) config: Arc<UserConfigStore>,
    pub(crate) router: axum::Router,
    pub(crate) id: String,
    pub(crate) gitlab: Option<Arc<crate::gitlab::Gitlab>>,
    pub(crate) github: Arc<crate::github::Integration>,
    events: Option<tokio::task::JoinHandle<()>>,
    settings: Option<tokio::task::JoinHandle<()>>,
    configured_hosts: std::collections::HashMap<String, String>,
    trusted_commands: Option<std::collections::HashMap<diffle::protocol::LanguageId, String>>,
    closing: watch::Sender<bool>,
    hub: Arc<Hub>,
    session: Arc<Session>,
    repo: Arc<GitRepo>,
    lsp: Option<Arc<LspPool>>,
    temporary: Option<Arc<tempfile::TempDir>>,
    checkout: Option<Arc<crate::storage::Checkout>>,
    slot: Option<OwnedSemaphorePermit>,
    lsp_slot: Option<OwnedSemaphorePermit>,
    closed: bool,
}
impl ReviewRuntime {
    async fn enable_lsp(&mut self) -> anyhow::Result<diffle::protocol::LspStatus> {
        if self.lsp.is_none() {
            let snapshot = self.session.snapshotter()?.current().await?;
            let commands = self.config.get().lsp_commands;
            let pool =
                lsp::start_limited(&self.session, &self.hub, commands.clone(), vec![], Some(4));
            self.router = crate::github::router(
                desktop_router(ApiDeps {
                    session: self.session.clone(),
                    config: self.config.clone(),
                    extra_auto_viewed: vec![],
                    hub: self.hub.clone(),
                    lsp: Some(pool.clone()),
                }),
                &self.github,
            );
            self.trusted_commands = Some(commands);
            self.lsp = Some(pool.clone());
            tokio::spawn(pool.track(&snapshot));
        }
        Ok(self.lsp.as_ref().unwrap().status())
    }

    async fn disable_lsp(&mut self) {
        let pool = self.lsp.take();
        let permit = self.lsp_slot.take();
        self.trusted_commands = None;
        let _ = tokio::spawn(async move {
            if let Some(pool) = pool {
                pool.close().await;
            }
            drop(permit);
        })
        .await;
        self.router = crate::github::router(
            desktop_router(ApiDeps {
                session: self.session.clone(),
                config: self.config.clone(),
                extra_auto_viewed: vec![],
                hub: self.hub.clone(),
                lsp: None,
            }),
            &self.github,
        );
        use diffle::session::Broadcaster;
        self.hub.broadcast(diffle::protocol::ServerMessage::Lsp {
            payload: diffle::protocol::LspStatus {
                enabled: false,
                servers: vec![],
                missing: vec![],
            },
        });
    }
    fn follow_settings(&mut self) {
        let mut changes = self.config.subscribe();
        let config = self.config.clone();
        let session = Arc::downgrade(&self.session);
        let hub = self.hub.clone();
        self.settings = Some(tokio::spawn(async move {
            loop {
                let settings = config.get();
                let Some(session) = session.upgrade() else {
                    break;
                };
                session.set_follow_refs(settings.follow_refs);
                let _ = session
                    .set_context(settings.context_lines.clamp(0, 10000) as u32)
                    .await;
                use diffle::session::Broadcaster;
                hub.broadcast(diffle::protocol::ServerMessage::Config { auth_changed: None });
                drop(session);
                if changes.changed().await.is_err() {
                    break;
                }
            }
        }));
    }
    pub(crate) async fn agent_comparison(&self) -> anyhow::Result<(String, String, String)> {
        let (_, _, snapshot) = self.session.checkpoint().await?;
        let query = diffle::protocol::ThreadQuery {
            state: Some(diffle::protocol::ThreadState::Open),
            ..Default::default()
        };
        let prompt = diffle::comments::format::format_prompt(
            &self.session.comments()?.threads(&query).await?,
        );
        anyhow::ensure!(
            prompt.len() <= 32768,
            "Exported comments exceed the 32 KB agent prompt limit"
        );
        Ok((snapshot.old_sha.clone(), snapshot.new_sha.clone(), prompt))
    }
    pub(crate) async fn hosting_revision(&self) -> anyhow::Result<(String, String)> {
        let (_, _, snapshot) = self.session.checkpoint().await?;
        Ok((
            snapshot.commits.old_sha.clone(),
            snapshot.commits.new_sha.clone(),
        ))
    }
    pub(crate) async fn hosting_base(&self, base: &str, head: &str) -> anyhow::Result<String> {
        Ok(self.repo.merge_base(base, head).await?)
    }
    pub(crate) fn hosting_pr(&self) -> anyhow::Result<Option<String>> {
        Ok(self.session.pr_url()?)
    }
    pub(crate) fn repository_root(&self) -> &std::path::Path {
        &self.repo.root
    }
    pub(crate) fn temporary_repository(&self) -> bool {
        self.temporary.is_some() || self.checkout.is_some()
    }
    pub(crate) fn temporary_checkout(&self) -> Option<Arc<tempfile::TempDir>> {
        self.temporary.clone()
    }
    pub(crate) fn owned_checkout(&self) -> Option<Arc<crate::storage::Checkout>> {
        self.checkout.clone()
    }
    pub(crate) fn closing(&self) -> watch::Receiver<bool> {
        self.closing.subscribe()
    }

    async fn close(mut self) {
        self.closing.send_replace(true);
        if let Some(settings) = self.settings.take() {
            settings.abort();
        }
        self.github
            .enabled
            .store(false, std::sync::atomic::Ordering::SeqCst);
        self.hub.close();
        if let Some(events) = self.events.take() {
            events.abort();
        }
        let close_lsp = async {
            if let Some(pool) = &self.lsp {
                pool.close().await;
            }
        };
        self.session.close().await;
        let _ = tokio::time::timeout(std::time::Duration::from_secs(5), async {
            close_lsp.await;
        })
        .await;
        let _ = self.repo.clean_review_refs().await;
        self.closed = true;
    }
}
impl Drop for ReviewRuntime {
    fn drop(&mut self) {
        if self.closed {
            return;
        }
        self.closing.send_replace(true);
        self.github.invalidate(false);
        self.hub.close();
        if let Some(task) = self.events.take() {
            task.abort();
        }
        if let Some(task) = self.settings.take() {
            task.abort();
        }
        let session = self.session.clone();
        let repo = self.repo.clone();
        let pool = self.lsp.take();
        let leases = (
            self.slot.take(),
            self.lsp_slot.take(),
            self.temporary.take(),
            self.checkout.take(),
        );
        if let Ok(runtime) = tokio::runtime::Handle::try_current() {
            runtime.spawn(async move {
                let _leases = leases;
                session.close().await;
                if let Some(pool) = pool {
                    let _ =
                        tokio::time::timeout(std::time::Duration::from_secs(5), pool.close()).await;
                }
                let _ = repo.clean_review_refs().await;
            });
        }
    }
}
impl Engine {
    /// Recompute only live related sessions; cold mutable descriptors already rebuild
    /// from disk on activation. Session refresh owns relocation and coalesces bursts.
    pub(crate) async fn refresh_git(&self, common: &std::path::Path) {
        for handle in self.handles().await {
            let session = {
                let live = handle.live.lock().await;
                live.as_ref()
                    .filter(|runtime| runtime.repo.common_dir == common)
                    .map(|runtime| runtime.session.clone())
            };
            if let Some(session) = session {
                drop(session.refresh());
            }
        }
    }

    pub(crate) async fn uses_checkout(&self, root: &std::path::Path) -> bool {
        self.handles()
            .await
            .iter()
            .any(|handle| handle.root == root)
    }

    pub(crate) async fn workspace_is_live(&self, id: &str, owner: &str) -> bool {
        match self.handle(id, Some(owner)).await {
            Ok(handle) => handle.live.lock().await.is_some(),
            Err(_) => false,
        }
    }
    pub(crate) async fn reap_workspace_cold(&self, owner: &str) {
        for handle in self.handles().await {
            if handle.owner == owner
                && handle.key.starts_with("workspace/")
                && handle.live.lock().await.is_none()
            {
                let id = {
                    let handles = self.handles.lock().await;
                    handles
                        .iter()
                        .find(|(_, h)| Arc::ptr_eq(h, &handle))
                        .map(|(id, _)| id.clone())
                };
                if let Some(id) = id {
                    let _ = self.close_owned(&id, owner).await;
                }
            }
        }
    }
    pub(crate) async fn suspend_owned(&self, id: &str, owner: &str) -> Result<(), String> {
        let handle = self.handle(id, Some(owner)).await?;
        handle.visible.store(false, Ordering::SeqCst);
        Ok(())
    }
    pub(crate) async fn close_workspace_runtime(
        &self,
        id: &str,
        owner: &str,
    ) -> Result<(), String> {
        self.close_owned(id, owner).await
    }
    pub(crate) async fn activate_live(
        &self,
        id: &str,
        owner: &str,
    ) -> Result<Option<OpenedReview>, String> {
        let handle = match self.handle(id, Some(owner)).await {
            Ok(h) => h,
            Err(_) => return Ok(None),
        };
        if handle.live.lock().await.is_none() {
            return Ok(None);
        }
        Ok(self.duplicate(&handle.key, owner).await)
    }
    pub(crate) async fn pin_workspace_review(&self, id: &str, owner: &str) -> Result<(), String> {
        let runtime = self.owned_runtime(id, owner).await?;
        if !matches!(runtime.session.review().map_err(|e| e.to_string())?, diffle::protocol::Review::Range { ref old, ref new, .. } if old=="worktree" || new=="worktree")
        {
            runtime.session.pin().await.map_err(|e| e.to_string())?;
        }
        Ok(())
    }
    pub(crate) async fn save_workspace_review(
        &self,
        id: &str,
        owner: &str,
        tab: &str,
        previous: Option<&SavedReview>,
    ) -> Result<SavedReview, String> {
        let runtime = self.owned_runtime(id, owner).await?;
        let handle = self.handle(id, Some(owner)).await?;
        let (review, range_key, snapshot) = runtime
            .session
            .checkpoint()
            .await
            .map_err(|e| e.to_string())?;
        let source_review = runtime.session.review().map_err(|e| e.to_string())?;
        let request = runtime.session.source_request();
        if let Some(previous) = previous {
            if previous.root == runtime.repo.root
                && previous.old == snapshot.old_sha
                && previous.new == snapshot.new_sha
                && previous.head == snapshot.head_sha
                && previous.range_key == range_key
                && serde_json::to_value(&previous.review).ok() == serde_json::to_value(&review).ok()
                && serde_json::to_value(&previous.request).ok()
                    == serde_json::to_value(&request).ok()
                && serde_json::to_value(&previous.source_review).ok()
                    == serde_json::to_value(&source_review).ok()
            {
                return Ok(previous.clone());
            }
        }
        // A published workspace may restore this exact checkout after a process dies.
        // Retain it rather than launching detached cleanup that could outlive its file lease.
        if let Some(checkout) = &runtime.checkout {
            checkout.mark_editable();
        }
        let generation = crate::workspaces::id();
        let objects = [
            &snapshot.commits.old_sha,
            &snapshot.commits.new_sha,
            &snapshot.old_sha,
            &snapshot.new_sha,
            &snapshot.head_sha,
        ];
        let pins: Vec<_> = objects
            .iter()
            .enumerate()
            .filter(|(_, s)| {
                [40, 64].contains(&s.len()) && s.bytes().all(|b| b.is_ascii_hexdigit())
            })
            .map(|(i, s)| {
                (
                    format!("refs/diffle/iterations/workspaces/{tab}/{generation}/{i}"),
                    Some((*s).clone()),
                )
            })
            .collect();
        runtime.repo.pin(&pins).await.map_err(|e| e.to_string())?;
        Ok(SavedReview {
            key: handle.key.clone(),
            root: runtime.repo.root.clone(),
            data_root: runtime.repo.data_git_dir.clone(),
            archive: runtime.checkout.as_ref().map(|c| c.archive().to_owned()),
            range_key,
            review,
            source_review,
            request,
            has_iterations: !snapshot.iterations.is_empty(),
            head: snapshot.head_sha.clone(),
            old: snapshot.old_sha.clone(),
            new: snapshot.new_sha.clone(),
            pins: pins.into_iter().map(|(name, _)| name).collect(),
        })
    }
    pub(crate) async fn restore_workspace_review(
        &self,
        saved: &SavedReview,
        pinned: bool,
        owner: &str,
        config_path: PathBuf,
        window: Option<WebviewWindow>,
    ) -> Result<OpenedReview, String> {
        let _opening = self.opening(&saved.key).await;
        // Retire the old cold descriptor; persistence already holds its own object pins.
        let old: Vec<_> = self
            .handles()
            .await
            .into_iter()
            .filter(|h| h.owner == owner && h.key == saved.key)
            .collect();
        for handle in old {
            let id = {
                let handles = self.handles.lock().await;
                handles
                    .iter()
                    .find(|(_, h)| Arc::ptr_eq(h, &handle))
                    .map(|(id, _)| id.clone())
            };
            if let Some(id) = id {
                self.close_owned(&id, owner).await?;
            }
        }
        let slot = self.reserve().await?;
        let (root, checkout) = if let Some(archive) = &saved.archive {
            let lease = crate::storage::restore(archive, &saved.head, Some(&saved.root))
                .await
                .map_err(|e| e.to_string())?;
            (lease.path().to_owned(), Some(lease))
        } else {
            (saved.root.clone(), None)
        };
        let mut repo = GitRepo::open(&root)
            .await
            .map_err(|e| e.to_string())?
            .with_data_root(saved.data_root.clone());
        repo.range_identity = saved.range_key.clone();
        repo.resume_iterations = saved.has_iterations;
        let opened = diffle::cli::repository::Opened {
            repo: Arc::new(repo),
            github: Github::tokenless(),
            token_source: None,
            clone: None,
        };
        let review = if pinned {
            Some(saved.review.clone())
        } else if saved.request.is_some() {
            None
        } else {
            Some(saved.source_review.clone())
        };
        let (mut runtime, _) = build_runtime(
            self.config(config_path).await,
            opened,
            saved.request.clone().unwrap_or(EnterRequest::Working),
            None,
            Arc::new(AtomicBool::new(false)),
            checkout,
            review,
        )
        .await
        .map_err(|e| format!("{e:#}"))?;
        if !pinned {
            if let diffle::protocol::Review::Range { view, .. } = &saved.source_review {
                if !matches!(view, diffle::protocol::RangeView::Whole) {
                    if let Err(e) = runtime.session.show(view.clone()).await {
                        runtime.close().await;
                        return Err(format!("Saved review slice is no longer available: {e}. Refresh source to show the whole range"));
                    }
                }
            }
        }
        if let Some(window) = window {
            attach_events(&mut runtime, window);
        }
        self.insert(owner.into(), saved.key.clone(), runtime, slot)
            .await
    }

    pub(crate) async fn config(&self, path: PathBuf) -> Arc<UserConfigStore> {
        self.config
            .get_or_init(|| async { Arc::new(UserConfigStore::open(path).await) })
            .await
            .clone()
    }
    pub(crate) async fn handles(&self) -> Vec<Arc<ReviewHandle>> {
        self.handles.lock().await.values().cloned().collect()
    }
    async fn handle(&self, id: &str, owner: Option<&str>) -> Result<Arc<ReviewHandle>, String> {
        self.handles
            .lock()
            .await
            .get(id)
            .filter(|h| owner.is_none_or(|o| h.owner == o))
            .cloned()
            .ok_or_else(|| "This review has closed or belongs to another window".into())
    }
    pub(crate) async fn runtime(
        &self,
        id: &str,
    ) -> Result<OwnedMappedMutexGuard<Option<ReviewRuntime>, ReviewRuntime>, String> {
        let handle = self.handle(id, None).await?;
        OwnedMutexGuard::try_map(handle.live.clone().lock_owned().await, |r| r.as_mut()).map_err(
            |_| "This review is cold or has closed. Reopen it before sending requests.".into(),
        )
    }
    pub(crate) async fn owned_runtime(
        &self,
        id: &str,
        owner: &str,
    ) -> Result<OwnedMappedMutexGuard<Option<ReviewRuntime>, ReviewRuntime>, String> {
        self.handle(id, Some(owner)).await?;
        self.runtime(id).await
    }
    pub(crate) async fn owns(&self, id: &str, owner: &str) -> Result<(), String> {
        self.handle(id, Some(owner)).await.map(|_| ())
    }
    async fn opening(&self, key: &str) -> tokio::sync::OwnedMutexGuard<()> {
        let lock = {
            let mut locks = self.openers.lock().await;
            locks.retain(|_, v| v.strong_count() > 0);
            match locks.get(key).and_then(Weak::upgrade) {
                Some(lock) => lock,
                None => {
                    let lock = Arc::new(Mutex::new(()));
                    locks.insert(key.into(), Arc::downgrade(&lock));
                    lock
                }
            }
        };
        lock.lock_owned().await
    }
    async fn reconnect_if_needed(
        &self,
        key: &str,
        owner: &str,
        enabled: bool,
    ) -> Result<(), String> {
        let Some(handle) = self
            .handles()
            .await
            .into_iter()
            .find(|h| h.key == key && h.owner == owner)
        else {
            return Ok(());
        };
        let reconnect = {
            let live = handle.live.lock().await;
            live.as_ref().is_some_and(|runtime| {
                key.starts_with("gitlab:") && runtime.gitlab.is_none()
                    || enabled
                        && runtime.gitlab.is_none()
                        && !runtime.github.enabled.load(Ordering::SeqCst)
            })
        };
        if reconnect {
            self.evict(&handle).await?;
        }
        Ok(())
    }
    async fn checkout_for(
        &self,
        path: &std::path::Path,
        owner: &str,
    ) -> Option<Arc<crate::storage::Checkout>> {
        let path = tokio::fs::canonicalize(path).await.ok()?;
        self.handles()
            .await
            .into_iter()
            .find(|h| h.owner == owner && path.starts_with(&h.root))
            .and_then(|h| h.checkout.as_ref().and_then(Weak::upgrade))
    }
    async fn duplicate(&self, key: &str, owner: &str) -> Option<OpenedReview> {
        let handle = self
            .handles()
            .await
            .into_iter()
            .find(|h| h.key == key && h.owner == owner)?;
        let live = handle.live.lock().await;
        let runtime = live.as_ref()?;
        if runtime
            .session
            .snapshotter()
            .ok()?
            .current()
            .await
            .ok()?
            .follows
            == diffle::protocol::Live::Worktree
        {
            runtime.session.reload().await.ok()?;
        }
        let opened = runtime.opened(key);
        drop(live);
        self.focus(&handle).await;
        Some(opened)
    }
    async fn focus(&self, handle: &ReviewHandle) {
        for h in self.handles().await {
            if h.owner == handle.owner {
                h.visible.store(false, Ordering::SeqCst);
            }
        }
        handle.visible.store(true, Ordering::SeqCst);
        handle
            .touched
            .store(self.clock.fetch_add(1, Ordering::SeqCst), Ordering::SeqCst);
    }
    async fn reserve(&self) -> Result<OwnedSemaphorePermit, String> {
        if self.shutting_down.load(Ordering::SeqCst) {
            return Err("Application is closing".into());
        }
        if let Ok(slot) = self.slots.clone().try_acquire_owned() {
            return Ok(slot);
        }
        let mut candidates = self.handles().await;
        candidates.retain(|h| !h.visible.load(Ordering::SeqCst));
        candidates.sort_by_key(|h| h.touched.load(Ordering::SeqCst));
        for handle in candidates {
            self.evict(&handle).await?;
            if let Ok(slot) = self.slots.clone().try_acquire_owned() {
                return Ok(slot);
            }
        }
        Err("Review runtime budget reached. Close or suspend another review before opening this one.".into())
    }
    async fn evict(&self, handle: &ReviewHandle) -> Result<(), String> {
        let mut live = handle.live.lock().await;
        let Some(runtime) = live.as_ref() else {
            return Ok(());
        };
        let (review, range_key, snapshot) = runtime
            .session
            .checkpoint()
            .await
            .map_err(|e| e.to_string())?;
        let mut pins = vec![];
        for (i, sha) in [
            &snapshot.commits.old_sha,
            &snapshot.commits.new_sha,
            &snapshot.old_sha,
            &snapshot.new_sha,
            &snapshot.head_sha,
        ]
        .iter()
        .enumerate()
        {
            if [40, 64].contains(&sha.len()) && sha.bytes().all(|b| b.is_ascii_hexdigit()) {
                pins.push((
                    format!(
                        "refs/diffle/iterations/mdiff-handle-{}/{i}",
                        runtime.repo.review_refs.trim_start_matches("refs/diffle/")
                    ),
                    Some((*sha).clone()),
                ));
            }
        }
        runtime.repo.pin(&pins).await.map_err(|e| e.to_string())?;
        *handle.descriptor.lock().await = Some(ColdReview {
            id: runtime.id.clone(),
            key: handle.key.clone(),
            root: runtime.repo.root.clone(),
            review,
            data_root: runtime.repo.data_git_dir.clone(),
            range_key,
            archive: runtime.checkout.as_ref().map(|c| c.archive().to_owned()),
            has_iterations: !snapshot.iterations.is_empty(),
            checkout_head: snapshot.head_sha.clone(),
            pins: pins.into_iter().map(|(name, _)| name).collect(),
            github_enabled: runtime.github.enabled.load(Ordering::SeqCst),
            state: "cold",
        });
        let runtime = live.take().unwrap();
        drop(live);
        runtime.close().await;
        Ok(())
    }
    async fn resume(
        &self,
        id: &str,
        owner: &str,
        config_path: PathBuf,
        window: Option<WebviewWindow>,
    ) -> Result<OpenedReview, String> {
        let handle = self.handle(id, Some(owner)).await?;
        let _opening = self.opening(&handle.key).await;
        if let Some(opened) = self.duplicate(&handle.key, owner).await {
            return Ok(opened);
        }
        let descriptor = handle
            .descriptor
            .lock()
            .await
            .clone()
            .ok_or("This review has closed")?;
        let slot = self.reserve().await?;
        let (root, checkout) = if let Some(archive) = &descriptor.archive {
            let lease =
                crate::storage::restore(archive, &descriptor.checkout_head, Some(&descriptor.root))
                    .await
                    .map_err(|e| e.to_string())?;
            (lease.path().to_owned(), Some(lease))
        } else {
            (descriptor.root.clone(), None)
        };
        let mut repo = GitRepo::open(&root)
            .await
            .map_err(|e| e.to_string())?
            .with_data_root(descriptor.data_root.clone());
        repo.range_identity = descriptor.range_key.clone();
        repo.resume_iterations = descriptor.has_iterations;
        let opened = diffle::cli::repository::Opened {
            repo: Arc::new(repo),
            github: Github::tokenless(),
            token_source: None,
            clone: None,
        };
        let (mut runtime, _) = build_runtime(
            self.config(config_path).await,
            opened,
            EnterRequest::Working,
            None,
            Arc::new(AtomicBool::new(false)),
            checkout,
            Some(descriptor.review.clone()),
        )
        .await
        .map_err(|e| format!("{e:#}"))?;
        if let Some(window) = window {
            attach_events(&mut runtime, window);
        }
        let result = self
            .insert(owner.into(), handle.key.clone(), runtime, slot)
            .await?;
        Ok(result)
    }
    pub(crate) async fn check_credentials(&self, options: &OpenOptions) -> Result<(), String> {
        let (host, token) = if options.gitlab_url.is_some() {
            (
                format!(
                    "gitlab:{}",
                    options.gitlab_host.as_deref().unwrap_or("gitlab.com")
                ),
                options.gitlab_token.as_deref(),
            )
        } else {
            (
                format!(
                    "github:{}",
                    options.token_host.as_deref().unwrap_or("github.com")
                ),
                options.token.as_deref(),
            )
        };
        let stamp = crate::storage::digest(token.unwrap_or("auto"));
        let changed = {
            let mut credentials = self.credentials.lock().await;
            credentials
                .insert(host.clone(), stamp.clone())
                .map_or(token.is_some(), |old| old != stamp)
        };
        if !changed {
            return Ok(());
        }
        for handle in self.handles().await {
            let affected = {
                let mut live = handle.live.lock().await;
                if let Some(runtime) = live.as_mut() {
                    let same = runtime.gitlab.as_ref().map_or_else(
                        || host == format!("github:{}", runtime.github.host()),
                        |gl| host == format!("gitlab:{}", gl.origin.authority()),
                    );
                    if same {
                        runtime.github.invalidate(false);
                        runtime.session.invalidate_github();
                    }
                    same
                } else {
                    false
                }
            };
            if affected {
                self.evict(&handle).await?;
            }
        }
        Ok(())
    }
    pub(crate) async fn settings_changed(&self, enabled: bool, auth_changed: bool) {
        for handle in self.handles().await {
            if let Some(runtime) = handle.live.lock().await.as_mut() {
                if runtime
                    .trusted_commands
                    .as_ref()
                    .is_some_and(|commands| *commands != runtime.config.get().lsp_commands)
                {
                    runtime.disable_lsp().await;
                }
                if !auth_changed {
                    continue;
                }
                let current_hosts = runtime.config.get().github_hosts;
                let same_hosts = runtime.configured_hosts == current_hosts;
                runtime.configured_hosts = current_hosts;
                runtime
                    .github
                    .invalidate(enabled && same_hosts && runtime.gitlab.is_none());
                runtime.session.invalidate_github();
                use diffle::session::Broadcaster;
                runtime
                    .hub
                    .broadcast(diffle::protocol::ServerMessage::Config {
                        auth_changed: Some(true),
                    });
            }
        }
    }
    pub async fn close(&self) {
        self.shutting_down.store(true, Ordering::SeqCst);
        let handles = std::mem::take(&mut *self.handles.lock().await);
        for handle in handles.into_values() {
            close_handle(handle).await;
        }
    }
    async fn trust(&self, id: &str, owner: &str) -> Result<diffle::protocol::LspStatus, String> {
        let mut runtime = self.owned_runtime(id, owner).await?;
        if runtime.lsp.is_none() {
            runtime.lsp_slot=Some(self.lsp_slots.clone().try_acquire_owned().map_err(|_| "Code intelligence budget reached. Close the other trusted review first; its trust cannot be reused.")?);
        }
        match runtime.enable_lsp().await {
            Ok(status) => {
                use diffle::session::Broadcaster;
                runtime.hub.broadcast(diffle::protocol::ServerMessage::Lsp {
                    payload: status.clone(),
                });
                Ok(status)
            }
            Err(e) => {
                runtime.lsp_slot = None;
                Err(format!("{e:#}"))
            }
        }
    }
    pub(crate) async fn close_owner(&self, owner: &str) {
        self.closed_windows.lock().await.insert(owner.into());
        self.release_owner(owner).await;
    }
    pub(crate) async fn release_owner(&self, owner: &str) {
        let handles: Vec<_> = {
            let mut handles = self.handles.lock().await;
            let ids: Vec<_> = handles
                .iter()
                .filter(|(_, h)| h.owner == owner)
                .map(|(id, _)| id.clone())
                .collect();
            ids.into_iter()
                .filter_map(|id| handles.remove(&id))
                .collect()
        };
        for handle in handles {
            close_handle(handle).await;
        }
    }
    async fn close_owned(&self, id: &str, owner: &str) -> Result<(), String> {
        let handle = {
            let mut handles = self.handles.lock().await;
            if handles.get(id).is_some_and(|h| h.owner != owner) {
                return Err("This review belongs to another window".into());
            }
            handles.remove(id)
        };
        if let Some(handle) = handle {
            tokio::spawn(close_handle(handle))
                .await
                .map_err(|e| e.to_string())?;
        }
        Ok(())
    }
    async fn insert(
        &self,
        owner: String,
        key: String,
        mut runtime: ReviewRuntime,
        slot: OwnedSemaphorePermit,
    ) -> Result<OpenedReview, String> {
        if self.shutting_down.load(Ordering::SeqCst)
            || self.closed_windows.lock().await.contains(&owner)
        {
            runtime.close().await;
            return Err("Owner window has closed".into());
        }
        runtime.slot = Some(slot);
        let opened = runtime.opened(&key);
        let handle = Arc::new(ReviewHandle {
            root: runtime.repo.root.clone(),
            checkout: runtime.checkout.as_ref().map(Arc::downgrade),
            owner,
            key,
            live: Arc::new(Mutex::new(Some(runtime))),
            visible: AtomicBool::new(false),
            touched: AtomicU64::new(0),
            descriptor: Mutex::new(None),
        });
        let mut handles = self.handles.lock().await;
        if self.shutting_down.load(Ordering::SeqCst)
            || self.closed_windows.lock().await.contains(&handle.owner)
            || handles.len() >= MAX_DESCRIPTORS
                && !handles
                    .values()
                    .any(|h| h.key == handle.key && h.owner == handle.owner)
        {
            drop(handles);
            if let Some(runtime) = handle.live.lock().await.take() {
                runtime.close().await;
            }
            return Err("Review descriptor limit reached. Close an existing review first.".into());
        }
        let replaced: Vec<_> = handles
            .iter()
            .filter(|(_, h)| h.key == handle.key && h.owner == handle.owner)
            .map(|(id, _)| id.clone())
            .collect();
        let replaced: Vec<_> = replaced
            .into_iter()
            .filter_map(|id| handles.remove(&id))
            .collect();
        handles.insert(opened.id.clone(), handle.clone());
        drop(handles);
        for old in replaced {
            close_handle(old).await;
        }
        self.focus(&handle).await;
        Ok(opened)
    }
}
impl ReviewRuntime {
    fn opened(&self, key: &str) -> OpenedReview {
        OpenedReview {
            id: self.id.clone(),
            root: self.repo.root.to_string_lossy().into_owned(),
            github_enabled: self.github.enabled.load(Ordering::SeqCst),
            key: key.into(),
        }
    }
}
#[derive(Serialize)]
pub struct OpenedReview {
    pub id: String,
    root: String,
    github_enabled: bool,
    key: String,
}

#[derive(Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenOptions {
    #[serde(default)]
    pub(crate) tab_scope: Option<String>,
    pub(crate) path: String,
    pub(crate) request: EnterRequest,
    pub(crate) token: Option<String>,
    pub(crate) token_host: Option<String>,
    pub(crate) gitlab_url: Option<String>,
    pub(crate) gitlab_token: Option<String>,
    pub(crate) gitlab_host: Option<String>,
}
#[tauri::command]
pub async fn open_review(
    app: tauri::AppHandle,
    window: WebviewWindow,
    state: State<'_, Engine>,
    repositories: State<'_, crate::repository::Repositories>,
    mut options: OpenOptions,
) -> Result<OpenedReview, String> {
    crate::windows::authorize(&window)?;
    let sessions = app.state::<crate::workspaces::Sessions>();
    let _work = sessions.gate.read().await;
    let _opening = sessions.opening(window.label()).await;
    app.state::<crate::windows::Windows>()
        .admit(window.label())?;
    let credentials = app
        .state::<crate::settings::MemorySettings>()
        .credentials()
        .await;
    options.token = credentials.token;
    options.token_host = credentials.token_host;
    options.gitlab_token = credentials.gitlab_token;
    options.gitlab_host = credentials.gitlab_host;
    open_owned(app.clone(), window, &state, &repositories, options).await
}

pub(crate) async fn open_owned(
    app: tauri::AppHandle,
    window: WebviewWindow,
    state: &Engine,
    repositories: &crate::repository::Repositories,
    options: OpenOptions,
) -> Result<OpenedReview, String> {
    state.check_credentials(&options).await?;
    let OpenOptions {
        tab_scope,
        path,
        request,
        token,
        token_host,
        gitlab_url,
        gitlab_token,
        gitlab_host,
    } = options;
    let owner = window.label().to_owned();
    let config_path = app
        .path()
        .app_config_dir()
        .map_err(|e| e.to_string())?
        .join("review.json");
    let config = state.config(config_path.clone()).await;
    let borrowed = if gitlab_url.is_none() {
        match state
            .checkout_for(std::path::Path::new(&path), &owner)
            .await
        {
            Some(lease) => Some(lease),
            None => {
                repositories
                    .checkout_for(std::path::Path::new(&path), &owner)
                    .await
            }
        }
    } else {
        None
    };
    let key = if let Some(lease) = &borrowed {
        format!(
            "managed:{}:{}",
            lease.archive().display(),
            serde_json::to_string(&request).map_err(|e| e.to_string())?
        )
    } else {
        logical_key(
            &path,
            &request,
            gitlab_url.as_deref(),
            &config.get().github_hosts,
        )
        .await?
    };
    let remote_key = key.clone();
    let key = match tab_scope {
        Some(scope) if scope.len() == 64 && scope.bytes().all(|b| b.is_ascii_hexdigit()) => {
            format!("workspace/{scope}/{key}")
        }
        Some(_) => return Err("Invalid tab identity".into()),
        None => key,
    };
    let _opening = state.opening(&key).await;
    let enabled = crate::github::Connections::load(&config_path)
        .await
        .map_err(|e| e.to_string())?
        .github_enabled;
    state.reconnect_if_needed(&key, &owner, enabled).await?;
    if let Some(opened) = state.duplicate(&key, &owner).await {
        return Ok(opened);
    }
    let slot = state.reserve().await?;

    let data = app
        .path()
        .app_data_dir()
        .map_err(|e| e.to_string())?
        .join("reviews");

    let prepare = async move {
        let gitlab = match gitlab_url {
            Some(url) => Some(Arc::new(
                crate::gitlab::Gitlab::connect(&url, gitlab_token, gitlab_host.as_deref())
                    .await
                    .map_err(|e| e.to_string())?,
            )),
            None => None,
        };
        let (path, checkout) = if let Some(gl) = &gitlab {
            let cache = app
                .path()
                .app_data_dir()
                .map_err(|e| e.to_string())?
                .join("repositories");
            // Legacy checkout is only a preparation/migration source. Every live review gets an isolated worktree.
            let _checkout_lock = preparation_lock(&cache, gl)
                .await
                .map_err(|e| e.to_string())?;
            let legacy = gl.cached_path(&cache);
            if legacy.join(".git").exists()
                && !data
                    .join(crate::storage::digest(&remote_key))
                    .join("migration-v1.json")
                    .exists()
            {
                let source = GitRepo::open(&legacy).await.map_err(|e| e.to_string())?;
                let migrated = crate::storage::adopt(&data, &remote_key, &source)
                    .await
                    .map_err(|e| e.to_string())?;
                drop(migrated);
            }
            let staging = tempfile::Builder::new()
                .prefix("mdiff-mr-")
                .tempdir()
                .map_err(|e| e.to_string())?;
            let path = gl
                .prepare(staging.path().into())
                .await
                .map_err(|e| e.to_string())?;
            let source = GitRepo::open(&path).await.map_err(|e| e.to_string())?;
            let (repo, lease) = crate::storage::adopt(&data, &remote_key, &source)
                .await
                .map_err(|e| e.to_string())?;
            (repo.root, Some(lease))
        } else {
            (PathBuf::from(path), borrowed)
        };
        let request = if let Some(gl) = &gitlab {
            gl.request()
        } else {
            request
        };
        let result = start_with(
            config_path,
            path,
            request,
            token,
            token_host,
            gitlab,
            Some(config),
            Some((data, remote_key)),
            checkout,
        )
        .await
        .map_err(|e| format!("{e:#}"))?;
        let (mut runtime, _) = result;
        attach_events(&mut runtime, window);
        Ok::<_, String>(runtime)
    };
    state.insert(owner, key, prepare.await?, slot).await
}

#[cfg(test)]
async fn start(
    config_path: PathBuf,
    path: PathBuf,
    request: EnterRequest,
    token: Option<String>,
    token_host: Option<String>,
    gitlab: Option<Arc<crate::gitlab::Gitlab>>,
) -> anyhow::Result<(ReviewRuntime, OpenedReview)> {
    start_with(
        config_path,
        path,
        request,
        token,
        token_host,
        gitlab,
        None,
        None,
        None,
    )
    .await
}

#[allow(clippy::too_many_arguments)]
async fn start_with(
    config_path: PathBuf,
    path: PathBuf,
    mut request: EnterRequest,
    token: Option<String>,
    token_host: Option<String>,
    gitlab: Option<Arc<crate::gitlab::Gitlab>>,
    shared: Option<Arc<UserConfigStore>>,
    durable: Option<(PathBuf, String)>,
    checkout: Option<Arc<crate::storage::Checkout>>,
) -> anyhow::Result<(ReviewRuntime, OpenedReview)> {
    let (config, connections) = tokio::join!(
        async {
            match shared {
                Some(config) => config,
                None => Arc::new(UserConfigStore::open(config_path.clone()).await),
            }
        },
        crate::github::Connections::load(&config_path),
    );
    let settings = config.get();
    let github_enabled = Arc::new(std::sync::atomic::AtomicBool::new(
        connections?.github_enabled && gitlab.is_none(),
    ));
    if !github_enabled.load(std::sync::atomic::Ordering::SeqCst)
        && matches!(request, EnterRequest::Pr { .. })
    {
        anyhow::bail!("GitHub integration is disabled. Enable it in Preferences → Connections to open a pull request.");
    }
    let mut opened = if !github_enabled.load(std::sync::atomic::Ordering::SeqCst) {
        // Do not resolve environment credentials or invoke gh when the integration is off.
        diffle::cli::repository::Opened {
            repo: Arc::new(GitRepo::open(&path).await?),
            github: Github::tokenless(),
            token_source: None,
            clone: None,
        }
    } else if let Some(token) = token.filter(|t| !t.trim().is_empty()) {
        let selector = match &request {
            EnterRequest::Pr { pr } => pr.clone(),
            _ => None,
        };
        open_review_repository_with(&mut request, &path, async |local| {
            let instance = choose_instance(&settings.github_hosts, local, selector.as_deref()).await;
            // Never send an explicitly entered token to a different Enterprise host.
            if instance.host != token_host.as_deref().unwrap_or("github.com") {
                return Err(diffle::error::GithubError::conflict("This repository uses a different GitHub host. Update the host in Desktop settings before connecting.").into());
            }
            let client = HttpClient::new(token, &instance.api_base_url())?;
            Ok((Github { instance, client: Some(Arc::new(client)) }, Some("mdiff desktop session".into())))
        }).await?
    } else {
        open_review_repository(&mut request, &path, &settings.github_hosts).await?
    };
    let range_identity = gitlab.as_ref().map(|gl| {
        (
            format!("gitlab:{}{}!{}", gl.origin, gl.project, gl.iid),
            format!(
                "range:{}..{}",
                gl.metadata["diff_refs"]["base_sha"].as_str().unwrap(),
                gl.metadata["diff_refs"]["head_sha"].as_str().unwrap()
            ),
        )
    });
    let mut checkout = checkout;
    if let Some((root, key)) = durable {
        if opened.clone.is_some() {
            let (repo, lease) = crate::storage::adopt(&root, &key, &opened.repo).await?;
            opened.repo = Arc::new(repo);
            opened.clone = None;
            checkout = Some(lease);
        } else if let Some(lease) = &checkout {
            let data_root = lease.data_root();
            let repo = opened
                .repo
                .reopen_at(&opened.repo.root)
                .await?
                .with_data_root(data_root);
            opened.repo = Arc::new(repo);
        }
    }
    if let Some((key, current)) = range_identity {
        let mut repo = opened
            .repo
            .reopen_at(&opened.repo.root)
            .await?
            .with_data_root(opened.repo.data_git_dir.clone());
        repo.range_identity = Some(key.clone());
        repo.resume_iterations = true;
        let repo = Arc::new(repo);
        diffle::comments::CommentStore::migrate_ranges_current(
            &repo.data_git_dir,
            key.clone(),
            current,
        )
        .await?;
        diffle::iterations::IterationStore::new(repo.clone(), key)
            .migrate_ranges()
            .await?;
        opened.repo = repo;
    }
    build_runtime(
        config,
        opened,
        request,
        gitlab,
        github_enabled,
        checkout,
        None,
    )
    .await
}
#[allow(clippy::too_many_arguments)]
async fn build_runtime(
    config: Arc<UserConfigStore>,
    opened: diffle::cli::repository::Opened,
    request: EnterRequest,
    gitlab: Option<Arc<crate::gitlab::Gitlab>>,
    github_enabled: Arc<AtomicBool>,
    checkout: Option<Arc<crate::storage::Checkout>>,
    restored: Option<diffle::protocol::Review>,
) -> anyhow::Result<(ReviewRuntime, OpenedReview)> {
    let settings = config.get();
    let epoch = Arc::new(AtomicU64::new(0));
    let github =
        crate::github::guarded_with_epoch(opened.github, github_enabled.clone(), epoch.clone());
    let integration = crate::github::Integration::with_epoch(
        github.clone(),
        opened.repo.clone(),
        github_enabled,
        epoch,
    );
    let hub = Hub::new();
    let session = Session::new(
        opened.repo.clone(),
        hub.clone(),
        SessionOptions {
            watch: true,
            github,
            context: settings.context_lines.clamp(0, 10000) as u32,
            follow_refs: settings.follow_refs,
        },
    );
    session.history.set_admission(crate::repository::walkers());
    // Every newly opened repository starts without language servers, including
    // downloaded MRs. Only the session-scoped trust action below can start them.
    let pool = None;
    static NEXT_ID: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
    let router = crate::github::router(
        desktop_router(ApiDeps {
            session: session.clone(),
            config: config.clone(),
            extra_auto_viewed: vec![],
            hub: hub.clone(),
            lsp: pool.clone(),
        }),
        &integration,
    );
    let mut runtime = ReviewRuntime {
        config: config.clone(),
        router,
        id: NEXT_ID
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed)
            .to_string(),
        gitlab,
        github: integration,
        events: None,
        settings: None,
        trusted_commands: None,
        configured_hosts: settings.github_hosts.clone(),
        closing: watch::channel(false).0,
        hub,
        session: session.clone(),
        repo: opened.repo,
        lsp: pool.clone(),
        temporary: opened.clone.map(Arc::new),
        checkout,
        slot: None,
        lsp_slot: None,
        closed: false,
    };
    runtime.follow_settings();
    let launch = async {
        let review = match restored {
            Some(review) => review,
            None => session.resolve(request.clone()).await?,
        };
        let fresh = runtime
            .checkout
            .as_ref()
            .is_some_and(|checkout| checkout.take_fresh());
        if fresh {
            if let (
                Some(checkout),
                diffle::protocol::Review::Range {
                    new, pr: Some(_), ..
                },
            ) = (&runtime.checkout, &review)
            {
                let sha = runtime.repo.resolve(new).await?;
                diffle::git::exec::exec_git(
                    &runtime.repo.root,
                    &[
                        "-c",
                        "core.hooksPath=/dev/null",
                        "checkout",
                        "--detach",
                        &sha,
                    ],
                    Default::default(),
                )
                .await?;
                checkout.set_head(sha);
            }
        }
        session.start_requested(review, Some(request)).await?;
        Ok::<_, anyhow::Error>(())
    }
    .await;
    match launch {
        Ok(()) => {
            let root = runtime.repo.root.to_string_lossy().into_owned();
            let id = runtime.id.clone();
            let github_enabled = runtime
                .github
                .enabled
                .load(std::sync::atomic::Ordering::SeqCst);
            Ok((
                runtime,
                OpenedReview {
                    id,
                    root,
                    github_enabled,
                    key: String::new(),
                },
            ))
        }
        Err(e) => {
            runtime.close().await;
            Err(e)
        }
    }
}
fn attach_events(runtime: &mut ReviewRuntime, window: WebviewWindow) {
    let mut messages = runtime.hub.subscribe();
    let id = runtime.id.clone();
    runtime.events = Some(tokio::spawn(async move {
        loop {
            let message = match messages.recv().await {
                Ok(m) => m,
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => "resync".into(),
                Err(_) => break,
            };
            if window
                .emit(
                    "review-update",
                    serde_json::json!({"id":id,"message":message}),
                )
                .is_err()
            {
                break;
            }
        }
    }));
}
async fn logical_key(
    path: &str,
    request: &EnterRequest,
    gitlab: Option<&str>,
    github_hosts: &std::collections::HashMap<String, String>,
) -> Result<String, String> {
    if let Some(input) = gitlab {
        let (origin, project, iid) = crate::gitlab::parse(input).map_err(|e| e.to_string())?;
        return Ok(format!("gitlab:{origin}{project}!{iid}"));
    }
    if let EnterRequest::Pr { pr: Some(pr) } = request {
        if let Ok(mut url) = url::Url::parse(pr) {
            if url.scheme() == "https" {
                url.set_query(None);
                url.set_fragment(None);
                let key = url.as_str().trim_end_matches('/').to_lowercase();
                if let Ok(repo) = GitRepo::open(std::path::Path::new(path)).await {
                    let repository = url
                        .path()
                        .split("/pull/")
                        .next()
                        .unwrap_or_default()
                        .trim_start_matches('/');
                    let instance = choose_instance(github_hosts, Some(&repo), Some(pr)).await;
                    if repo
                        .remotes()
                        .await
                        .map_err(|e| e.to_string())?
                        .iter()
                        .any(|r| {
                            diffle::github::hosts::github_repository(&r.url, &instance)
                                .is_some_and(|name| name.eq_ignore_ascii_case(repository))
                        })
                    {
                        return Ok(format!(
                            "github:{key}:{}:{}",
                            repo.common_dir.display(),
                            repo.git_dir.display()
                        ));
                    }
                }
                return Ok(format!("github:{key}"));
            }
        }
    }
    let repo = diffle::history::Repository::open(std::path::Path::new(path))
        .await
        .map_err(|e| e.to_string())?;
    let info = repo.info().await.map_err(|e| e.to_string())?;
    Ok(format!(
        "local:{}:{}:{}",
        info.common_dir,
        info.git_dir,
        serde_json::to_string(request).map_err(|e| e.to_string())?
    ))
}
async fn preparation_lock(
    cache: &std::path::Path,
    gl: &crate::gitlab::Gitlab,
) -> anyhow::Result<std::fs::File> {
    let path = cache.join(format!(
        "{}.prepare.lock",
        crate::storage::digest(&format!("{}{}!{}", gl.origin, gl.project, gl.iid))
    ));
    tokio::fs::create_dir_all(cache).await?;
    let mut opts = std::fs::OpenOptions::new();
    opts.write(true).create(true).truncate(false);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600);
    }
    let file = opts.open(path)?;
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(180);
    loop {
        match file.try_lock() {
            Ok(()) => return Ok(file),
            Err(std::fs::TryLockError::WouldBlock) => {}
            Err(std::fs::TryLockError::Error(e)) => return Err(e.into()),
        }
        anyhow::ensure!(
            tokio::time::Instant::now() < deadline,
            "Checkout preparation is busy"
        );
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
}
#[tauri::command]
pub async fn close_review(
    state: State<'_, Engine>,
    window: WebviewWindow,
    id: String,
) -> Result<(), String> {
    let sessions = window.app_handle().state::<crate::workspaces::Sessions>();
    let _work = sessions.gate.read().await;

    state.close_owned(&id, window.label()).await
}
#[tauri::command]
pub async fn enable_review_lsp(
    state: State<'_, Engine>,
    window: WebviewWindow,
    id: String,
) -> Result<diffle::protocol::LspStatus, String> {
    state.trust(&id, window.label()).await
}
#[tauri::command]
pub async fn suspend_review(
    state: State<'_, Engine>,
    window: WebviewWindow,
    id: String,
    evict: bool,
) -> Result<(), String> {
    let sessions = window.app_handle().state::<crate::workspaces::Sessions>();
    let _work = sessions.gate.read().await;

    let handle = state.handle(&id, Some(window.label())).await?;
    handle.visible.store(false, Ordering::SeqCst);
    if evict {
        state.evict(&handle).await?;
    }
    Ok(())
}
#[tauri::command]
pub async fn resume_review(
    app: tauri::AppHandle,
    state: State<'_, Engine>,
    window: WebviewWindow,
    id: String,
) -> Result<OpenedReview, String> {
    let sessions = window.app_handle().state::<crate::workspaces::Sessions>();
    let _work = sessions.gate.read().await;

    let owner = window.label().to_owned();
    let config_path = app
        .path()
        .app_config_dir()
        .map_err(|e| e.to_string())?
        .join("review.json");
    state
        .resume(&id, &owner, config_path, Some(window.clone()))
        .await
}
async fn release_descriptor(descriptor: &ColdReview) {
    let at = descriptor.archive.as_ref().unwrap_or(&descriptor.root);
    for name in &descriptor.pins {
        let _ =
            diffle::git::exec::exec_git(at, &["update-ref", "-d", name], Default::default()).await;
    }
}
async fn close_handle(handle: Arc<ReviewHandle>) {
    if let Some(runtime) = handle.live.lock().await.take() {
        runtime.close().await;
    }
    if let Some(descriptor) = handle.descriptor.lock().await.take() {
        release_descriptor(&descriptor).await;
    }
}
#[tauri::command]
pub async fn review_descriptors(
    state: State<'_, Engine>,
    window: WebviewWindow,
) -> Result<Vec<ColdReview>, String> {
    let mut result = vec![];
    for h in state.handles().await {
        if h.owner == window.label() {
            if let Some(d) = h.descriptor.lock().await.clone() {
                result.push(d);
            }
        }
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transport::dispatch;
    #[tokio::test]
    async fn disabled_github_ignores_tokens_and_survives_lsp_router_rebuild_and_reopen() {
        let dir = tempfile::tempdir().unwrap();
        assert!(std::process::Command::new("git")
            .args(["init", "-q"])
            .arg(dir.path())
            .status()
            .unwrap()
            .success());
        std::fs::write(dir.path().join("example.txt"), "local review\n").unwrap();
        let config = dir.path().join("settings/review.json");
        crate::github::Connections {
            github_enabled: false,
        }
        .save(&config)
        .await
        .unwrap();
        let (mut runtime, opened) = start(
            config.clone(),
            dir.path().into(),
            EnterRequest::Working,
            Some("must-not-be-used".into()),
            Some("unrelated.example".into()),
            None,
        )
        .await
        .unwrap();
        assert!(!opened.github_enabled);
        assert!(runtime.lsp.is_none());
        runtime.enable_lsp().await.unwrap();
        assert_eq!(
            dispatch(runtime.router.clone(), "GET", "/api/github", None)
                .await
                .unwrap()
                .status,
            403
        );
        assert_eq!(
            dispatch(runtime.router.clone(), "GET", "/api/snapshot", None)
                .await
                .unwrap()
                .status,
            200
        );
        runtime.close().await;
        let (runtime, opened) = start(
            config.clone(),
            dir.path().into(),
            EnterRequest::Working,
            None,
            None,
            None,
        )
        .await
        .unwrap();
        assert!(!opened.github_enabled);
        runtime.close().await;
        let attempt = start(
            config,
            dir.path().into(),
            EnterRequest::Pr {
                pr: Some("https://github.com/user/project/pull/7".into()),
            },
            None,
            None,
            None,
        )
        .await;
        assert!(
            matches!(attempt, Err(e) if e.to_string().contains("GitHub integration is disabled"))
        );
    }
    #[tokio::test]
    async fn language_servers_require_trust_again_for_each_opened_review() {
        let dir = tempfile::tempdir().unwrap();
        assert!(std::process::Command::new("git")
            .args(["init", "-q"])
            .arg(dir.path())
            .status()
            .unwrap()
            .success());
        std::fs::write(dir.path().join("example.py"), "print('review')\n").unwrap();
        let script = dir.path().join(".git/server.sh");
        let marker = dir.path().join(".git/server-started");
        std::fs::write(
            &script,
            format!("#!/bin/sh\nprintf started > '{}'\n", marker.display()),
        )
        .unwrap();
        let config = dir.path().join(".git/settings.json");
        let store = UserConfigStore::open(config.clone()).await;
        store
            .set_desktop_commands(
                std::collections::HashMap::from([(
                    diffle::protocol::LanguageId::Python,
                    format!("/bin/sh '{}'", script.display()),
                )]),
                Default::default(),
            )
            .await
            .unwrap();
        let (mut runtime, _) = start(
            config.clone(),
            dir.path().into(),
            EnterRequest::Working,
            None,
            None,
            None,
        )
        .await
        .unwrap();
        let status = dispatch(runtime.router.clone(), "GET", "/api/lsp/status", None)
            .await
            .unwrap();
        let status: serde_json::Value = serde_json::from_slice(&status.body).unwrap();
        assert_eq!(status["enabled"], false);
        assert!(!marker.exists());
        assert!(runtime.enable_lsp().await.unwrap().enabled);
        tokio::time::timeout(std::time::Duration::from_secs(5), async {
            while !marker.exists() {
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("trusted language server did not start");
        let pool = runtime.lsp.clone().unwrap();
        runtime.enable_lsp().await.unwrap();
        assert!(Arc::ptr_eq(&pool, runtime.lsp.as_ref().unwrap()));
        runtime.close().await;
        std::fs::remove_file(&marker).unwrap();
        let (runtime, _) = start(
            config,
            dir.path().into(),
            EnterRequest::Working,
            None,
            None,
            None,
        )
        .await
        .unwrap();
        assert!(runtime.lsp.is_none());
        assert!(!marker.exists());
        runtime.close().await;
    }

    #[tokio::test]
    async fn native_review_reads_writes_and_reopens_without_a_server() {
        let dir = tempfile::tempdir().unwrap();
        assert!(std::process::Command::new("git")
            .args(["init", "-q"])
            .arg(dir.path())
            .status()
            .unwrap()
            .success());
        std::fs::write(dir.path().join("example.txt"), "desktop review\n").unwrap();
        let config = dir.path().join("settings/review.json");
        let (runtime, opened) = start(
            config.clone(),
            dir.path().into(),
            EnterRequest::Working,
            None,
            None,
            None,
        )
        .await
        .unwrap();
        let router = runtime.router.clone();
        let snapshot = dispatch(router.clone(), "GET", "/api/snapshot", None)
            .await
            .unwrap();
        let snapshot: serde_json::Value = serde_json::from_slice(&snapshot.body).unwrap();
        assert_eq!(snapshot["changed"][0]["path"], "example.txt");
        let mut events = runtime.hub.subscribe();
        let added = dispatch(
            router.clone(),
            "POST",
            "/api/threads",
            Some(
                serde_json::json!({"path":"example.txt","body":"Native review comment"})
                    .to_string(),
            ),
        )
        .await
        .unwrap();
        assert!(added.status < 300, "{:?}", added.body);
        assert!(
            tokio::time::timeout(std::time::Duration::from_secs(2), events.recv())
                .await
                .is_ok()
        );
        let forbidden = dispatch(
            router.clone(),
            "GET",
            "/api/file?path=../secret&rev=new",
            None,
        )
        .await
        .unwrap();
        assert!(forbidden.status >= 400);
        let prompt = dispatch(
            router.clone(),
            "GET",
            "/api/threads/export?state=open",
            None,
        )
        .await
        .unwrap();
        assert_eq!(prompt.status, 200);
        let prompt = String::from_utf8(prompt.body.to_vec()).unwrap();
        assert!(prompt.contains("Native review comment"));
        assert!(prompt.contains("example.txt"));
        assert!(
            dispatch(router, "GET", "https://evil.invalid/api/snapshot", None)
                .await
                .is_err()
        );
        runtime.close().await;
        let (runtime, next) = start(
            config,
            dir.path().into(),
            EnterRequest::Working,
            None,
            None,
            None,
        )
        .await
        .unwrap();
        assert_ne!(opened.id, next.id);
        let comments = dispatch(runtime.router.clone(), "GET", "/api/threads", None)
            .await
            .unwrap();
        assert!(String::from_utf8_lossy(&comments.body).contains("Native review comment"));
        runtime.close().await;
    }
    #[tokio::test]
    async fn failed_repository_open_preserves_the_active_review_and_its_comments() {
        let dir = tempfile::tempdir().unwrap();
        assert!(std::process::Command::new("git")
            .args(["init", "-q"])
            .arg(dir.path())
            .status()
            .unwrap()
            .success());
        std::fs::write(dir.path().join("example.txt"), "keep this review\n").unwrap();
        let config = dir.path().join(".git/review-settings.json");
        let engine = Engine::default();
        let start_worktree = |path| {
            let config = config.clone();
            async move {
                start(config, path, EnterRequest::Working, None, None, None)
                    .await
                    .map_err(|e| e.to_string())
            }
        };
        let opened = engine
            .insert(
                "main".into(),
                "first".into(),
                start_worktree(dir.path().into()).await.unwrap().0,
                engine.reserve().await.unwrap(),
            )
            .await
            .unwrap();
        let (router, closing) = {
            let runtime = engine.runtime(&opened.id).await.unwrap();
            (runtime.router.clone(), runtime.closing())
        };
        let added = dispatch(
            router.clone(),
            "POST",
            "/api/threads",
            Some(serde_json::json!({"path":"example.txt","body":"Keep this comment"}).to_string()),
        )
        .await
        .unwrap();
        assert_eq!(added.status, 201);
        let (preparing, started) = tokio::sync::oneshot::channel();
        let (read, resume) = tokio::sync::oneshot::channel();
        let (failed, ()) = tokio::join!(
            async {
                preparing.send(()).unwrap();
                resume.await.unwrap();
                start_worktree(dir.path().join("not-a-repository")).await
            },
            async {
                started.await.unwrap();
                let router = {
                    let runtime = tokio::time::timeout(
                        std::time::Duration::from_secs(2),
                        engine.runtime(&opened.id),
                    )
                    .await
                    .expect("preparing a replacement must not lock the active review")
                    .unwrap();
                    runtime.router.clone()
                };
                let response = dispatch(router, "GET", "/api/threads", None).await.unwrap();
                assert_eq!(response.status, 200);
                assert!(String::from_utf8_lossy(&response.body).contains("Keep this comment"));
                read.send(()).unwrap();
            },
        );
        assert!(failed.is_err());
        assert!(engine.runtime(&opened.id).await.is_ok());
        assert!(!*closing.borrow());
        let next = engine
            .insert(
                "main".into(),
                "second".into(),
                start_worktree(dir.path().into()).await.unwrap().0,
                engine.reserve().await.unwrap(),
            )
            .await
            .unwrap();
        assert_ne!(next.id, opened.id);
        assert!(!*closing.borrow());
        assert!(engine.runtime(&opened.id).await.is_ok());
        assert!(engine.runtime(&next.id).await.unwrap().lsp.is_none());
        engine.close().await;
    }
}

#[cfg(test)]
#[path = "desktop_performance.rs"]
mod desktop_performance;

#[cfg(test)]
#[path = "github_live.rs"]
mod github_live;

#[cfg(test)]
#[path = "lifecycle_tests.rs"]
mod lifecycle_tests;
