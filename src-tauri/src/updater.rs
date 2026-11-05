#[cfg(target_os = "linux")]
use tauri::Manager;

#[derive(serde::Serialize)]
pub struct UpdateInfo {
    version: String,
    supported: bool,
    reason: Option<&'static str>,
}

#[tauri::command]
pub fn updater_info(app: tauri::AppHandle) -> UpdateInfo {
    let configured = app
        .config()
        .plugins
        .0
        .get("updater")
        .and_then(|config| config.get("pubkey"))
        .and_then(|key| key.as_str())
        .is_some_and(|key| !key.trim().is_empty());
    #[cfg(target_os = "linux")]
    let supported_package = app.env().appimage.is_some();
    #[cfg(not(target_os = "linux"))]
    let supported_package = cfg!(target_os = "macos");
    let reason = if !supported_package {
        Some("In-app updates require the AppImage on Linux. Update other packages through your package manager or download a release.")
    } else if !configured {
        Some("Update signing is not configured for this build.")
    } else {
        None
    };
    UpdateInfo {
        version: app.package_info().version.to_string(),
        supported: reason.is_none(),
        reason,
    }
}

#[tauri::command]
pub async fn restart_after_update(
    app: tauri::AppHandle,
    engine: tauri::State<'_, crate::engine::Engine>,
) -> Result<(), String> {
    // Tauri restart requests cannot be prevented in ExitRequested.
    engine.close().await;
    app.request_restart();
    Ok(())
}
