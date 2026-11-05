use crate::engine::Engine;
use serde_json::Value;
use std::sync::Arc;
use tauri::{Manager, State};
async fn store(
    app: &tauri::AppHandle,
) -> Result<Arc<diffle::user_config::UserConfigStore>, String> {
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
    let active = state.0.lock().await;
    let config = match active.as_ref() {
        Some(runtime) => runtime.config.clone(),
        None => store(&app).await?,
    };
    serde_json::to_value(config.get()).map_err(|e| e.to_string())
}
#[tauri::command]
pub async fn save_desktop_settings(
    app: tauri::AppHandle,
    state: State<'_, Engine>,
    lsp_commands: Value,
    github_hosts: Value,
) -> Result<(), String> {
    let commands = serde_json::from_value(lsp_commands)
        .map_err(|e| format!("Invalid language server settings: {e}"))?;
    let hosts =
        serde_json::from_value(github_hosts).map_err(|e| format!("Invalid GitHub hosts: {e}"))?;
    let active = state.0.lock().await;
    let config = match active.as_ref() {
        Some(runtime) => runtime.config.clone(),
        None => store(&app).await?,
    };
    config
        .set_desktop_commands(commands, hosts)
        .await
        .map_err(|e| e.to_string())
}
