//! Window-owned terminal authority. No renderer paths/argv or persisted commands.
pub(crate) mod core;
pub mod preferences;
use anyhow::{Context, Result};
use serde::Serialize;
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};
use tauri::{
    ipc::{Channel, Response},
    Manager, WebviewWindow,
};
use tokio::sync::Semaphore;
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Info {
    pub id: String,
    pub root: String,
    pub shell: String,
    pub pid: u32,
    pub running: bool,
    pub exit: Option<core::Exit>,
}
struct Entry {
    owner: String,
    root: PathBuf,
    shell: String,
    actor: Arc<core::Actor>,
}
pub struct Terminals {
    entries: Mutex<HashMap<String, Entry>>,
    slots: Arc<Semaphore>,
    owners: Arc<Mutex<HashMap<String, usize>>>,
}
impl Default for Terminals {
    fn default() -> Self {
        Self {
            entries: Mutex::default(),
            slots: Arc::new(Semaphore::new(4)),
            owners: Arc::default(),
        }
    }
}
struct OwnerSlot {
    owner: String,
    owners: Arc<Mutex<HashMap<String, usize>>>,
}
impl Drop for OwnerSlot {
    fn drop(&mut self) {
        let mut owners = self.owners.lock().unwrap();
        if let Some(n) = owners.get_mut(&self.owner) {
            *n -= 1;
            if *n == 0 {
                owners.remove(&self.owner);
            }
        }
    }
}
impl Terminals {
    fn reserve(&self, owner: &str) -> Result<OwnerSlot> {
        let mut owners = self.owners.lock().unwrap();
        let count = owners.entry(owner.into()).or_default();
        anyhow::ensure!(
            *count < 2,
            "At most two running terminals per window are supported"
        );
        *count += 1;
        Ok(OwnerSlot {
            owner: owner.into(),
            owners: self.owners.clone(),
        })
    }
    fn actor(&self, owner: &str, id: &str) -> Result<Arc<core::Actor>> {
        let entries = self.entries.lock().unwrap();
        let entry = entries.get(id).context("Terminal is unavailable")?;
        anyhow::ensure!(entry.owner == owner, "Terminal belongs to another window");
        Ok(entry.actor.clone())
    }
    fn list(&self, owner: &str) -> Vec<Info> {
        self.entries
            .lock()
            .unwrap()
            .iter()
            .filter(|(_, e)| e.owner == owner)
            .map(|(id, e)| Info {
                id: id.clone(),
                root: e.root.to_string_lossy().into_owned(),
                shell: e.shell.clone(),
                pid: e.actor.pid,
                running: !*e.actor.done.borrow(),
                exit: e.actor.status.lock().unwrap().clone(),
            })
            .collect()
    }
    pub fn stop_owner(&self, owner: &str) {
        for entry in self.entries.lock().unwrap().values() {
            if entry.owner == owner {
                entry.actor.stop();
            }
        }
    }
    pub fn uses_checkout(&self, root: &Path) -> bool {
        self.entries
            .lock()
            .unwrap()
            .values()
            .any(|e| e.root == root && !*e.actor.done.borrow())
    }
    pub async fn close_owner(&self, owner: &str) -> Result<()> {
        let actors = self
            .entries
            .lock()
            .unwrap()
            .values()
            .filter(|e| e.owner == owner)
            .map(|e| e.actor.clone())
            .collect::<Vec<_>>();
        for actor in &actors {
            actor.stop();
        }
        for actor in actors {
            tokio::time::timeout(std::time::Duration::from_secs(5), actor.finished())
                .await
                .context("Terminal teardown timed out; inspect its processes before closing")??;
        }
        self.entries.lock().unwrap().retain(|_, e| e.owner != owner);
        Ok(())
    }
    pub async fn close_all(&self) -> Result<()> {
        let owners = self
            .entries
            .lock()
            .unwrap()
            .values()
            .map(|e| e.owner.clone())
            .collect::<std::collections::HashSet<_>>();
        for owner in owners {
            self.close_owner(&owner).await?;
        }
        Ok(())
    }
}
#[tauri::command]
pub async fn terminal_start(
    app: tauri::AppHandle,
    window: WebviewWindow,
    context: String,
    worktree: Option<String>,
    cols: u16,
    rows: u16,
    on_event: Channel<Response>,
) -> Result<Info, String> {
    crate::windows::authorize(&window)?;
    let result = async {
        let sessions = app.state::<crate::workspaces::Sessions>();
        let _gate = sessions.gate.read().await;
        app.state::<crate::windows::Windows>()
            .admit(window.label())
            .map_err(anyhow::Error::msg)?;
        core::validate_size(cols, rows)?;
        anyhow::ensure!(
            preferences::load(&preferences::file(&app)?).await?.enabled,
            "Embedded terminals are disabled. Enable new terminals or use an external terminal."
        );
        let target =
            crate::worktree_tools::tool_target(&app, window.label(), &context, worktree.as_deref())
                .await?;
        anyhow::ensure!(
            target.temporary.is_none() || target.owned.is_some(),
            "This temporary checkout cannot retain shell edits. Refresh its remote source first."
        );
        let identity = diffle::git::operations::identity(&target.root).await?;
        let held = app
            .state::<crate::git_operations::Operations>()
            .locks
            .acquire(&identity, true)
            .await?;
        anyhow::ensure!(
            diffle::git::operations::identity(&target.root).await? == identity,
            "Checkout identity changed before terminal startup"
        );
        app.state::<crate::windows::Windows>()
            .admit(window.label())
            .map_err(anyhow::Error::msg)?;
        anyhow::ensure!(
            !*target.closing.borrow(),
            "The review/context closed before terminal startup"
        );
        let shell = core::shell()?;
        let command = core::command(&target.root, &shell)?;
        if let Some(checkout) = &target.owned {
            checkout.mark_editable();
        }
        let info = start_owned(
            &app,
            &window,
            identity.root,
            shell,
            command,
            Box::new(target),
            on_event,
            cols,
            rows,
        )
        .await?;
        drop(held);
        Ok(info)
    }
    .await;
    result.map_err(|e: anyhow::Error| e.to_string())
}
// Shared admission/streaming/cleanup for explicit agent launches; callers hold the session gate.
#[allow(clippy::too_many_arguments)]
pub(crate) async fn start_owned(
    app: &tauri::AppHandle,
    window: &WebviewWindow,
    root: PathBuf,
    shell: PathBuf,
    command: portable_pty::CommandBuilder,
    lease: Box<dyn Send>,
    on_event: Channel<Response>,
    cols: u16,
    rows: u16,
) -> Result<Info> {
    let state = app.state::<Terminals>();
    let slot = state
        .slots
        .clone()
        .try_acquire_owned()
        .context("Four terminals are already active; close one before starting another")?;
    let owner_slot = state.reserve(window.label())?;
    let for_spawn = root.clone();
    let actor = tokio::task::spawn_blocking(move || {
        core::spawn(
            &for_spawn,
            command,
            cols,
            rows,
            Arc::new(move |bytes| on_event.send(Response::new(bytes)).is_ok()),
            Box::new((lease, slot, owner_slot)),
        )
    })
    .await??;
    let actor = Arc::new(actor);
    if let Err(error) = app.state::<crate::windows::Windows>().admit(window.label()) {
        actor.stop();
        actor.finished().await?;
        anyhow::bail!(error);
    }
    let id = crate::workspaces::id();
    {
        let mut entries = state.entries.lock().unwrap();
        if entries.len() >= 8 {
            if let Some(old) = entries
                .iter()
                .find(|(_, e)| *e.actor.done.borrow())
                .map(|(id, _)| id.clone())
            {
                entries.remove(&old);
            }
        }
        entries.insert(
            id.clone(),
            Entry {
                owner: window.label().into(),
                root: root.clone(),
                shell: shell.to_string_lossy().into_owned(),
                actor: actor.clone(),
            },
        );
    }
    let running = !*actor.done.borrow();
    let exit = actor.status.lock().unwrap().clone();
    Ok(Info {
        id,
        root: root.to_string_lossy().into_owned(),
        shell: shell.to_string_lossy().into_owned(),
        pid: actor.pid,
        running,
        exit,
    })
}
#[tauri::command]
pub fn terminal_list(app: tauri::AppHandle, window: WebviewWindow) -> Result<Vec<Info>, String> {
    crate::windows::authorize(&window)?;
    Ok(app.state::<Terminals>().list(window.label()))
}
#[tauri::command]
pub fn terminal_input(
    app: tauri::AppHandle,
    window: WebviewWindow,
    id: String,
    data: Vec<u8>,
) -> Result<(), String> {
    crate::windows::authorize(&window)?;
    app.state::<crate::windows::Windows>()
        .admit(window.label())?;
    app.state::<Terminals>()
        .actor(window.label(), &id)
        .and_then(|a| a.input(data))
        .map_err(|e| e.to_string())
}
#[tauri::command]
pub fn terminal_ack(
    app: tauri::AppHandle,
    window: WebviewWindow,
    id: String,
    seq: u64,
) -> Result<(), String> {
    crate::windows::authorize(&window)?;
    app.state::<Terminals>()
        .actor(window.label(), &id)
        .and_then(|a| a.ack(seq))
        .map_err(|e| e.to_string())
}
#[tauri::command]
pub fn terminal_resize(
    app: tauri::AppHandle,
    window: WebviewWindow,
    id: String,
    cols: u16,
    rows: u16,
) -> Result<(), String> {
    crate::windows::authorize(&window)?;
    app.state::<crate::windows::Windows>()
        .admit(window.label())?;
    app.state::<Terminals>()
        .actor(window.label(), &id)
        .and_then(|a| a.resize(cols, rows))
        .map_err(|e| e.to_string())
}
#[tauri::command]
pub async fn terminal_close(
    app: tauri::AppHandle,
    window: WebviewWindow,
    id: String,
) -> Result<(), String> {
    crate::windows::authorize(&window)?;
    let state = app.state::<Terminals>();
    let actor = state
        .actor(window.label(), &id)
        .map_err(|e| e.to_string())?;
    actor.stop();
    tokio::time::timeout(std::time::Duration::from_secs(5), actor.finished())
        .await
        .map_err(|_| "Terminal cleanup timed out".to_string())?
        .map_err(|e| e.to_string())?;
    state.entries.lock().unwrap().remove(&id);
    Ok(())
}
#[tauri::command]
pub async fn terminal_close_owner(
    app: tauri::AppHandle,
    window: WebviewWindow,
) -> Result<(), String> {
    crate::windows::authorize(&window)?;
    app.state::<Terminals>()
        .close_owner(window.label())
        .await
        .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn window_authority_cleanup_and_checkout_protection_follow_live_workers() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().canonicalize().unwrap();
        let mut cmd = portable_pty::CommandBuilder::new("/bin/sh");
        cmd.args(["-c", "sleep 100"]);
        cmd.cwd(&root);
        let state = Terminals::default();
        let slot = state.slots.clone().try_acquire_owned().unwrap();
        let owner = state.reserve("a").unwrap();
        let actor = Arc::new(
            core::spawn(
                &root,
                cmd,
                80,
                24,
                Arc::new(|_| true),
                Box::new((slot, owner)),
            )
            .unwrap(),
        );
        state.entries.lock().unwrap().insert(
            "terminal".into(),
            Entry {
                owner: "a".into(),
                root: root.clone(),
                shell: "/bin/sh".into(),
                actor,
            },
        );
        assert!(state.actor("b", "terminal").is_err());
        assert!(state.list("b").is_empty());
        assert!(state.uses_checkout(&root));
        state.close_owner("b").await.unwrap();
        assert!(state.list("a")[0].running);
        state.close_owner("a").await.unwrap();
        assert!(!state.uses_checkout(&root));
        assert!(state.list("a").is_empty());
        assert_eq!(state.slots.available_permits(), 4);
        assert!(state.owners.lock().unwrap().is_empty());
    }
    #[test]
    fn owner_and_process_limits_release_on_failed_or_ended_startup() {
        let state = Terminals::default();
        let first = state.reserve("a").unwrap();
        let second = state.reserve("a").unwrap();
        assert!(state.reserve("a").is_err());
        let other = state.reserve("b").unwrap();
        drop(first);
        assert!(state.reserve("a").is_ok());
        drop((second, other));
        let slots = (0..4)
            .map(|_| state.slots.clone().try_acquire_owned().unwrap())
            .collect::<Vec<_>>();
        assert!(state.slots.clone().try_acquire_owned().is_err());
        drop(slots);
        assert_eq!(state.slots.available_permits(), 4);
    }
}
