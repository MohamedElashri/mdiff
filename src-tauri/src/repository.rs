static NEXT_ID: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
// Lightweight, owner-scoped history contexts independent of review trust/transport.
use crate::{
    engine::Engine,
    transport::{dispatch, pack, Requests},
};
use diffle::history::Repository;
use std::{
    collections::{HashMap, HashSet},
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Weak,
    },
};
use tauri::{Manager, State};
use tokio::sync::{watch, Mutex};
pub(crate) fn walkers() -> Arc<tokio::sync::Semaphore> {
    static LIMIT: std::sync::LazyLock<Arc<tokio::sync::Semaphore>> =
        std::sync::LazyLock::new(|| Arc::new(tokio::sync::Semaphore::new(2)));
    LIMIT.clone()
}
#[derive(Default)]
pub struct Repositories {
    active: Mutex<HashMap<String, Context>>,
    opening: Mutex<HashMap<String, Weak<Mutex<()>>>>,
    closed_owners: Mutex<HashSet<String>>,
    shutting_down: AtomicBool,
}
struct Context {
    id: String,
    repository: Arc<Repository>,
    router: axum::Router,
    closing: watch::Sender<bool>,
    _checkout: Option<Arc<tempfile::TempDir>>,
    _owned: Option<Arc<crate::storage::Checkout>>,
    owner: String,
    root: PathBuf,
}
impl Context {
    async fn close(self) {
        self.closing.send_replace(true);
        self.repository.close().await;
    }
}
impl Repositories {
    pub(crate) async fn uses_checkout(&self, root: &std::path::Path) -> bool {
        self.active
            .lock()
            .await
            .values()
            .any(|context| context.root == root)
    }

