//! Optional process-owned metadata discovery. No transcript access or external process control.
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
    watches: Vec<Watch>,
    rows: Vec<Activity>,
    cache: HashMap<(i32, String), Option<String>>,
    cursor: usize,
    error: Option<String>,
    generation: u64,
    watch_sequence: u64,
    watch_requests: HashMap<String, (u64, String)>,
}
impl Data {
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
                d.watches
                    .iter()
                    .find(|w| w.context == a.context)
                    .map(|w| (a.clone(), w.owner.clone()))
            })
            .collect()
    }
    pub fn close_owner(&self, owner: &str) {
        let mut d = self.data.lock().unwrap();
        d.watches.retain(|w| w.owner != owner);
        d.watch_requests.remove(owner);
        d.generation += 1;
        self.wake.notify_one();
    }
    fn changed(app: &tauri::AppHandle) {
        let _ = app.emit("agent-activity-changed", ());
    }
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct View {
    preferences: preferences::Preferences,
    activities: Vec<Activity>,
    tools: Vec<Tool>,
    terminals: Vec<serde_json::Value>,
    error: Option<String>,
    hooks: Option<hooks::Bundle>,
}
#[derive(Serialize)]
pub struct Tool {
    id: String,
    path: PathBuf,
}
fn view(app: &tauri::AppHandle, owner: &str) -> Result<View> {
    let state = app.state::<Agents>();
    let d = state.data.lock().unwrap();
    let contexts = d
        .watches
        .iter()
        .filter(|w| w.owner == owner)
        .map(|w| &w.context)
        .collect::<Vec<_>>();
    Ok(View {
        preferences: d.preferences.clone(),
        activities: d
            .rows
            .iter()
            .filter(|a| contexts.contains(&&a.context))
            .cloned()
            .collect(),
        tools: preferences::tools(&d.preferences)
            .into_iter()
            .map(|(id, path)| Tool { id, path })
            .collect(),
        terminals: crate::editors::terminals::agent_terminals()
            .into_iter()
            .map(|t| serde_json::json!({"id":t.id,"name":t.name}))
            .collect(),
        error: d.error.clone(),
        hooks: hooks::bundle(app)
            .ok()
            .filter(|b| b.helper.exists() && (b.claude.exists() || b.codex.exists())),
    })
}
pub fn setup(app: &tauri::AppHandle) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        match preferences::file(&app) {
            Ok(path) => match preferences::load(&path).await {
                Ok(p) => app.state::<Agents>().data.lock().unwrap().preferences = p,
                Err(e) => app.state::<Agents>().data.lock().unwrap().error = Some(e.to_string()),
            },
            Err(e) => app.state::<Agents>().data.lock().unwrap().error = Some(e.to_string()),
        }
        hooks::serve(app.clone());
        loop {
            let active = {
                let s = app.state::<Agents>();
                let d = s.data.lock().unwrap();
                d.preferences.enabled && !d.watches.is_empty()
            };
            if active {
                let sample = app.clone();
                let _ = tokio::task::spawn_blocking(move || sample_processes(&sample)).await;
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
type Samples = (Vec<(process::Process, Option<String>)>, usize);
fn sample_processes(app: &tauri::AppHandle) {
    let state = app.state::<Agents>();
    let (generation, watches, tools, cursor) = {
        let d = state.data.lock().unwrap();
        if !d.preferences.enabled {
            return;
        }
        (
            d.generation,
            d.watches.clone(),
            preferences::tools(&d.preferences),
            d.cursor,
        )
    };
    let result = (|| -> Result<Samples> {
        let pids = process::pids()?;
        let started = std::time::Instant::now();
        let mut found = vec![];
        let mut visited = 0;
        for pid in pids
            .iter()
            .cycle()
            .skip(cursor % pids.len().max(1))
            .take(pids.len().min(2048))
        {
            if started.elapsed() > Duration::from_millis(100) {
                break;
            }
            visited += 1;
            if let Ok(p) = process::inspect(*pid) {
                let key = (p.pid, p.start.clone());
                let known = state.data.lock().unwrap().cache.get(&key).cloned();
                let tool = known.unwrap_or_else(|| process::tool(&p, &tools));
                found.push((p, tool));
            }
        }
        Ok((found, visited))
    })();
    let mut d = state.data.lock().unwrap();
    if generation != d.generation || !d.preferences.enabled {
        return;
    }
    match result {
        Err(error) => d.error = Some(format!("Agent discovery unavailable: {error}")),
        Ok((found, visited)) => {
            d.cursor = d.cursor.wrapping_add(visited.max(1));
            d.cache.clear();
            for (p, tool) in found {
                d.cache.insert((p.pid, p.start.clone()), tool.clone());
                let Some(tool) = tool else {
                    continue;
                };
                let Some(w) = watches
                    .iter()
                    .filter(|w| p.cwd.starts_with(&w.root))
                    .max_by_key(|w| w.root.components().count())
                else {
                    continue;
                };
                let id = crate::storage::digest(&format!("{}:{}:{}", p.pid, p.start, w.context));
                if let Some(old) = d.rows.iter_mut().find(|a| a.id == id) {
                    // Hooks cannot keep an association after the process changes directories.
                    if old.root != w.root {
                        old.session = None;
                        old.parent = None;
                        old.commit = None;
                        old.provenance = "detected".into();
                        old.state = "detected".into();
                    }
                    old.root = w.root.clone();
                    old.context = w.context.clone();
                    old.seen = now();
                } else if d.rows.len() < 256
                    && !d.rows.iter().any(|a| {
                        a.pid == p.pid
                            && a.start == p.start
                            && a.context == w.context
                            && a.provenance == "hook"
                    })
                {
                    d.rows.push(Activity {
                        id,
                        tool,
                        session: None,
                        parent: None,
                        root: w.root.clone(),
                        context: w.context.clone(),
                        state: "detected".into(),
                        provenance: "detected".into(),
                        started: now(),
                        seen: now(),
                        reported: None,
                        sequence: 0,
                        pid: p.pid,
                        start: p.start,
                        branch: None,
                        commit: None,
                        terminal: None,
                    });
                }
            }
            d.rows.retain(|a| {
                now().saturating_sub(a.seen) < 300 && watches.iter().any(|w| w.context == a.context)
            });
            for a in &mut d.rows {
                if process::inspect(a.pid)
                    .map(|p| p.start != a.start || !p.cwd.starts_with(&a.root))
                    .unwrap_or(true)
                {
                    a.state = "unavailable".into();
                    a.commit = None;
                } else if a.provenance == "launch" {
                    a.seen = now();
                } else if a.provenance == "hook"
                    && now().saturating_sub(a.reported.unwrap_or(0)) > 60
                {
                    a.state = "unknown".into();
                }
            }
        }
    }
    drop(d);
    Agents::changed(app);
}
#[tauri::command]
pub async fn agent_view(
    app: tauri::AppHandle,
    window: WebviewWindow,
    context: Option<String>,
) -> Result<View, String> {
    crate::windows::authorize(&window)?;
    // Claim selection before awaiting repository discovery. A slower previous
    // selection must never replace the latest watch (including after close).
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
            let target =
                crate::worktree_tools::tool_target(&app, window.label(), &id, None).await?;
            let state = app.state::<Agents>();
            let new_context = !state
                .data
                .lock()
                .unwrap()
                .watches
                .iter()
                .any(|w| w.owner == window.label() && w.context == id);
            if new_context {
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
    let result = async {
        let p = preferences::save(&preferences::file(&app)?, revision, preferences).await?;
        let s = app.state::<Agents>();
        {
            let mut d = s.data.lock().unwrap();
            d.preferences = p;
            d.cache.clear();
            d.rows.retain(|a| a.terminal.is_some());
            for a in &mut d.rows {
                a.provenance = "launch".into();
                a.state = "launched".into();
                a.session = None;
                a.parent = None;
                a.reported = None;
                a.id = crate::storage::digest(&format!("{}:{}:{}", a.pid, a.start, a.context));
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
) -> Result<(), String> {
    crate::windows::authorize(&window)?;
    let state = app.state::<Agents>();
    if let Some(context) = context {
        let mut d = state.data.lock().unwrap();
        if d.watch_requests
            .get(window.label())
            .is_some_and(|(_, c)| c == &context)
        {
            d.watch_requests.remove(window.label());
        }
        d.watches
            .retain(|w| w.owner != window.label() || w.context != context);
        d.generation += 1;
        state.wake.notify_one();
    } else {
        state.close_owner(window.label());
    }
    Ok(())
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
    let result = async {
        if install {
            hooks::install(&app).await?;
        } else {
            hooks::remove(&app).await?;
        }
        view(&app, window.label())
    }
    .await;
    result.map_err(|e: anyhow::Error| e.to_string())
}

#[cfg(test)]
mod watch_tests {
    use super::*;
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
