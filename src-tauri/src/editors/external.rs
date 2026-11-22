use super::{
    discovery::{discover, Locations},
    terminals,
};
use anyhow::{Context, Result};
use diffle::persist::{with_file_lock, write_file_atomic};
use serde::{Deserialize, Serialize};
use std::path::Path;
use tauri::{Emitter, Manager, WebviewWindow};
#[derive(Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Preferences {
    pub version: u8,
    pub revision: u64,
    pub terminal: Option<String>,
}
impl Default for Preferences {
    fn default() -> Self {
        Self {
            version: 1,
            revision: 0,
            terminal: None,
        }
    }
}
#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Editor,
    Terminal,
}
async fn load(path: &Path) -> Result<Preferences> {
    if !path.exists() {
        return Ok(Preferences::default());
    }
    anyhow::ensure!(
        tokio::fs::metadata(path).await?.len() <= 8192,
        "External-tool preferences exceed their limit; original file was preserved"
    );
    let value: Preferences = serde_json::from_slice(&tokio::fs::read(path).await?)?;
    anyhow::ensure!(
        value.version == 1
            && value
                .terminal
                .as_ref()
                .is_none_or(|id| terminals::known(id)),
        "Unsupported terminal preference; original file was preserved"
    );
    Ok(value)
}
async fn save(path: &Path, revision: u64, terminal: Option<String>) -> Result<Preferences> {
    anyhow::ensure!(
        terminal.as_deref().is_none_or(terminals::known),
        "Choose a supported terminal ID"
    );
    with_file_lock(path, || async {
        let old = load(path).await?;
        anyhow::ensure!(
            old.revision == revision,
            "External-tool preferences changed in another window. Refresh before saving."
        );
        let value = Preferences {
            version: 1,
            revision: revision
                .checked_add(1)
                .context("Preference revision exhausted")?,
            terminal,
        };
        write_file_atomic(path, &serde_json::to_string(&value)?).await?;
        Ok(value)
    })
    .await
}
#[tauri::command]
pub async fn external_preferences(
    app: tauri::AppHandle,
    window: WebviewWindow,
) -> Result<serde_json::Value, String> {
    crate::windows::authorize(&window)?;
    let sessions = app.state::<crate::workspaces::Sessions>();
    let _work = sessions.gate.read().await;
    app.state::<crate::windows::Windows>()
        .admit(window.label())?;
    let result = async {
        let preferences = load(&app.path().app_config_dir()?.join("external-tools.json")).await?;
        let (editors, terminals) = tauri::async_runtime::spawn_blocking(|| {
            let places = Locations::system();
            (
                discover(&places, None),
                terminals::discover(&places, cfg!(target_os = "macos"), None),
            )
        })
        .await?;
        Ok::<_, anyhow::Error>(
            serde_json::json!({"preferences":preferences,"editors":editors,"terminals":terminals}),
        )
    }
    .await;
    result.map_err(|e| e.to_string())
}
#[tauri::command]
pub async fn external_tools(
    app: tauri::AppHandle,
    window: WebviewWindow,
    id: String,
    worktree: Option<String>,
) -> Result<serde_json::Value, String> {
    crate::windows::authorize(&window)?;
    let result = async {
        let sessions = app.state::<crate::workspaces::Sessions>();
        let _work = sessions.gate.read().await;
        app.state::<crate::windows::Windows>()
            .admit(window.label())
            .map_err(anyhow::Error::msg)?;
        let target =
            crate::worktree_tools::tool_target(&app, window.label(), &id, worktree.as_deref())
                .await?;
        let preferences = load(&app.path().app_config_dir()?.join("external-tools.json")).await?;
        let (editors, terminals) = tauri::async_runtime::spawn_blocking(|| {
            let places = Locations::system();
            (
                discover(&places, None),
                terminals::discover(&places, cfg!(target_os = "macos"), None),
            )
        })
        .await?;
        Ok::<_, anyhow::Error>(serde_json::json!({
            "root": target.root.to_string_lossy(),
            "temporary": target.managed(),
            "editors": editors,
            "terminals": terminals,
            "preferences": preferences,
        }))
    }
    .await;
    result.map_err(|error| error.to_string())
}
#[tauri::command]
pub async fn save_external_preferences(
    app: tauri::AppHandle,
    window: WebviewWindow,
    revision: u64,
    terminal: Option<String>,
) -> Result<Preferences, String> {
    crate::windows::authorize(&window)?;
    let result = async {
        let sessions = app.state::<crate::workspaces::Sessions>();
        let _work = sessions.gate.read().await;
        app.state::<crate::windows::Windows>()
            .admit(window.label())
            .map_err(anyhow::Error::msg)?;
        let value = save(
            &app.path().app_config_dir()?.join("external-tools.json"),
            revision,
            terminal,
        )
        .await?;
        let _ = app.emit("external-tool-preferences", value.revision);
        Ok::<_, anyhow::Error>(value)
    }
    .await;
    result.map_err(|error| error.to_string())
}
#[tauri::command]
pub async fn open_external_tool(
    app: tauri::AppHandle,
    window: WebviewWindow,
    id: String,
    worktree: Option<String>,
    kind: Kind,
    tool_id: String,
) -> Result<(), String> {
    crate::windows::authorize(&window)?;
    tauri::async_runtime::spawn(async move {
        let result = async {
            let sessions = app.state::<crate::workspaces::Sessions>();
            let _work = sessions.gate.read().await;
            app.state::<crate::windows::Windows>()
                .admit(window.label())
                .map_err(anyhow::Error::msg)?;
            let target = crate::worktree_tools::tool_target(
                &app, window.label(), &id, worktree.as_deref(),
            ).await?;
            anyhow::ensure!(
                target.temporary.is_none() || target.owned.is_some(),
                "This temporary checkout cannot be retained for an external tool. Refresh its remote source first."
            );
            let tool = tauri::async_runtime::spawn_blocking(move || {
                let places = Locations::system();
                match kind {
                    Kind::Editor => discover(&places, Some(&tool_id)).pop(),
                    Kind::Terminal => terminals::discover(&places, cfg!(target_os = "macos"), Some(&tool_id)).pop(),
                }
            }).await?.context("This tool is no longer available. Refresh the list or install its command-line launcher.")?;
            app.state::<crate::windows::Windows>()
                .admit(window.label())
                .map_err(anyhow::Error::msg)?;
            anyhow::ensure!(!*target.closing.borrow(), "Repository context closed before launch");
            let _external_lease = crate::git_operations::external_lease(&app, &target.root).await.map_err(anyhow::Error::msg)?;
            app.state::<crate::windows::Windows>().admit(window.label()).map_err(anyhow::Error::msg)?;
            anyhow::ensure!(!*target.closing.borrow(), "Repository context closed before launch");
            let mut command = match kind {
                Kind::Editor => super::command(&tool.launcher, &target.root),
                Kind::Terminal => terminals::command(&tool, &target.root),
            };
            let child = command.spawn()
                .map_err(|error| anyhow::anyhow!("Could not launch {}: {error}", tool.name))?;
            if let Some(checkout) = &target.owned {
                checkout.mark_editable();
            }
            super::check_launch(&tool, child).await.map_err(anyhow::Error::msg)
        }.await;
        result.map_err(|error| error.to_string())
    }).await.map_err(|error|error.to_string())?
}
#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn preferences_use_revisions_and_preserve_bad_inputs_without_accepting_commands() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("tools.json");
        let saved = save(&file, 0, Some("kitty".into())).await.unwrap();
        assert_eq!(saved.revision, 1);
        assert!(save(&file, 0, None).await.is_err());
        assert!(save(&file, 1, Some("sh -c unsafe".into())).await.is_err());
        assert_eq!(
            load(&file).await.unwrap().terminal.as_deref(),
            Some("kitty")
        );
        tokio::fs::write(&file, "future or corrupt").await.unwrap();
        assert!(save(&file, 1, None).await.is_err());
        assert_eq!(
            tokio::fs::read_to_string(file).await.unwrap(),
            "future or corrupt"
        );
    }
}
