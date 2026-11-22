mod catalog;
mod discovery;
pub mod external;
pub(crate) mod terminals;
#[cfg(test)]
mod tests;

use crate::engine::Engine;
use discovery::{discover, InstalledEditor, Launcher, Locations};
use std::{path::Path, process::Stdio, time::Duration};
use tauri::{Manager, State};
use tokio::process::{Child, Command};

#[derive(serde::Serialize)]
pub struct EditorList {
    editors: Vec<InstalledEditor>,
    temporary: bool,
}

#[tauri::command]
pub async fn discover_editors(
    window: tauri::WebviewWindow,
    state: State<'_, Engine>,
    id: String,
) -> Result<EditorList, String> {
    let temporary = state
        .owned_runtime(&id, window.label())
        .await?
        .temporary_repository();
    let editors = tauri::async_runtime::spawn_blocking(|| discover(&Locations::system(), None))
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
    clean_git_environment(&mut cmd);
    cmd.arg(root)
        .current_dir(root)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    cmd
}

fn launch(editor: &InstalledEditor, root: &Path) -> Result<Child, String> {
    command(&editor.launcher, root)
        .spawn()
        .map_err(|e| format!("Could not open {}: {e}", editor.name))
}

pub(crate) async fn check_launch(editor: &InstalledEditor, mut child: Child) -> Result<(), String> {
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
    app: tauri::AppHandle,
    window: tauri::WebviewWindow,
    state: State<'_, Engine>,
    id: String,
    editor_id: String,
) -> Result<(), String> {
    crate::windows::authorize(&window)?;
    let editor = tauri::async_runtime::spawn_blocking(move || {
        discover(&Locations::system(), Some(&editor_id)).pop()
    })
    .await
    .map_err(|e| e.to_string())?
    .ok_or("This editor is no longer available. Refresh the list.")?;
    // The caller supplies only a catalog ID. The folder always comes from the active review.
    let (_checkout, child) = {
        let runtime = state.owned_runtime(&id, window.label()).await?;
        let _external_lease =
            crate::git_operations::external_lease(&app, runtime.repository_root()).await?;
        app.state::<crate::windows::Windows>()
            .admit(window.label())?;
        if let Some(checkout) = runtime.owned_checkout() {
            checkout.mark_editable();
        }
        (
            (runtime.temporary_checkout(), runtime.owned_checkout()),
            launch(&editor, runtime.repository_root())?,
        )
    };
    check_launch(&editor, child).await
}

fn clean_git_environment(command: &mut Command) {
    for name in [
        "GIT_DIR",
        "GIT_WORK_TREE",
        "GIT_INDEX_FILE",
        "GIT_COMMON_DIR",
        "GIT_NAMESPACE",
        "GIT_PREFIX",
    ] {
        command.env_remove(name);
    }
}
