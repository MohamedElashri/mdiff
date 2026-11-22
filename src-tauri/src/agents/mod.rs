//! Optional process-owned metadata discovery. No transcript access or external process control.
mod activity;
mod hooks;
pub mod launch;
mod preferences;
mod process;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    path::PathBuf,
    sync::Mutex,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tauri::{Emitter, Manager, WebviewWindow};
pub fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
#[derive(Clone, Serialize, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Activity {
    pub id: String,
    pub tool: String,
    pub session: Option<String>,
    pub parent: Option<String>,
    pub root: PathBuf,
    pub context: String,
    pub state: String,
    pub provenance: String,
    pub started: u64,
    pub seen: u64,
    pub reported: Option<u64>,
    pub sequence: u64,
    pub pid: i32,
    pub start: String,
    pub branch: Option<String>,
    pub commit: Option<String>,
    pub terminal: Option<String>,
}
#[derive(Clone)]
struct Watch {
    owner: String,
    context: String,
    root: PathBuf,
}
#[derive(Default)]
struct Data {
    preferences: preferences::Preferences,
    version: u64,
    watches: Vec<Watch>,
    rows: Vec<Activity>,
    scan: Scan,
    hook_error: Option<String>,
    hook_ready: bool,
    last_hook: Option<u64>,
    cursor: usize,
    error: Option<String>,
    generation: u64,
    watch_sequence: u64,
    watch_requests: HashMap<String, (u64, String)>,
    observers: HashMap<String, String>,
}
impl Data {
    fn release_watch(&mut self, owner: &str, context: Option<&str>, observer: &str) -> bool {
        if self.observers.get(owner).map(String::as_str) != Some(observer) {
            return false;
        }
        if context.is_none()
            || self
                .watch_requests
                .get(owner)
                .is_some_and(|(_, c)| Some(c.as_str()) == context)
        {
            self.watch_requests.remove(owner);
        }
        self.watches
            .retain(|w| w.owner != owner || context.is_some_and(|c| w.context != c));
        self.generation += 1;
        true
    }
    fn disable_reports(&mut self) {
        self.rows.retain(|a| a.parent.is_none());
        for a in &mut self.rows {
            if a.provenance == "hook" {
                a.state = "detected".into();
                a.provenance = if a.terminal.is_some() {
                    "launch"
                } else {
                    "detected"
                }
                .into();
                a.session = None;
                a.parent = None;
                a.reported = None;
                a.sequence = 0;
            }
        }
    }
    fn request_watch(&mut self, owner: &str, context: &str) -> u64 {
        self.watch_sequence = self.watch_sequence.wrapping_add(1);
        self.watch_requests
            .insert(owner.into(), (self.watch_sequence, context.into()));
        self.watch_sequence
    }
    fn current_watch(&self, owner: &str, sequence: u64, context: &str) -> bool {
        self.watch_requests
            .get(owner)
            .is_some_and(|(s, c)| *s == sequence && c == context)
    }
}
#[derive(Default)]
pub struct Agents {
    data: Mutex<Data>,
    wake: tokio::sync::Notify,
    scan_gate: tokio::sync::Mutex<()>,
    preferences_gate: tokio::sync::Mutex<()>,
    initialized: tokio::sync::OnceCell<()>,
}
impl Agents {
    pub(crate) fn observing(&self) -> bool {
        let d = self.data.lock().unwrap();
        d.preferences.enabled && !d.watches.is_empty()
    }
    pub(crate) fn attention(&self) -> Vec<(Activity, String)> {
        let d = self.data.lock().unwrap();
        if !d.preferences.enabled {
            return vec![];
        }
        d.rows
            .iter()
            .filter_map(|a| {
                d.watches.iter().find(|w| w.root == a.root).map(|w| {
                    let mut row = a.clone();
                    row.context = w.context.clone();
                    (row, w.owner.clone())
                })
            })
            .collect()
    }
    pub fn close_owner(&self, owner: &str) {
        let mut d = self.data.lock().unwrap();
        d.watches.retain(|w| w.owner != owner);
        d.watch_requests.remove(owner);
        d.observers.remove(owner);
        d.generation += 1;
        self.wake.notify_one();
    }
    fn changed(app: &tauri::AppHandle) {
        app.state::<Agents>().data.lock().unwrap().version += 1;
        let _ = app.emit("agent-activity-changed", ());
    }
}
#[derive(Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Scan {
    running: bool,
    completed: Option<u64>,
    checked: usize,
    total: usize,
    readable: usize,
    limited: bool,
    error: Option<String>,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct View {
    version: u64,
    preferences: preferences::Preferences,
    activities: Vec<Activity>,
    tools: Vec<preferences::Tool>,
    terminals: Vec<serde_json::Value>,
    error: Option<String>,
    scan: Scan,
    roots: Vec<PathBuf>,
    hook_error: Option<String>,
    hook_ready: bool,
    last_hook: Option<u64>,
    hooks: Option<hooks::Bundle>,
}
fn view(app: &tauri::AppHandle, owner: &str) -> Result<View> {
    let state = app.state::<Agents>();
    let (version, preferences, activities, error, scan, roots, hook_error, hook_ready, last_hook) = {
        let d = state.data.lock().unwrap();
        let watches = d
            .watches
            .iter()
            .filter(|w| w.owner == owner)
            .collect::<Vec<_>>();
        let activities = d
            .rows
            .iter()
            .filter_map(|a| {
                watches.iter().find(|w| w.root == a.root).map(|w| {
                    let mut row = a.clone();
                    row.context = w.context.clone();
                    row
                })
            })
            .collect();
        (
            d.version,
            d.preferences.clone(),
            activities,
            d.error.clone(),
            d.scan.clone(),
            watches.iter().map(|w| w.root.clone()).collect(),
            d.hook_error.clone(),
            d.hook_ready,
            d.last_hook,
        )
    };
    // Filesystem/tool discovery must not hold the activity mutex or block hooks.
    Ok(View {
        version,
        tools: preferences::discover(&preferences),
        preferences,
        activities,
        error,
        scan,
        roots,
        hook_error,
        hook_ready,
        last_hook,
        terminals: crate::editors::terminals::agent_terminals()
            .into_iter()
            .map(|t| serde_json::json!({"id":t.id,"name":t.name}))
            .collect(),
        hooks: hooks::bundle(app)
            .ok()
            .filter(|b| b.helper.exists() && (b.claude.exists() || b.codex.exists())),
    })
}
async fn initialize(app: &tauri::AppHandle) {
    let state = app.state::<Agents>();
    state
        .initialized
        .get_or_init(|| async {
            let result = match preferences::file(app) {
                Ok(path) => preferences::load(&path).await,
                Err(e) => Err(e),
            };
            let mut d = state.data.lock().unwrap();
            match result {
                Ok(p) => d.preferences = p,
                Err(e) => d.error = Some(format!("Could not load agent settings: {e}")),
            }
        })
        .await;
}
pub fn setup(app: &tauri::AppHandle) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        initialize(&app).await;
        hooks::serve(app.clone());
        loop {
            let active = {
                let s = app.state::<Agents>();
                let d = s.data.lock().unwrap();
                d.preferences.enabled && !d.watches.is_empty()
            };
            if active {
                scan(&app, false).await;
                let state = app.state::<Agents>();
                let interval = if state.data.lock().unwrap().rows.iter().any(|a| {
                    !["unavailable", "ended", "detected", "unknown"].contains(&a.state.as_str())
                }) {
                    5
                } else {
                    15
                };
                tokio::select! {_ = tokio::time::sleep(Duration::from_secs(interval))=>(),_=state.wake.notified()=>()}
            } else {
                app.state::<Agents>().wake.notified().await;
            }
        }
    });
}
async fn scan(app: &tauri::AppHandle, manual: bool) {
    let state = app.state::<Agents>();
    // Coalesce overlapping explicit/background scans; every waiter sees the same result.
    let Ok(_scan) = state.scan_gate.try_lock() else {
        let _existing = state.scan_gate.lock().await;
        return;
    };
    let (generation, watches, preferences, cursor, rows) = {
        let mut d = state.data.lock().unwrap();
        if !d.preferences.enabled || d.watches.is_empty() {
            return;
        }
        d.scan.running = true;
        (
            d.generation,
            d.watches.clone(),
            d.preferences.clone(),
            d.cursor,
            d.rows.clone(),
        )
    };
    Agents::changed(app);
    let work = tokio::task::spawn_blocking(move || {
        let tools = preferences::tools(&preferences);
        let mut pids = process::pids()?;
        pids.sort_unstable();
        let total = pids.len();
        let start = std::time::Instant::now();
        let budget = Duration::from_millis(if manual { 2000 } else { 150 });
        let mut found = Vec::new();
        let mut checked = 0;
        for pid in pids
            .iter()
            .cycle()
            .skip(cursor % total.max(1))
            .take(total.min(2048))
        {
            if checked > 0 && start.elapsed() >= budget {
                break;
            }
            checked += 1;
            if let Ok(p) = process::inspect(*pid) {
                found.push(p);
            }
        }
        // Revalidate existing identities even when the discovery pass was partial.
        let current = rows
            .iter()
            .map(|a| (a.id.clone(), process::inspect(a.pid).ok()))
            .collect::<HashMap<_, _>>();
        Ok::<_, anyhow::Error>((found, current, tools, checked, total))
    })
    .await;
    let result = work.map_err(anyhow::Error::from).and_then(|r| r);
    let mut d = state.data.lock().unwrap();
    d.scan.running = false;
    if generation == d.generation && d.preferences.enabled {
        match result {
            Ok((found, current, tools, checked, total)) => {
                let readable = found.len();
                d.cursor = d.cursor.wrapping_add(checked.max(1));
                d.reconcile(&watches, &found, &current, &tools, now());
                d.scan = Scan {
                    running: false,
                    completed: Some(now()),
                    checked,
                    total,
                    readable,
                    limited: checked < total,
                    error: None,
                };
            }
            Err(error) => {
                d.scan.error = Some(format!(
                    "Could not scan processes: {error}. Try scanning again."
                ))
            }
        }
    }
    drop(d);
    Agents::changed(app);
}
#[tauri::command]
pub async fn agent_settings(app: tauri::AppHandle, window: WebviewWindow) -> Result<View, String> {
    crate::windows::authorize(&window)?;
    app.state::<crate::windows::Windows>()
        .admit(window.label())?;
    initialize(&app).await;
    view(&app, window.label()).map_err(|e| e.to_string())
}
#[tauri::command]
pub async fn agent_reload(app: tauri::AppHandle, window: WebviewWindow) -> Result<View, String> {
    crate::windows::authorize(&window)?;
    initialize(&app).await;
    let sessions = app.state::<crate::workspaces::Sessions>();
    let _gate = sessions.gate.read().await;
    app.state::<crate::windows::Windows>()
        .admit(window.label())?;
    let state = app.state::<Agents>();
    let _preferences = state.preferences_gate.lock().await;
    let result = async {
        let p = preferences::load(&preferences::file(&app)?).await?;
        {
            let mut d = state.data.lock().unwrap();
            d.preferences = p;
            if !d.preferences.hooks {
                d.disable_reports();
            }
            d.error = None;
            d.generation += 1;
        }
        state.wake.notify_one();
        Agents::changed(&app);
        view(&app, window.label())
    }
    .await;
    result.map_err(|e: anyhow::Error| e.to_string())
}
#[tauri::command]
pub async fn agent_tools(
    app: tauri::AppHandle,
    window: WebviewWindow,
    preferences: preferences::Preferences,
) -> Result<Vec<preferences::Tool>, String> {
    crate::windows::authorize(&window)?;
    app.state::<crate::windows::Windows>()
        .admit(window.label())?;
    preferences.validate().map_err(|e| e.to_string())?;
    tokio::task::spawn_blocking(move || preferences::discover(&preferences))
        .await
        .map_err(|e| e.to_string())
}
#[tauri::command]
pub async fn agent_scan(app: tauri::AppHandle, window: WebviewWindow) -> Result<View, String> {
    crate::windows::authorize(&window)?;
    initialize(&app).await;
    let sessions = app.state::<crate::workspaces::Sessions>();
    let _gate = sessions.gate.read().await;
    app.state::<crate::windows::Windows>()
        .admit(window.label())?;
    scan(&app, true).await;
    view(&app, window.label()).map_err(|e| e.to_string())
}
#[tauri::command]
pub async fn agent_view(
    app: tauri::AppHandle,
    window: WebviewWindow,
    context: Option<String>,
    observer: String,
) -> Result<View, String> {
    crate::windows::authorize(&window)?;
    initialize(&app).await;
    if observer.is_empty() || observer.len() > 128 {
        return Err("Invalid activity observer".into());
    }
    app.state::<Agents>()
        .data
        .lock()
        .unwrap()
        .observers
        .insert(window.label().into(), observer);
    // Claim selection before awaiting repository discovery. A slower previous
    // selection must never replace the latest watch (including after close).
    if context.is_none() {
        let state = app.state::<Agents>();
        let mut d = state.data.lock().unwrap();
        if d.watches.iter().any(|w| w.owner == window.label()) {
            d.watches.retain(|w| w.owner != window.label());
            d.watch_requests.remove(window.label());
            d.generation += 1;
            d.scan = Scan::default();
        }
    }
    let request = context.as_ref().map(|id| {
        app.state::<Agents>()
            .data
            .lock()
            .unwrap()
            .request_watch(window.label(), id)
    });
    let result = async {
        let sessions = app.state::<crate::workspaces::Sessions>();
        let _gate = sessions.gate.read().await;
        app.state::<crate::windows::Windows>()
            .admit(window.label())
            .map_err(anyhow::Error::msg)?;
        if let Some(id) = context {
            let state = app.state::<Agents>();
            let new_context = !state
                .data
                .lock()
                .unwrap()
                .watches
                .iter()
                .any(|w| w.owner == window.label() && w.context == id);
            if new_context {
                let target =
                    crate::worktree_tools::tool_target(&app, window.label(), &id, None).await?;
                let mut roots = vec![target.root.clone()];
                if let Ok(rows) = diffle::git::worktrees::list(&target.root).await {
                    for row in rows.into_iter().take(16) {
                        if !row.bare
                            && row.prunable.is_none()
                            && !crate::worktree_tools::protected(&app, &row.path)
                        {
                            if let Ok(path) = row.path.canonicalize() {
                                if !roots.contains(&path) {
                                    roots.push(path);
                                }
                            }
                        }
                    }
                }
                let mut d = state.data.lock().unwrap();
                if !d.current_watch(window.label(), request.unwrap(), &id) {
                    drop(d);
                    return view(&app, window.label());
                }
                d.watches.retain(|w| w.owner != window.label());
                for root in roots {
                    if d.watches.len() < 32 {
                        d.watches.push(Watch {
                            owner: window.label().into(),
                            context: id.clone(),
                            root,
                        });
                    }
                }
                d.generation += 1;
                d.scan = Scan::default();
                state.wake.notify_one();
            }
        }
        view(&app, window.label())
    }
    .await;
    result.map_err(|e: anyhow::Error| e.to_string())
}
#[tauri::command]
pub async fn agent_preferences(
    app: tauri::AppHandle,
    window: WebviewWindow,
    revision: u64,
    preferences: preferences::Preferences,
) -> Result<View, String> {
    crate::windows::authorize(&window)?;
    let sessions = app.state::<crate::workspaces::Sessions>();
    let _gate = sessions.gate.read().await;
    app.state::<crate::windows::Windows>()
        .admit(window.label())?;
    initialize(&app).await;
    let agents = app.state::<Agents>();
    let _preferences = agents.preferences_gate.lock().await;
    let result = async {
        let p = preferences::save(&preferences::file(&app)?, revision, preferences).await?;
        let s = app.state::<Agents>();
        {
            let mut d = s.data.lock().unwrap();
            d.preferences = p;
            if !d.preferences.hooks {
                d.disable_reports();
            }
            d.generation += 1;
        }
        s.wake.notify_one();
        Agents::changed(&app);
        view(&app, window.label())
    }
    .await;
    result.map_err(|e: anyhow::Error| e.to_string())
}
#[tauri::command]
pub fn agent_unwatch(
    app: tauri::AppHandle,
    window: WebviewWindow,
    context: Option<String>,
    observer: String,
) -> Result<(), String> {
    crate::windows::authorize(&window)?;
    let state = app.state::<Agents>();
    if state
        .data
        .lock()
        .unwrap()
        .release_watch(window.label(), context.as_deref(), &observer)
    {
        state.wake.notify_one();
    }
    Ok(())
}
#[tauri::command]
pub async fn agent_hook_config(
    app: tauri::AppHandle,
    window: WebviewWindow,
    tool: String,
) -> Result<String, String> {
    crate::windows::authorize(&window)?;
    app.state::<crate::windows::Windows>()
        .admit(window.label())?;
    let b = hooks::bundle(&app).map_err(|e| e.to_string())?;
    let path = match tool.as_str() {
        "codex" => b.codex,
        "claude" => b.claude,
        _ => return Err("Unsupported agent".into()),
    };
    if tokio::fs::metadata(&path)
        .await
        .map_err(|e| e.to_string())?
        .len()
        > 32768
    {
        return Err("Hook configuration exceeds limit".into());
    }
    tokio::fs::read_to_string(path)
        .await
        .map_err(|e| e.to_string())
}
#[tauri::command]
pub async fn agent_hooks(
    app: tauri::AppHandle,
    window: WebviewWindow,
    install: bool,
) -> Result<View, String> {
    crate::windows::authorize(&window)?;
    let sessions = app.state::<crate::workspaces::Sessions>();
    let _gate = sessions.gate.read().await;
    app.state::<crate::windows::Windows>()
        .admit(window.label())?;
    initialize(&app).await;
    let agents = app.state::<Agents>();
    let _preferences = agents.preferences_gate.lock().await;
    let result = async {
        if install {
            hooks::install(&app).await?;
        } else {
            let old = agents.data.lock().unwrap().preferences.clone();
            let revision = old.revision;
            let p = preferences::save(
                &preferences::file(&app)?,
                revision,
                preferences::Preferences {
                    hooks: false,
                    ..old
                },
            )
            .await?;
            {
                let mut d = agents.data.lock().unwrap();
                d.preferences = p;
                d.disable_reports();
                d.generation += 1;
            }
            hooks::remove(&app).await?;
        }
        Agents::changed(&app);
        view(&app, window.label())
    }
    .await;
    result.map_err(|e: anyhow::Error| e.to_string())
}

