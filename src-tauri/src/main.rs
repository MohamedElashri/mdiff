#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
mod agents;
mod attention;
mod editors;
mod engine;
mod git_operations;
mod github;
mod gitlab;
mod hosting;
mod localization;
mod repository;
mod settings;
mod shell_preferences;
mod storage;
mod terminal;
mod transport;
mod updater;
mod windows;
mod workspaces;
mod worktree_tools;
use std::sync::atomic::Ordering;
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
    tauri::Builder::default()
        .setup(|_app| {
            #[cfg(target_os = "macos")]
            windows::macos::install(_app.handle())?;
            agents::setup(_app.handle());
            attention::setup(_app.handle());
            Ok(())
        })
        .on_page_load(|webview, payload| {
            if payload.event() == tauri::webview::PageLoadEvent::Started {
                if let Some(terminals) = webview.app_handle().try_state::<terminal::Terminals>() {
                    terminals.stop_owner(webview.label());
                }
            }
        })
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_notification::init())
        .manage(localization::Localization::default())
        .manage(attention::Attention::default())
        .manage(agents::Agents::default())
        .manage(agents::launch::Launches::default())
        .manage(terminal::Terminals::default())
        .manage(engine::Engine::default())
        .manage(workspaces::Sessions::default())
        .manage(windows::Windows::default())
        .manage(settings::MemorySettings::default())
        .manage(hosting::Hosting::default())
        .manage(git_operations::Operations::default())
        .manage(repository::Repositories::default())
        .manage(worktree_tools::Worktrees::default())
        .manage(transport::Requests::default())
        .invoke_handler(tauri::generate_handler![
            localization::desktop_locale,
            attention::attention_view,
            attention::attention_badge,
            attention::attention_preferences,
            attention::attention_reload,
            attention::attention_update,
            attention::attention_open,
            agents::agent_view,
            agents::agent_scan,
            agents::agent_reload,
            agents::agent_settings,
            agents::agent_tools,
            agents::agent_preferences,
            agents::agent_unwatch,
            agents::agent_hooks,
            agents::agent_hook_config,
            agents::launch::agent_preview,
            agents::launch::agent_launch,
            agents::launch::agent_import,
            terminal::terminal_start,
            terminal::terminal_list,
            terminal::terminal_input,
            terminal::terminal_ack,
            terminal::terminal_resize,
            terminal::terminal_close,
            terminal::terminal_close_owner,
            terminal::preferences::terminal_preferences,
            shell_preferences::shell_preferences,
            shell_preferences::save_shell_preferences,
            windows::workspace_windows,
            windows::workspace_focus_window,
            windows::workspace_cancel_exit,
            workspaces::workspace_open_window,
            workspaces::workspace_move_window,
            workspaces::workspace_register_ui,
            workspaces::workspace_finish_exit,
            workspaces::workspace_read_view,
            workspaces::workspace_release,
            workspaces::workspace_load,
            workspaces::workspace_update,
            workspaces::workspace_activate,
            workspaces::workspace_capture,
            workspaces::workspace_save_view,
            windows::updater_lease,
            updater::updater_info,
            updater::restart_after_update,
            editors::discover_editors,
            editors::external::external_tools,
            editors::external::external_preferences,
            editors::external::save_external_preferences,
            editors::external::open_external_tool,
            editors::open_in_editor,
            repository::open_repository_context,
            repository::close_repository_context,
            repository::repository_request,
            worktree_tools::worktree_read,
            worktree_tools::worktree_cancel,
            git_operations::drafts::git_operation_draft,
            git_operations::git_operation_read,
            git_operations::git_operation_cancel_read,
            git_operations::git_operation_forget_preview,
            git_operations::git_operation_preview,
            git_operations::git_operation_execute,
            git_operations::git_operation_receipts,
            git_operations::git_operation_cancel,
            git_operations::git_operation_inspect,
            engine::open_review,
            engine::close_review,
            engine::suspend_review,
            engine::resume_review,
            engine::review_descriptors,
            engine::enable_review_lsp,
            transport::review_request,
            transport::cancel_review_request,
            gitlab::gitlab_action,
            hosting::hosting_context,
            hosting::hosting_cancel,
            hosting::hosting_read,
            hosting::hosting_write,
            settings::desktop_settings,
            settings::review_preferences,
            settings::save_desktop_settings
        ])
        .build(tauri::generate_context!())
        .expect("Unable to start mdiff")
        .run(move |app, event| {
            #[cfg(target_os = "macos")]
            if let tauri::RunEvent::Reopen { .. } = &event {
                attention::reopen(app);
            }

            if let tauri::RunEvent::WindowEvent {
                label,
                event: tauri::WindowEvent::CloseRequested { api, .. },
                ..
            } = &event
            {
                if attention::hide_on_close(app, label) {
                    api.prevent_close();
                    return;
                }
                if app.state::<windows::Windows>().close_requested(label) {
                    api.prevent_close();
                    windows::request_close(app, label);
                }
            }
            if let tauri::RunEvent::WindowEvent {
                label,
                event: tauri::WindowEvent::Destroyed,
                ..
            } = &event
            {
                app.state::<hosting::Hosting>().reads.cancel_owner(label);
                app.state::<git_operations::Operations>().close_reads(label);
                app.state::<worktree_tools::Worktrees>()
                    .requests
                    .cancel_owner(label);
                app.state::<agents::Agents>().close_owner(label);
                app.state::<agents::launch::Launches>().close_owner(label);
                app.state::<windows::Windows>().remove(label);
                windows::changed(app);
                let handle = app.clone();
                let owner = label.clone();
                tauri::async_runtime::spawn(async move {
                    let sessions = handle.state::<workspaces::Sessions>();
                    let _work = sessions.gate.read().await;
                    let _opening = sessions.opening(&owner).await;
                    let _ = handle
                        .state::<terminal::Terminals>()
                        .close_owner(&owner)
                        .await;
                    handle
                        .state::<repository::Repositories>()
                        .close_owner(&owner)
                        .await;
                    handle.state::<engine::Engine>().close_owner(&owner).await;
                    handle
                        .state::<workspaces::Sessions>()
                        .close_owner(&owner)
                        .await;
                });
            }
            if let tauri::RunEvent::ExitRequested { api, code, .. } = event {
                if app
                    .state::<workspaces::Sessions>()
                    .quit_ready
                    .load(Ordering::SeqCst)
                {
                    return;
                }
                api.prevent_exit();
                windows::request_exit(
                    app,
                    if code == Some(tauri::RESTART_EXIT_CODE) {
                        "restart"
                    } else {
                        "quit"
                    },
                );
            }
        });
}
