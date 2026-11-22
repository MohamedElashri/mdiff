//! Process-owned, opt-in summary coordinator. No renderer polling or review startup.
mod model;
mod platform;
mod sources;
#[cfg(test)]
mod tests;
use anyhow::{Context, Result};
use model::*;
use serde::Serialize;
use std::{
    collections::{HashMap, HashSet},
    sync::atomic::{AtomicBool, Ordering},
    time::Duration,
};
use tauri::{Emitter, Listener, Manager, WebviewWindow};
pub(super) fn now() -> u64 {
    crate::agents::now()
}
#[derive(Default, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct Metrics {
    cycles: u64,
    local_reads: u64,
    remote_jobs: u64,
    notifications: u64,
}
#[derive(Default)]
struct Data {
    doc: Document,
    loaded: bool,
    error: Option<String>,
    load_failed: bool,
    revision: u64,
    generation: u64,
    refresh: u64,
    running: Vec<Item>,
    warnings: HashMap<String, String>,
    metrics: Metrics,
    tray_error: Option<String>,
    last_notification: u64,
    force_persist: bool,
}
impl Data {
    fn warnings(&self) -> Vec<String> {
        let mut warnings: Vec<_> = self.warnings.values().cloned().collect();
        if self.doc.limited() {
            warnings.push("Older observations exceed the inbox display budget. Read/dismiss identities are retained within the bounded history; inspect Hosting for full lists.".into());
        }
        warnings.sort();
        warnings
    }
}
#[derive(Default)]
pub struct Attention {
    data: tokio::sync::Mutex<Data>,
    wake: tokio::sync::Notify,
    events: tokio::sync::Notify,
    local_revision: std::sync::atomic::AtomicU64,
    stopped: AtomicBool,
    tray_ready: AtomicBool,
    keep: AtomicBool,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct View {
    revision: u64,
    preferences: Preferences,
    items: Vec<Item>,
    running: Vec<Item>,
    repositories: Vec<sources::Repository>,
    warnings: Vec<String>,
    error: Option<String>,
    tray_error: Option<String>,
    unread: usize,
    metrics: Metrics,
}
#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct Delta {
    revision: u64,
    upsert: Vec<Item>,
    remove: Vec<String>,
    unread: usize,
    running: Vec<Item>,
}
fn file(app: &tauri::AppHandle) -> Result<std::path::PathBuf> {
    Ok(app.path().app_config_dir()?.join("attention-v1.json"))
}
async fn initialize(app: &tauri::AppHandle) {
    let state = app.state::<Attention>();
    let mut d = state.data.lock().await;
    if d.loaded {
        return;
    }
    match file(app) {
        Ok(path) => match model::load(&path).await {
            Ok(doc) => d.doc = doc,
            Err(e) => {
                d.error = Some(e.to_string());
                d.load_failed = true;
            }
        },
        Err(e) => {
            d.error = Some(e.to_string());
            d.load_failed = true;
        }
    }
    d.loaded = true;
}
async fn snapshot(app: &tauri::AppHandle) -> Result<View> {
    initialize(app).await;
    let repositories = sources::inventory(app).await?;
    let state = app.state::<Attention>();
    let d = state.data.lock().await;
    Ok(View {
        revision: d.revision,
        preferences: d.doc.preferences.clone(),
        items: d.doc.items.clone(),
        running: d.running.clone(),
        repositories,
        warnings: d.warnings(),
        error: d.error.clone(),
        tray_error: d.tray_error.clone(),
        unread: d.doc.unread(),
        metrics: d.metrics.clone(),
    })
}
async fn publish(app: &tauri::AppHandle, before: &[Item], running_before: &[Item]) {
    let state = app.state::<Attention>();
    let mut d = state.data.lock().await;
    let changed: Vec<_> = d
        .doc
        .items
        .iter()
        .filter(|i| before.iter().find(|b| b.id == i.id) != Some(i))
        .cloned()
        .collect();
    let remove: Vec<_> = before
        .iter()
        .filter(|b| !d.doc.items.iter().any(|i| i.id == b.id))
        .map(|i| i.id.clone())
        .collect();
    let force_persist = std::mem::take(&mut d.force_persist);
    if !force_persist && changed.is_empty() && remove.is_empty() && running_before == d.running {
        return;
    }
    d.revision += 1;
    d.doc.remember();
    let durable_changed = changed.iter().any(|i| i.source != "agents")
        || before
            .iter()
            .any(|i| i.source != "agents" && remove.contains(&i.id));
    if durable_changed || force_persist {
        let result = async { model::save(&file(app)?, &d.doc).await }.await;
        if let Err(e) = result {
            d.error = Some(format!("Attention history was not saved: {e}"));
        }
    }
    let delta = Delta {
        revision: d.revision,
        upsert: changed,
        remove,
        unread: d.doc.unread(),
        running: d.running.clone(),
    };
    let p = d.doc.preferences.clone();
    let items = d.doc.items.clone();
    let _ = app.emit("attention-changed", delta);
    let result = platform::tray(
        app,
        &p,
        &items,
        items
            .iter()
            .filter(|i| {
                p.source(&i.source) && i.resolved.is_none() && !i.read && !i.dismissed && !i.paused
            })
            .count(),
    );
    state
        .tray_ready
        .store(result.is_ok() && p.tray, Ordering::SeqCst);
    d.tray_error = result
        .err()
        .map(|e| format!("Menu bar/tray unavailable: {e}. Use Needs attention in the app."));
}
async fn apply(
    app: &tauri::AppHandle,
    batch: sources::Batch,
    generation: u64,
    next_running: Option<Vec<Item>>,
) {
    if app.state::<crate::windows::Windows>().exiting() {
        return;
    }
    let state = app.state::<Attention>();
    let mut d = state.data.lock().await;
    if d.generation != generation || !d.doc.preferences.source(batch.source) {
        return;
    }
    let before = d.doc.items.clone();
    let running = d.running.clone();
    if let Some(next) = next_running {
        d.running = next;
    }
    if let Some(warning) = batch.warning {
        if d.warnings.len() < 128 || d.warnings.contains_key(&batch.scope) {
            d.warnings.insert(batch.scope.clone(), warning);
        }
    } else {
        d.warnings.remove(&batch.scope);
    }
    let fresh = d
        .doc
        .reconcile(&batch.scope, batch.items, batch.complete, now());
    let notification = fresh.into_iter().find(|id| {
        d.doc
            .items
            .iter()
            .find(|i| i.id == *id)
            .is_some_and(|i| d.doc.preferences.notify(&i.source))
    });
    let notify = notification.filter(|_| {
        d.metrics.notifications < 16 && now().saturating_sub(d.last_notification) >= 60
    });
    if notify.is_some() {
        d.last_notification = now();
        d.metrics.notifications += 1;
    }
    drop(d);
    publish(app, &before, &running).await;
    if let Some(id) = notify.filter(|_| !app.state::<crate::windows::Windows>().exiting()) {
        if let Err(e) = platform::notify(app, &id) {
            state.data.lock().await.error = Some(format!(
                "Notification unavailable: {e}. The item remains in the inbox."
            ));
        }
    }
}
pub fn setup(app: &tauri::AppHandle) {
    if let Err(e) = platform::install_actions(app) {
        eprintln!("Notification actions unavailable: {e}");
    }
    for event in [
        "agent-activity-changed",
        "git-operations-changed",
        "workspace-state",
        "workspace-windows",
        "workspace-exit-cancelled",
    ] {
        let app = app.clone();
        app.clone().listen(event, move |_| {
            let state = app.state::<Attention>();
            if event != "agent-activity-changed" {
                state.local_revision.fetch_add(1, Ordering::SeqCst);
            }
            state.events.notify_one();
        });
    }
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        initialize(&app).await;
        let (p, items, unread) = {
            let s = app.state::<Attention>();
            let d = s.data.lock().await;
            (
                d.doc.preferences.clone(),
                d.doc.items.clone(),
                d.doc.unread(),
            )
        };
        let result = platform::tray(&app, &p, &items, unread);
        let s = app.state::<Attention>();
        s.tray_ready
            .store(result.is_ok() && p.tray, Ordering::SeqCst);
        s.keep.store(p.keep_in_tray, Ordering::SeqCst);
        s.data.lock().await.tray_error = result
            .err()
            .map(|e| format!("Tray unavailable: {e}. Use the in-app inbox."));
        let mut remote = sources::RemoteState::default();
        let mut locals = Schedule::default();
        let mut refresh = 0;
        let mut local_revision = 0;
        let clock = std::time::Instant::now();
        let mut budget = CycleBudget::default();
        let mut changes = app.state::<crate::hosting::Hosting>().broker.changes();
        loop {
            let state = app.state::<Attention>();
            if state.stopped.load(Ordering::SeqCst) {
                break;
            }
            let (p, doc, generation, next_refresh) = {
                let d = state.data.lock().await;
                (
                    d.doc.preferences.clone(),
                    d.doc.clone(),
                    d.generation,
                    d.refresh,
                )
            };
            if refresh != next_refresh {
                remote.clear();
                locals = Schedule::default();
                refresh = next_refresh;
                budget = CycleBudget::default();
            }
            if !p.enabled
                || !(p.conflicts && !p.local_tabs.is_empty()
                    || (p.requested || p.checks) && !p.repositories.is_empty()
                    || p.agents && app.state::<crate::agents::Agents>().observing())
                || app.state::<crate::windows::Windows>().exiting()
            {
                tokio::select! {_=state.wake.notified()=>(),_=state.events.notified()=>(),_=changes.changed()=>{remote.clear();}}
                continue;
            }
            let mut actual_targets = true;
            let work = async {
                let repos = match sources::inventory(&app).await {
                    Ok(rows) => rows,
                    Err(_) => {
                        let mut d = state.data.lock().await;
                        d.error=Some("Workspace inventory unavailable; monitoring is paused until its metadata can be read.".into());
                        let _=app.emit("attention-status",serde_json::json!({"error":d.error,"warnings":d.warnings(),"metrics":d.metrics}));
                        return;
                    }
                };
                actual_targets = p.conflicts
                    && repos
                        .iter()
                        .any(|r| p.local_tabs.contains(&r.tab) && !r.root.is_empty())
                    || (p.requested || p.checks) && !p.repositories.is_empty()
                    || p.agents && app.state::<crate::agents::Agents>().observing();
                let (before, running_before) = {
                    let mut d = state.data.lock().await;
                    d.metrics.cycles += 1;
                    let before = d.doc.items.clone();
                    let running = d.running.clone();
                    d.doc.pause_unconfigured(
                        &repos
                            .iter()
                            .map(|r| (r.tab.clone(), r.root.clone()))
                            .collect(),
                    );
                    if !p.agents {
                        d.running.clear();
                    }
                    (before, running)
                };
                publish(&app, &before, &running_before).await;
                if p.agents {
                    let (batch, running) = sources::agents(&app, &doc.items);
                    apply(&app, batch, generation, Some(running)).await;
                }
                let version = state.local_revision.load(Ordering::SeqCst);
                if budget.admit(clock.elapsed().as_secs()) {
                    if p.conflicts {
                        let mut unique = HashSet::new();
                        let rows: Vec<_> = repos
                            .iter()
                            .filter(|r| {
                                p.local_tabs.contains(&r.tab)
                                    && !r.root.is_empty()
                                    && unique.insert(r.root.clone())
                            })
                            .collect();
                        let mut jobs: Vec<_> =
                            rows.iter().map(|r| (r.root.clone(), r.active)).collect();
                        jobs.sort_by_key(|(_, active)| !*active);
                        if version != local_revision {
                            locals.expedite(
                                jobs.iter()
                                    .filter(|(_, active)| *active)
                                    .map(|(root, _)| root.clone()),
                            );
                            local_revision = version;
                        }
                        if let Some(root) = locals.next(&jobs, clock.elapsed().as_secs()) {
                            let repo = rows.iter().find(|r| r.root == root).unwrap();
                            state.data.lock().await.metrics.local_reads += 1;
                            let batch = sources::conflicts(repo).await.unwrap_or_else(|_| {
                                sources::Batch::error(
                                    format!("conflict:{}", crate::storage::digest(&repo.root)),
                                    "conflicts",
                                )
                            });
                            apply(&app, batch, generation, None).await;
                        }
                    }
                    if p.requested || p.checks {
                        let epoch = app.state::<crate::hosting::Hosting>().broker.epoch();
                        if let Some(batch) = remote
                            .poll(&app, &p, &doc, &repos, clock.elapsed().as_secs())
                            .await
                        {
                            state.data.lock().await.metrics.remote_jobs += 1;
                            if epoch == app.state::<crate::hosting::Hosting>().broker.epoch() {
                                apply(&app, batch, generation, None).await;
                            }
                        }
                    }
                }
                let d = state.data.lock().await;
                let _=app.emit("attention-status",serde_json::json!({"warnings":d.warnings(),"error":d.error,"metrics":d.metrics}));
            };
            tokio::select! {_=work=>(),_=state.wake.notified()=>{continue;},_=changes.changed()=>{remote.clear();continue;}}
            if !actual_targets {
                tokio::select! {_=state.wake.notified()=>(),_=state.events.notified()=>(),_=changes.changed()=>{remote.clear();}}
                continue;
            }
            tokio::select! {_=tokio::time::sleep(Duration::from_secs(15))=>(),_=state.wake.notified()=>(),_=state.events.notified()=>(),_=changes.changed()=>{remote.clear();}}
        }
    });
}
pub fn hide_on_close(app: &tauri::AppHandle, owner: &str) -> bool {
    let state = app.state::<Attention>();
    if state.keep.load(Ordering::SeqCst)
        && state.tray_ready.load(Ordering::SeqCst)
        && !app.state::<crate::windows::Windows>().exiting()
    {
        if let Some(w) = app.get_webview_window(owner) {
            return w.hide().is_ok();
        }
    }
    false
}
#[cfg(target_os = "macos")]
pub fn reopen(app: &tauri::AppHandle) {
    platform::show(app, None);
    app.state::<Attention>().events.notify_one();
}
pub fn reveal_for_quit(app: &tauri::AppHandle) {
    for w in app.webview_windows().into_values() {
        let _ = w.show();
    }
}
pub fn pause_for_exit(app: &tauri::AppHandle) {
    if let Some(state) = app.try_state::<Attention>() {
        state.wake.notify_one();
    }
}
pub fn shutdown(app: &tauri::AppHandle) {
    let state = app.state::<Attention>();
    state.stopped.store(true, Ordering::SeqCst);
    state.wake.notify_one();
}
#[tauri::command]
pub async fn attention_badge(
    app: tauri::AppHandle,
    window: WebviewWindow,
) -> Result<serde_json::Value, String> {
    crate::windows::authorize(&window)?;
    initialize(&app).await;
    let s = app.state::<Attention>();
    let d = s.data.lock().await;
    Ok(serde_json::json!({"revision":d.revision,"unread":d.doc.unread()}))
}
#[tauri::command]
pub async fn attention_view(app: tauri::AppHandle, window: WebviewWindow) -> Result<View, String> {
    crate::windows::authorize(&window)?;
    snapshot(&app).await.map_err(|e| e.to_string())
}
#[tauri::command]
pub async fn attention_preferences(
    app: tauri::AppHandle,
    window: WebviewWindow,
    mut preferences: Preferences,
) -> Result<View, String> {
    crate::windows::authorize(&window)?;
    let sessions = app.state::<crate::workspaces::Sessions>();
    let _work = sessions.gate.read().await;
    app.state::<crate::windows::Windows>()
        .admit(window.label())?;
    initialize(&app).await;
    preferences.validate().map_err(|e| e.to_string())?;
    let repos = sources::inventory(&app).await.map_err(|e| e.to_string())?;
    preferences
        .local_tabs
        .retain(|id| repos.iter().any(|r| &r.tab == id && !r.root.is_empty()));
    let state = app.state::<Attention>();
    let mut d = state.data.lock().await;
    if d.load_failed {
        return Err(
            "Repair the original attention metadata file before saving; it was preserved".into(),
        );
    }
    if d.doc.preferences.revision != preferences.revision {
        return Err("Inbox preferences changed in another window; reload before saving".into());
    }
    // Persist and platform changes share the same authority; failed saves restore the prior tray.
    let old_doc = d.doc.clone();
    preferences.revision += 1;
    d.doc.preferences = preferences.clone();
    d.doc.pause_unconfigured(
        &repos
            .iter()
            .map(|r| (r.tab.clone(), r.root.clone()))
            .collect(),
    );
    state.tray_ready.store(false, Ordering::SeqCst);
    if let Err(e) = platform::tray(&app, &preferences, &d.doc.items, d.doc.unread()) {
        d.doc = old_doc;
        return Err(format!("Tray unavailable; preference unchanged: {e}"));
    }
    if let Err(e) = model::save(&file(&app).map_err(|e| e.to_string())?, &d.doc).await {
        d.doc = old_doc;
        let restored = platform::tray(&app, &d.doc.preferences, &d.doc.items, d.doc.unread());
        state
            .tray_ready
            .store(restored.is_ok() && d.doc.preferences.tray, Ordering::SeqCst);
        return Err(e.to_string());
    }
    if !preferences.agents || !preferences.enabled {
        d.running.clear();
    }
    d.generation += 1;
    d.refresh += 1;
    d.revision += 1;
    state.keep.store(preferences.keep_in_tray, Ordering::SeqCst);
    state.tray_ready.store(preferences.tray, Ordering::SeqCst);
    drop(d);
    state.wake.notify_one();
    let _ = app.emit("attention-settings", ());
    snapshot(&app).await.map_err(|e| e.to_string())
}
#[tauri::command]
pub async fn attention_update(
    app: tauri::AppHandle,
    window: WebviewWindow,
    id: Option<String>,
    action: String,
) -> Result<View, String> {
    crate::windows::authorize(&window)?;
    let sessions = app.state::<crate::workspaces::Sessions>();
    let _work = sessions.gate.read().await;
    app.state::<crate::windows::Windows>()
        .admit(window.label())?;
    initialize(&app).await;
    let state = app.state::<Attention>();
    let mut d = state.data.lock().await;
    if d.load_failed {
        return Err("Original metadata was preserved; repair it before updating".into());
    }
    let before = d.doc.items.clone();
    let running = d.running.clone();
    match action.as_str() {
        "refresh" => {
            d.refresh += 1;
            d.generation += 1;
        }
        "readAll" => {
            d.doc.mark_all_read();
            d.force_persist = true;
        }
        "read" | "dismiss" | "restore" => {
            let item = d
                .doc
                .items
                .iter_mut()
                .find(|i| Some(&i.id) == id.as_ref())
                .ok_or("Attention item no longer exists")?;
            match action.as_str() {
                "read" => item.read = true,
                "dismiss" => {
                    item.dismissed = true;
                    item.read = true;
                }
                _ => item.dismissed = false,
            }
        }
        _ => return Err("Unknown inbox action".into()),
    }
    drop(d);
    publish(&app, &before, &running).await;
    if action == "refresh" {
        state.wake.notify_one();
    }
    snapshot(&app).await.map_err(|e| e.to_string())
}
pub(super) async fn route(app: &tauri::AppHandle, origin: Option<&str>, id: &str) -> Result<()> {
    let state = app.state::<Attention>();
    let d = state.data.lock().await;
    let item = d
        .doc
        .items
        .iter()
        .chain(d.running.iter())
        .find(|i| i.id == id)
        .context("Attention item expired")?;
    anyhow::ensure!(
        d.doc.preferences.source(&item.source),
        "This attention source is paused"
    );
    anyhow::ensure!(
        !item.paused,
        "This destination is no longer configured for monitoring"
    );
    let mut target = item.target.clone();
    drop(d);
    if let Some(tab) = &target.tab {
        let doc = crate::workspaces::attention_document(app)
            .await?
            .context("Workspace metadata unavailable")?;
        let workspace = doc
            .workspaces
            .iter()
            .find(|w| w.tabs.iter().any(|t| &t.id == tab))
            .context("Destination tab was removed")?;
        target.workspace = Some(workspace.id.clone());
        if let Some(expected) = &target.root {
            let tab = workspace.tabs.iter().find(|t| &t.id == tab).unwrap();
            let actual = tab
                .saved
                .as_ref()
                .map(|s| s.root.clone())
                .unwrap_or_else(|| tab.source.path.clone().into());
            anyhow::ensure!(
                actual.canonicalize()? == std::path::Path::new(expected).canonicalize()?,
                "Destination checkout changed; refresh the inbox before opening this item"
            );
        }
        target.owner = app.state::<crate::windows::Windows>().owner(&workspace.id);
    }
    if target.kind == "agents" {
        anyhow::ensure!(
            app.state::<crate::agents::Agents>()
                .attention()
                .iter()
                .any(
                    |(a, o)| a.context == target.context.as_deref().unwrap_or("")
                        && Some(o) == target.owner.as_ref()
                        && a.terminal == target.terminal
                ),
            "Agent session is no longer available"
        );
    }
    if let Some(r) = &target.review {
        r.validate()?;
    }
    let owner = target
        .owner
        .clone()
        .or_else(|| origin.map(str::to_owned))
        .or_else(|| app.webview_windows().keys().next().cloned())
        .context("No inbox window available")?;
    app.state::<crate::windows::Windows>()
        .admit(&owner)
        .map_err(anyhow::Error::msg)?;
    platform::show(app, Some(&owner));
    app.emit_to(&owner, "attention-route", target)?;
    Ok(())
}
#[tauri::command]
pub async fn attention_open(
    app: tauri::AppHandle,
    window: WebviewWindow,
    id: String,
) -> Result<(), String> {
    crate::windows::authorize(&window)?;
    route(&app, Some(window.label()), &id)
        .await
        .map_err(|e| e.to_string())
}
