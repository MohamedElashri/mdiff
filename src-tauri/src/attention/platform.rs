use super::{model::*, Attention};
use anyhow::Result;
use tauri::{
    menu::{Menu, MenuItem},
    tray::TrayIconBuilder,
    Emitter, Manager,
};
use tauri_plugin_notification::NotificationExt;
pub fn show(app: &tauri::AppHandle, owner: Option<&str>) {
    let window = owner
        .and_then(|o| app.get_webview_window(o))
        .or_else(|| app.get_webview_window("main"))
        .or_else(|| app.webview_windows().into_values().next());
    if let Some(w) = window {
        let _ = w.show();
        let _ = w.unminimize();
        let _ = w.set_focus();
    }
}
pub fn tray(app: &tauri::AppHandle, p: &Preferences, items: &[Item], unread: usize) -> Result<()> {
    if !p.tray {
        if let Some(t) = app.tray_by_id("attention") {
            t.set_visible(false)?;
        }
        return Ok(());
    }
    let locale = app.state::<crate::localization::Localization>();
    let open = MenuItem::with_id(
        app,
        "attention-open",
        format!(
            "{} ({unread} {})",
            locale.text("Needs attention", "À traiter"),
            locale.text("unread", "non lus")
        ),
        true,
        None::<&str>,
    )?;
    let quit = MenuItem::with_id(
        app,
        "attention-quit",
        locale.text("Quit mdiff", "Quitter mdiff"),
        true,
        None::<&str>,
    )?;
    let menu = Menu::with_items(app, &[&open])?;
    for i in items
        .iter()
        .filter(|i| p.source(&i.source) && i.resolved.is_none() && !i.dismissed && !i.paused)
        .rev()
        .take(5)
    {
        menu.append(&MenuItem::with_id(
            app,
            format!("attention-item-{}", i.id),
            i.title.chars().take(72).collect::<String>(),
            true,
            None::<&str>,
        )?)?;
    }
    menu.append(&quit)?;
    if let Some(t) = app.tray_by_id("attention") {
        t.set_menu(Some(menu))?;
        t.set_visible(true)?;
        t.set_title(Some(if unread == 0 {
            String::new()
        } else {
            unread.to_string()
        }))?;
    } else {
        let mut builder = TrayIconBuilder::with_id("attention")
            .menu(&menu)
            .tooltip(locale.text("mdiff — Needs attention", "mdiff — À traiter"))
            .title(if unread == 0 {
                String::new()
            } else {
                unread.to_string()
            })
            .on_menu_event(|app, event| {
                let id = event.id().as_ref();
                if id == "attention-quit" {
                    super::reveal_for_quit(app);
                    crate::windows::request_exit(app, "quit");
                } else if id == "attention-open" {
                    open_inbox(app);
                } else if let Some(id) = id.strip_prefix("attention-item-") {
                    let app = app.clone();
                    let id = id.to_owned();
                    tauri::async_runtime::spawn(async move {
                        if super::route(&app, None, &id).await.is_err() {
                            open_inbox(&app);
                        }
                    });
                }
            });
        if let Some(icon) = app.default_window_icon() {
            builder = builder.icon(icon.clone());
        }
        builder.build(app)?;
    }
    Ok(())
}
pub fn install_actions(app: &tauri::AppHandle) -> Result<()> {
    let handle = app.clone();
    app.notification().on_action(move |action| {
        if action.action_id() != "tap" {
            return;
        }
        let id = action
            .notification()
            .and_then(|n| n.extra().get("attentionId"))
            .and_then(|v| v.as_str())
            .map(str::to_owned);
        let app = handle.clone();
        tauri::async_runtime::spawn(async move {
            let enabled = app
                .state::<Attention>()
                .data
                .lock()
                .await
                .doc
                .preferences
                .notifications;
            if !enabled || app.state::<crate::windows::Windows>().exiting() {
                return;
            }
            if let Some(id) = id {
                if super::route(&app, None, &id).await.is_ok() {
                    return;
                }
            }
            open_inbox(&app);
        });
    })?;
    Ok(())
}
pub fn notify(app: &tauri::AppHandle, id: &str) -> Result<()> {
    let locale = app.state::<crate::localization::Localization>();
    app.notification()
        .builder()
        .title(locale.text("mdiff — Needs attention", "mdiff — À traiter"))
        .body(locale.text(
            "An item needs attention. Open mdiff to inspect it.",
            "Un élément nécessite votre attention. Ouvrez mdiff pour le consulter.",
        ))
        .extra("attentionId", id)
        .show()?;
    Ok(())
}

fn open_inbox(app: &tauri::AppHandle) {
    let owner = app
        .get_webview_window("main")
        .map(|w| w.label().to_owned())
        .or_else(|| app.webview_windows().keys().next().cloned());
    if let Some(owner) = owner {
        show(app, Some(&owner));
        let _ = app.emit_to(&owner, "attention-open", ());
    }
}
