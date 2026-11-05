mod catalog;
mod discovery;
#[cfg(test)]
mod tests;

use crate::engine::Engine;
use discovery::{discover, InstalledEditor, Launcher, Locations};
use std::{path::Path, process::Stdio, time::Duration};
use tauri::State;
use tokio::process::Command;

#[derive(serde::Serialize)]
pub struct EditorList {
    editors: Vec<InstalledEditor>,
    temporary: bool,
}

#[tauri::command]
pub async fn discover_editors(state: State<'_, Engine>, id: String) -> Result<EditorList, String> {
    let temporary = {
        let slot = state.0.lock().await;
        let runtime = slot
            .as_ref()
            .filter(|r| r.id == id)
            .ok_or("This review has closed")?;
        runtime.temporary_repository()
    };
    let editors = tauri::async_runtime::spawn_blocking(|| discover(&Locations::system()))
        .await
        .map_err(|e| e.to_string())?;
    Ok(EditorList { editors, temporary })
}

fn command(launcher: &Launcher, root: &Path) -> Command {
    let mut cmd = match launcher {
        Launcher::Command(binary) => Command::new(binary),
        Launcher::MacApp(app) => {
            let mut cmd = Command::new("/usr/bin/open");
            cmd.arg("-a").arg(app);
            cmd
        }
        Launcher::Desktop { gio, entry } => {
            let mut cmd = Command::new(gio);
            cmd.arg("launch").arg(entry);
            cmd
        }
    };
    cmd.arg(root)
        .current_dir(root)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    cmd
}

async fn launch(editor: &InstalledEditor, root: &Path) -> Result<(), String> {
    let mut child = command(&editor.launcher, root)
        .spawn()
        .map_err(|e| format!("Could not open {}: {e}", editor.name))?;
    match tokio::time::timeout(Duration::from_millis(700), child.wait()).await {
        Ok(Ok(status)) if status.success() => Ok(()),
        Ok(Ok(status)) => Err(format!(
            "{} could not open this repository ({status}). Try opening it directly.",
            editor.name
        )),
        Ok(Err(e)) => Err(format!("Could not open {}: {e}", editor.name)),
        Err(_) => {
            // Editors may remain attached for their lifetime; reap them without blocking the UI.
            tauri::async_runtime::spawn(async move {
                let _ = child.wait().await;
            });
            Ok(())
        }
    }
}

#[tauri::command]
pub async fn open_in_editor(
    state: State<'_, Engine>,
    id: String,
    editor_id: String,
) -> Result<(), String> {
    let editor = tauri::async_runtime::spawn_blocking(move || {
        discover(&Locations::system())
            .into_iter()
            .find(|editor| editor.id == editor_id)
    })
    .await
    .map_err(|e| e.to_string())?
    .ok_or("This editor is no longer available. Refresh the list.")?;
    // The caller supplies only a catalog ID. The folder always comes from the active review.
    let slot = state.0.lock().await;
    let runtime = slot
        .as_ref()
        .filter(|r| r.id == id)
        .ok_or("This review has closed")?;
    launch(&editor, runtime.repository_root()).await
}