    pub(crate) async fn tool_target(
        &self,
        id: &str,
        owner: &str,
    ) -> Result<crate::worktree_tools::Target, String> {
        let contexts = self.active.lock().await;
        let context = contexts
            .get(owner)
            .filter(|context| context.id == id)
            .ok_or("Repository context has closed or belongs to another window")?;
        Ok(crate::worktree_tools::Target {
            root: context.root.clone(),
            owned: context._owned.clone(),
            temporary: context._checkout.clone(),
            closing: context.closing.subscribe(),
        })
    }
    async fn opening(&self, owner: &str) -> tokio::sync::OwnedMutexGuard<()> {
        let lock = {
            let mut locks = self.opening.lock().await;
            locks.retain(|_, v| v.strong_count() > 0);
            if let Some(lock) = locks.get(owner).and_then(Weak::upgrade) {
                lock
            } else {
                let lock = Arc::new(Mutex::new(()));
                locks.insert(owner.into(), Arc::downgrade(&lock));
                lock
            }
        };
        lock.lock_owned().await
    }
    pub(crate) async fn owns(&self, id: &str, owner: &str) -> Result<(), String> {
        if self
            .active
            .lock()
            .await
            .get(owner)
            .is_some_and(|c| c.id == id)
        {
            Ok(())
        } else {
            Err("Repository context has closed or belongs to another window".into())
        }
    }
    pub(crate) async fn checkout_for(
        &self,
        path: &std::path::Path,
        owner: &str,
    ) -> Option<Arc<crate::storage::Checkout>> {
        let path = tokio::fs::canonicalize(path).await.ok()?;
        self.active
            .lock()
            .await
            .get(owner)
            .filter(|context| path.starts_with(&context.root))
            .and_then(|context| context._owned.clone())
    }
    pub(crate) async fn close_owner(&self, owner: &str) {
        self.closed_owners.lock().await.insert(owner.into());
        self.release_owner(owner).await;
    }
    pub(crate) async fn release_owner(&self, owner: &str) {
        let _opening = self.opening(owner).await;
        let context = self.active.lock().await.remove(owner);
        if let Some(context) = context {
            context.close().await;
        }
    }
    pub async fn close(&self) {
        self.shutting_down.store(true, Ordering::SeqCst);
        let active = std::mem::take(&mut *self.active.lock().await);
        for context in active.into_values() {
            context.close().await;
        }
    }
}
#[derive(serde::Serialize)]
pub struct Opened {
    id: String,
    info: diffle::history::Info,
}
#[tauri::command]
pub async fn open_repository_context(
    window: tauri::WebviewWindow,
    state: State<'_, Repositories>,
    engine: State<'_, Engine>,
    path: String,
    review: Option<String>,
) -> Result<Opened, String> {
    crate::windows::authorize(&window)?;
    window
        .app_handle()
        .state::<crate::windows::Windows>()
        .writable()?;
    let sessions = window.app_handle().state::<crate::workspaces::Sessions>();
    let _work = sessions.gate.read().await;
    window
        .app_handle()
        .state::<crate::windows::Windows>()
        .admit(window.label())?;
    let _opening = state.opening(window.label()).await;
    let (path, checkout, owned) = if let Some(id) = review {
        let runtime = engine.owned_runtime(&id, window.label()).await?;
        (
            runtime.repository_root().to_owned(),
            runtime.temporary_checkout(),
            runtime.owned_checkout(),
        )
    } else {
        (PathBuf::from(path), None, None)
    };
    let repository = Repository::open(&path).await.map_err(|e| e.to_string())?;
    repository.set_admission(walkers());
    let info = repository.info().await.map_err(|e| e.to_string())?;
    let id = format!(
        "history-{}",
        NEXT_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    );
    let router = diffle::server::repository::router(repository.clone());
    let (closing, _) = watch::channel(false);
    let context = Context {
        id: id.clone(),
        repository,
        router,
        closing,
        _checkout: checkout,
        _owned: owned,
        owner: window.label().into(),
        root: PathBuf::from(&info.root),
    };
    let mut contexts = state.active.lock().await;
    if state.shutting_down.load(Ordering::SeqCst)
        || state.closed_owners.lock().await.contains(window.label())
    {
        drop(contexts);
        context.close().await;
        return Err("Owner window has closed".into());
    }
    if contexts.len() >= 2 && !contexts.contains_key(window.label()) {
        drop(contexts);
        context.close().await;
        return Err("Repository context budget reached".into());
    }
    let previous = contexts.insert(window.label().into(), context);
    drop(contexts);
    if let Some(previous) = previous {
        previous.close().await;
    }
    Ok(Opened { id, info })
}
#[tauri::command]
pub async fn close_repository_context(
    window: tauri::WebviewWindow,
    state: State<'_, Repositories>,
    id: String,
) -> Result<(), String> {
    let sessions = window.app_handle().state::<crate::workspaces::Sessions>();
    let _work = sessions.gate.read().await;

    let mut active = state.active.lock().await;
    let old = if active.get(window.label()).is_some_and(|c| c.id == id) {
        active.remove(window.label())
    } else {
        None
    };
    drop(active);
    if let Some(old) = old {
        old.close().await;
    }
    Ok(())
}
#[tauri::command]
#[allow(clippy::too_many_arguments)] // Window/state are injected; keep the request wire shape stable.
pub async fn repository_request(
    window: tauri::WebviewWindow,
    state: State<'_, Repositories>,
    requests: State<'_, Requests>,
    id: String,
    request_id: String,
    method: String,
    path: String,
    body: Option<String>,
) -> Result<tauri::ipc::Response, String> {
    if !path.starts_with("/api/history/") {
        return Err("Invalid repository operation".into());
    }
    let (router, mut closing) = {
        let active = state.active.lock().await;
        let context = active
            .get(window.label())
            .filter(|c| c.id == id && c.owner == window.label())
            .ok_or("Repository context has closed")?;
        (context.router.clone(), context.closing.subscribe())
    };
    let (_guard, cancelled) = requests.start((id, request_id))?;
    let reply = tokio::select! {biased;_=closing.wait_for(|closed|*closed)=>return Err("Repository context has closed".into()),_=cancelled=>return Err("Request cancelled".into()),reply=dispatch(router,&method,&path,body)=>reply?};
    Ok(tauri::ipc::Response::new(pack(reply)?))
}
#[cfg(test)]
mod tool_target_tests {
    use super::*;
    #[tokio::test]
    async fn external_tool_targets_remain_owner_scoped_and_follow_context_teardown() {
        let dir = tempfile::tempdir().unwrap();
        assert!(std::process::Command::new("git")
            .args(["init", "-q"])
            .arg(dir.path())
            .status()
            .unwrap()
            .success());
        let repository = Repository::open(dir.path()).await.unwrap();
        let (closing, _) = watch::channel(false);
        let state = Repositories::default();
        state.active.lock().await.insert(
            "main".into(),
            Context {
                id: "history-owned".into(),
                repository,
                router: axum::Router::new(),
                closing,
                _checkout: None,
                _owned: None,
                owner: "main".into(),
                root: dir.path().into(),
            },
        );
        assert!(state.tool_target("history-owned", "other").await.is_err());
        assert!(state.tool_target("history-other", "main").await.is_err());
        let target = state.tool_target("history-owned", "main").await.unwrap();
        assert!(!*target.closing.borrow());
        state.release_owner("main").await;
        assert!(*target.closing.borrow());
        assert!(state.tool_target("history-owned", "main").await.is_err());
    }
}