#[cfg(test)]
mod watch_tests {
    use super::*;
    #[test]
    fn old_observer_cleanup_cannot_remove_a_new_watch_even_for_the_same_context() {
        let mut d = Data::default();
        d.observers.insert("main".into(), "new-observer".into());
        d.watches.push(Watch {
            owner: "main".into(),
            context: "review".into(),
            root: "/repo".into(),
        });
        assert!(!d.release_watch("main", Some("review"), "old-observer"));
        assert_eq!(d.watches.len(), 1);
        assert!(d.release_watch("main", Some("review"), "new-observer"));
        assert!(d.watches.is_empty());
    }
    #[test]
    fn late_checkout_requests_cannot_restore_replaced_or_closed_watches() {
        let mut data = Data::default();
        let old = data.request_watch("window", "old-review");
        let current = data.request_watch("window", "new-review");
        assert!(!data.current_watch("window", old, "old-review"));
        assert!(data.current_watch("window", current, "new-review"));
        let other = data.request_watch("other-window", "old-review");
        assert!(data.current_watch("window", current, "new-review"));
        assert!(data.current_watch("other-window", other, "old-review"));
        data.watch_requests.remove("window");
        assert!(!data.current_watch("window", current, "new-review"));
        let reopened = data.request_watch("window", "new-review");
        assert_ne!(reopened, current);
        assert!(!data.current_watch("window", current, "new-review"));
    }
}
