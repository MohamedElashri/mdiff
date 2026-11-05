#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
mod engine;
mod gitlab;
mod settings;
mod transport;
use std::sync::atomic::{AtomicBool, Ordering};
use tauri::Manager;
fn main() {
    // GUI launchers omit common package-manager directories from PATH.
    let mut paths: Vec<std::path::PathBuf> = std::env::var_os("PATH")
        .map(|p| std::env::split_paths(&p).collect())
        .unwrap_or_default();
    for path in ["/opt/homebrew/bin", "/usr/local/bin", "/usr/bin", "/bin"] {
        let path = std::path::PathBuf::from(path);
        if !paths.contains(&path) {
            paths.push(path);
        }
    }
    if let Some(home) = std::env::var_os("HOME") {
        for suffix in [".cargo/bin", ".local/bin"] {
            let path = std::path::PathBuf::from(&home).join(suffix);
            if !paths.contains(&path) {
                paths.push(path);
            }
        }
    }
    if let Ok(path) = std::env::join_paths(paths) {
        std::env::set_var("PATH", path);
    }
    let exiting = AtomicBool::new(false);
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(tauri_plugin_opener::init())
        .manage(engine::Engine::default())
        .invoke_handler(tauri::generate_handler![
            engine::open_review,
            engine::close_review,
            transport::review_request,
            gitlab::gitlab_action,
            settings::desktop_settings,
            settings::save_desktop_settings
        ])
        .build(tauri::generate_context!())
        .expect("Unable to start mdiff")
        .run(move |app, event| {
            if let tauri::RunEvent::ExitRequested { api, .. } = event {
                if !exiting.swap(true, Ordering::SeqCst) {
                    api.prevent_exit();
                    let handle = app.clone();
                    tauri::async_runtime::spawn(async move {
                        handle.state::<engine::Engine>().close().await;
                        handle.exit(0);
                    });
                }
            }
        });
}
