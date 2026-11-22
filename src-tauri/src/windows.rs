//! Native admission, workspace ownership and the all-window exit barrier.
#[cfg(target_os = "macos")]
pub mod macos;
use serde::Serialize;
use std::{
    collections::{BTreeMap, HashSet},
    sync::Mutex,
};
use tauri::{Emitter, Manager, WebviewWindow};
pub const MAX_WINDOWS: usize = 2;
#[derive(Default)]
struct Window {
    workspace: Option<String>,
    registered: bool,
    close_ready: bool,
    pending_close: Option<u64>,
}
struct Exit {
    generation: u64,
    action: String,
    pending: HashSet<String>,
}
struct Registry {
    windows: BTreeMap<String, Window>,
    exit: Option<Exit>,
    generation: u64,
    update_owner: Option<String>,
}
pub struct Windows(Mutex<Registry>, std::sync::atomic::AtomicBool);
impl Default for Windows {
    fn default() -> Self {
        Self(
            Mutex::new(Registry {
                windows: BTreeMap::from([("main".into(), Window::default())]),
                exit: None,
                generation: 0,
                update_owner: None,
            }),
            std::sync::atomic::AtomicBool::new(false),
        )
    }
}
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WindowInfo {
    pub label: String,
    pub workspace: Option<String>,
    pub current: bool,
}
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Flush {
    owner: String,
    action: String,
    generation: Option<u64>,
}
impl Windows {
    pub fn authorize(&self, owner: &str) -> Result<(), String> {
        if self.0.lock().unwrap().windows.contains_key(owner) {
            Ok(())
        } else {
            Err("Window is not admitted".into())
        }
    }
    pub fn writable(&self) -> Result<(), String> {
        if self.0.lock().unwrap().exit.is_some() {
            Err("The app is saving and exiting".into())
        } else {
            Ok(())
        }
    }
    pub fn admit(&self, owner: &str) -> Result<(), String> {
        let registry = self.0.lock().unwrap();
        let window = registry
            .windows
            .get(owner)
            .ok_or("Window is not admitted")?;
        if registry.exit.is_some() || window.close_ready || window.pending_close.is_some() {
            return Err("Window is saving or closing".into());
        }
        Ok(())
    }
    pub fn allow_save(&self, owner: &str) -> Result<(), String> {
        let registry = self.0.lock().unwrap();
        let window = registry
            .windows
            .get(owner)
            .ok_or("Window is not admitted")?;
        if window.close_ready
            || registry
                .exit
                .as_ref()
                .is_some_and(|e| !e.pending.contains(owner))
        {
            return Err("Window has already finished saving".into());
        }
        Ok(())
    }
    pub fn list(&self, owner: &str) -> Vec<WindowInfo> {
        self.0
            .lock()
            .unwrap()
            .windows
            .iter()
            .map(|(label, w)| WindowInfo {
                label: label.clone(),
                workspace: w.workspace.clone(),
                current: label == owner,
            })
            .collect()
    }
    pub fn owner(&self, workspace: &str) -> Option<String> {
        self.0
            .lock()
            .unwrap()
            .windows
            .iter()
            .find(|(_, w)| w.workspace.as_deref() == Some(workspace))
            .map(|(l, _)| l.clone())
    }
    pub fn current(
        &self,
        owner: &str,
        fallback: &str,
        available: &[String],
    ) -> Result<String, String> {
        let mut r = self.0.lock().unwrap();
        let current = r
            .windows
            .get(owner)
            .ok_or("Window is not admitted")?
            .workspace
            .clone();
        if let Some(current) = current.filter(|w| available.contains(w)) {
            return Ok(current);
        }
        let free = |id: &String| {
            !r.windows
                .iter()
                .any(|(l, w)| l != owner && w.workspace.as_ref() == Some(id))
        };
        let selected = available
            .iter()
            .find(|id| id.as_str() == fallback && free(id))
            .or_else(|| available.iter().find(|id| free(id)))
            .ok_or("All workspaces are open in another window")?
            .clone();
        r.windows.get_mut(owner).unwrap().workspace = Some(selected.clone());
        Ok(selected)
    }
    pub fn select(&self, owner: &str, workspace: &str) -> Result<(), String> {
        let mut r = self.0.lock().unwrap();
        if r.windows
            .iter()
            .any(|(l, w)| l != owner && w.workspace.as_deref() == Some(workspace))
        {
            return Err("Workspace is open in another window; focus it there".into());
        }
        r.windows
            .get_mut(owner)
            .ok_or("Window is not admitted")?
            .workspace = Some(workspace.into());
        Ok(())
    }
    pub fn reserve(&self, label: &str, workspace: &str) -> Result<(), String> {
        let mut r = self.0.lock().unwrap();
        if r.exit.is_some() {
            return Err("The app is exiting".into());
        }
        if r.windows.len() >= MAX_WINDOWS {
            return Err(
                "At most two workspace windows are supported. Close the other window first".into(),
            );
        }
        if r.windows
            .values()
            .any(|w| w.workspace.as_deref() == Some(workspace))
        {
            return Err("Workspace already belongs to a window".into());
        }
        if r.windows.contains_key(label) {
            return Err("Window already exists".into());
        }
        r.windows.insert(
            label.into(),
            Window {
                workspace: Some(workspace.into()),
                ..Default::default()
            },
        );
        Ok(())
    }
    pub fn register(&self, owner: &str) -> Result<(), String> {
        let mut r = self.0.lock().unwrap();
        if r.exit.is_some() {
            return Err("The app is exiting".into());
        }
        let window = r.windows.get_mut(owner).ok_or("Window is not admitted")?;
        window.registered = true;
        window.close_ready = false;
        Ok(())
    }
    pub fn remove(&self, owner: &str) {
        let mut r = self.0.lock().unwrap();
        r.windows.remove(owner);
        if r.update_owner.as_deref() == Some(owner) {
            r.update_owner = None;
        }
        // A destroyed/unregistered renderer has no remaining UI edits to acknowledge.
        if let Some(exit) = &mut r.exit {
            exit.pending.remove(owner);
        }
    }
    pub fn close_requested(&self, owner: &str) -> bool {
        self.0
            .lock()
            .unwrap()
            .windows
            .get(owner)
            .is_some_and(|w| w.registered && !w.close_ready)
    }
    pub fn close_flush(&self, owner: &str) -> Result<Flush, String> {
        let mut r = self.0.lock().unwrap();
        let pending = r
            .windows
            .get(owner)
            .ok_or("Window is not admitted")?
            .pending_close;
        let generation = if let Some(pending) = pending {
            pending
        } else {
            r.generation += 1;
            r.generation
        };
        r.windows.get_mut(owner).unwrap().pending_close = Some(generation);
        Ok(Flush {
            owner: owner.into(),
            action: "close".into(),
            generation: Some(generation),
        })
    }
    pub fn prepare_close(&self, owner: &str, generation: Option<u64>) -> Result<(), String> {
        let mut r = self.0.lock().unwrap();
        let w = r.windows.get_mut(owner).ok_or("Window is not admitted")?;
        if generation.is_none() || w.pending_close != generation {
            return Err("Close request was cancelled".into());
        }
        w.pending_close = None;
        w.registered = false;
        w.close_ready = true;
        Ok(())
    }
    pub fn begin_exit(&self, action: &str) -> (Flush, Vec<String>) {
        let mut r = self.0.lock().unwrap();
        if r.exit.is_none() {
            r.generation += 1;
            r.exit = Some(Exit {
                generation: r.generation,
                action: action.into(),
                pending: r
                    .windows
                    .iter()
                    .filter(|(_, w)| w.registered)
                    .map(|(l, _)| l.clone())
                    .collect(),
            });
        }
        let exit = r.exit.as_ref().unwrap();
        (
            Flush {
                owner: String::new(),
                action: exit.action.clone(),
                generation: Some(exit.generation),
            },
            exit.pending.iter().cloned().collect(),
        )
    }
    pub fn acknowledge(
        &self,
        owner: &str,
        generation: Option<u64>,
    ) -> Result<Option<String>, String> {
        let mut r = self.0.lock().unwrap();
        let earlier_close = generation.is_some()
            && r.windows
                .get(owner)
                .is_some_and(|w| w.pending_close == generation);
        let Some(exit) = &mut r.exit else {
            return Err("No exit is pending".into());
        };
        if generation != Some(exit.generation) && !earlier_close {
            return Err("Exit request was cancelled or replaced".into());
        }
        if !exit.pending.contains(owner) {
            return Ok(None);
        }
        exit.pending.remove(owner);
        Ok(exit.pending.is_empty().then(|| exit.action.clone()))
    }
    pub fn cancel(&self, owner: &str, generation: Option<u64>) -> bool {
        let mut r = self.0.lock().unwrap();
        let earlier_close = generation.is_some()
            && r.windows
                .get(owner)
                .is_some_and(|w| w.pending_close == generation);
        if earlier_close {
            r.windows.get_mut(owner).unwrap().pending_close = None;
        }
        if r.exit.as_ref().is_some_and(|e| {
            e.pending.contains(owner) && (generation == Some(e.generation) || earlier_close)
        }) {
            r.exit = None;
            for window in r.windows.values_mut() {
                window.pending_close = None;
            }
            true
        } else {
            false
        }
    }
    pub fn update_lease(&self, owner: &str, acquire: bool) -> Result<(), String> {
        let mut r = self.0.lock().unwrap();
        if acquire {
            if r.exit.is_some() {
                return Err("App is exiting".into());
            }
            if r.update_owner.is_some() {
                return Err("An update operation is already running in another window".into());
            }
            r.update_owner = Some(owner.into());
        } else if r.update_owner.as_deref() == Some(owner) {
            r.update_owner = None;
        }
        Ok(())
    }
    pub fn exiting(&self) -> bool {
        self.0.lock().unwrap().exit.is_some()
    }
}
pub fn authorize(window: &WebviewWindow) -> Result<(), String> {
    window
        .app_handle()
        .state::<Windows>()
        .authorize(window.label())
}
pub fn changed(app: &tauri::AppHandle) {
    let _ = app.emit("workspace-windows", ());
}
pub fn request_exit(app: &tauri::AppHandle, action: &str) {
    crate::attention::reveal_for_quit(app);
    let (payload, owners) = app.state::<Windows>().begin_exit(action);
    crate::attention::pause_for_exit(app);
    if owners.is_empty() {
        let app = app.clone();
        tauri::async_runtime::spawn(async move {
            finish(&app, action_owned(payload)).await;
        });
    } else {
        for owner in owners {
            app.state::<crate::git_operations::Operations>()
                .close_reads(&owner);
            app.state::<crate::worktree_tools::Worktrees>()
                .requests
                .cancel_owner(&owner);
            app.state::<crate::hosting::Hosting>()
                .reads
                .cancel_owner(&owner);
            let mut message = payload.clone();
            message.owner = owner.clone();
            let _ = app.emit_to(&owner, "workspace-flush", message);
        }
    }
}
fn action_owned(payload: Flush) -> String {
    payload.action
}
pub fn request_close(app: &tauri::AppHandle, owner: &str) {
    app.state::<crate::git_operations::Operations>()
        .close_reads(owner);
    app.state::<crate::worktree_tools::Worktrees>()
        .requests
        .cancel_owner(owner);
    app.state::<crate::hosting::Hosting>()
        .reads
        .cancel_owner(owner);
    if app.state::<Windows>().exiting() {
        request_exit(app, "quit");
    } else {
        if let Ok(payload) = app.state::<Windows>().close_flush(owner) {
            let _ = app.emit_to(owner, "workspace-flush", payload);
        }
    }
}
pub async fn finish(app: &tauri::AppHandle, action: String) {
    if app
        .state::<Windows>()
        .1
        .swap(true, std::sync::atomic::Ordering::SeqCst)
    {
        return;
    }
    let sessions = app.state::<crate::workspaces::Sessions>();
    let _work = sessions.gate.write().await;
    if let Err(error) = app.state::<crate::terminal::Terminals>().close_all().await {
        let windows = app.state::<Windows>();
        {
            let mut registry = windows.0.lock().unwrap();
            registry.exit = None;
            for window in registry.windows.values_mut() {
                window.pending_close = None;
                window.close_ready = false;
            }
        }
        windows.1.store(false, std::sync::atomic::Ordering::SeqCst);
        let _ = app.emit("workspace-exit-cancelled", ());
        let _ = app.emit("terminal-shutdown-error", error.to_string());
        return;
    }
    app.state::<crate::repository::Repositories>().close().await;
    crate::attention::shutdown(app);
    app.state::<crate::engine::Engine>().close().await;
    sessions
        .quit_ready
        .store(true, std::sync::atomic::Ordering::SeqCst);
    if action == "restart" {
        app.restart();
    } else {
        app.exit(0);
    }
}
#[tauri::command]
pub fn workspace_windows(
    window: WebviewWindow,
    state: tauri::State<'_, Windows>,
) -> Result<Vec<WindowInfo>, String> {
    state.authorize(window.label())?;
    Ok(state.list(window.label()))
}
#[tauri::command]
pub fn workspace_focus_window(
    app: tauri::AppHandle,
    window: WebviewWindow,
    target: String,
) -> Result<(), String> {
    authorize(&window)?;
    app.state::<Windows>().authorize(&target)?;
    let other = app
        .get_webview_window(&target)
        .ok_or("Window is no longer open")?;
    other.unminimize().map_err(|e| e.to_string())?;
    other.set_focus().map_err(|e| e.to_string())
}
#[tauri::command]
pub fn workspace_cancel_exit(
    app: tauri::AppHandle,
    window: WebviewWindow,
    generation: Option<u64>,
) -> Result<(), String> {
    authorize(&window)?;
    if app.state::<Windows>().cancel(window.label(), generation) {
        let _ = app.emit("workspace-exit-cancelled", ());
    }
    Ok(())
}

