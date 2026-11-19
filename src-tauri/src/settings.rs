use crate::engine::Engine;
use serde_json::Value;
use std::sync::Arc;
use tauri::{Manager, State};
async fn store(
    app: &tauri::AppHandle,
    state: &Engine,
) -> Result<Arc<diffle::user_config::UserConfigStore>, String> {
    let config = state
        .active
        .lock()
        .await
        .as_ref()
        .map(|runtime| runtime.config.clone());
    if let Some(config) = config {
        return Ok(config);
    }
    let path = app
        .path()
        .app_config_dir()
        .map_err(|e| e.to_string())?
        .join("review.json");
    Ok(Arc::new(
        diffle::user_config::UserConfigStore::open(path).await,
    ))
}
#[tauri::command]
pub async fn desktop_settings(
    app: tauri::AppHandle,
    state: State<'_, Engine>,
) -> Result<Value, String> {
    let config = store(&app, &state).await?;
    let mut value = serde_json::to_value(config.get()).map_err(|e| e.to_string())?;
    value["githubEnabled"] = crate::github::Connections::load(config.file())
        .await
        .map_err(|e| e.to_string())?
        .github_enabled
        .into();
    Ok(value)
}
#[tauri::command]
pub async fn save_desktop_settings(
    app: tauri::AppHandle,
    state: State<'_, Engine>,
    lsp_commands: Value,
    github_hosts: Value,
    github_enabled: bool,
) -> Result<(), String> {
    let commands = serde_json::from_value(lsp_commands)
        .map_err(|e| format!("Invalid language server settings: {e}"))?;
    let hosts =
        serde_json::from_value(github_hosts).map_err(|e| format!("Invalid GitHub hosts: {e}"))?;
    let _transition = state.lifecycle.lock().await;
    let config = store(&app, &state).await?;
    config
        .set_desktop_commands(commands, hosts)
        .await
        .map_err(|e| e.to_string())?;
    crate::github::Connections { github_enabled }
        .save(config.file())
        .await
        .map_err(|e| e.to_string())?;
    if let Some(runtime) = state.active.lock().await.as_ref() {
        runtime.github.enabled.store(
            github_enabled && runtime.gitlab.is_none(),
            std::sync::atomic::Ordering::SeqCst,
        );
    }
    Ok(())
}