#[tauri::command]
pub fn updater_lease(
    window: WebviewWindow,
    state: tauri::State<'_, Windows>,
    acquire: bool,
) -> Result<(), String> {
    state.authorize(window.label())?;
    state.update_lease(window.label(), acquire)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn two_windows_have_independent_exclusive_workspaces_and_bounded_admission() {
        let windows = Windows::default();
        let available = vec!["a".into(), "b".into()];
        assert_eq!(windows.current("main", "a", &available).unwrap(), "a");
        assert!(windows.reserve("workspace-duplicate", "a").is_err());
        windows.reserve("workspace-b", "b").unwrap();
        assert_eq!(
            windows.current("workspace-b", "a", &available).unwrap(),
            "b"
        );
        assert_eq!(windows.current("main", "b", &available).unwrap(), "a");
        assert!(windows.select("main", "b").is_err());
        assert!(windows.reserve("workspace-third", "c").is_err());
        assert!(windows.authorize("workspace-forged").is_err());
        windows.remove("main");
        assert_eq!(
            windows.current("workspace-b", "a", &available).unwrap(),
            "b"
        );
        windows.select("workspace-b", "a").unwrap();
        windows.reserve("workspace-new", "b").unwrap();
        assert_eq!(windows.list("workspace-b").len(), 2);
    }
    #[test]
    fn quit_waits_for_both_windows_and_stale_acknowledgements_cannot_finish_a_new_exit() {
        let windows = Windows::default();
        windows.reserve("workspace-b", "b").unwrap();
        windows.register("main").unwrap();
        windows.register("workspace-b").unwrap();
        let (first, owners) = windows.begin_exit("quit");
        assert_eq!(owners.len(), 2);
        assert!(windows.writable().is_err());
        assert_eq!(windows.acknowledge("main", first.generation).unwrap(), None);
        assert!(windows.cancel("workspace-b", first.generation));
        assert!(windows.writable().is_ok());
        let (second, _) = windows.begin_exit("restart");
        assert!(windows.acknowledge("main", first.generation).is_err());
        assert!(windows.acknowledge("main", None).is_err());
        assert_eq!(
            windows.acknowledge("main", second.generation).unwrap(),
            None
        );
        assert_eq!(
            windows
                .acknowledge("workspace-b", second.generation)
                .unwrap(),
            Some("restart".into())
        );
    }
    #[test]
    fn closing_one_window_does_not_quit_the_other_and_a_racing_quit_accepts_its_flush() {
        let windows = Windows::default();
        windows.reserve("workspace-b", "b").unwrap();
        windows.register("main").unwrap();
        windows.register("workspace-b").unwrap();
        let close = windows.close_flush("main").unwrap();
        let (quit, _) = windows.begin_exit("quit");
        assert_ne!(close.generation, quit.generation);
        assert_eq!(windows.acknowledge("main", close.generation).unwrap(), None);
        assert_eq!(
            windows.acknowledge("workspace-b", quit.generation).unwrap(),
            Some("quit".into())
        );
        let windows = Windows::default();
        windows.reserve("workspace-b", "b").unwrap();
        windows.register("main").unwrap();
        windows.register("workspace-b").unwrap();
        let close = windows.close_flush("main").unwrap();
        windows.prepare_close("main", close.generation).unwrap();
        let (_, owners) = windows.begin_exit("quit");
        assert_eq!(owners, vec!["workspace-b"]);
    }
    #[test]
    fn cancelled_close_cannot_acknowledge_a_later_quit_and_updates_have_one_owner() {
        let windows = Windows::default();
        windows.register("main").unwrap();
        let close = windows.close_flush("main").unwrap();
        windows.cancel("main", close.generation);
        let (quit, _) = windows.begin_exit("quit");
        assert_ne!(close.generation, quit.generation);
        assert!(windows.acknowledge("main", close.generation).is_err());
        assert!(windows.update_lease("main", true).is_err());
        windows.cancel("main", quit.generation);
        windows.update_lease("main", true).unwrap();
        assert!(windows.update_lease("workspace-b", true).is_err());
        windows.update_lease("workspace-b", false).unwrap();
        assert!(windows.update_lease("main", true).is_err());
        windows.update_lease("main", false).unwrap();
        windows.update_lease("workspace-b", true).unwrap();
    }
}
