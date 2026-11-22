//! Native authority for desktop appearance and shortcut preferences. No credentials here.
use anyhow::{Context, Result};
use diffle::persist::{with_file_lock, write_file_atomic};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::Path;
use tauri::{Emitter, Manager, WebviewWindow};
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Preferences {
    pub version: u8,
    pub revision: u64,
    pub appearance: Value,
    pub shortcuts: Value,
    pub theme: String,
}
impl Preferences {
    fn validate(&self) -> Result<()> {
        anyhow::ensure!(
            self.version == 1 && ["system", "light", "dark"].contains(&self.theme.as_str()),
            "Invalid desktop preference version or theme"
        );
        let appearance = self
            .appearance
            .as_object()
            .context("Invalid appearance preferences")?;
        anyhow::ensure!(
            appearance.len() <= 10
                && appearance.iter().all(|(k, v)| {
                    [
                        "density",
                        "light",
                        "dark",
                        "appFont",
                        "codeFont",
                        "appSize",
                        "codeSize",
                        "codeWeight",
                        "ligatures",
                    ]
                    .contains(&k.as_str())
                        && (v.as_str().is_some_and(|s| s.len() <= 80)
                            || v.is_boolean()
                            || v.as_f64().is_some_and(|n| {
                                n.is_finite() && (-10000.0..=10000.0).contains(&n)
                            }))
                }),
            "Invalid appearance preferences"
        );
        let shortcuts = self
            .shortcuts
            .as_object()
            .context("Invalid shortcut preferences")?;
        anyhow::ensure!(
            shortcuts.len() <= 256
                && shortcuts.iter().all(|(k, v)| k.len() <= 100
                    && (k.starts_with("app.") || k.starts_with("review."))
                    && (v.is_null() || v.as_str().is_some_and(|s| s.len() <= 100))),
            "Invalid shortcut preferences"
        );
        Ok(())
    }
}
#[tauri::command]
pub async fn shell_preferences(
    app: tauri::AppHandle,
    window: WebviewWindow,
    legacy: Preferences,
) -> Result<Preferences, String> {
    crate::windows::authorize(&window)?;
    let sessions = app.state::<crate::workspaces::Sessions>();
    let _work = sessions.gate.read().await;
    app.state::<crate::windows::Windows>()
        .allow_save(window.label())?;
    let file = app
        .path()
        .app_config_dir()
        .map_err(|e| e.to_string())?
        .join("shell-preferences.json");
    load(&file, legacy).await.map_err(|e| format!("{e:#}"))
}
async fn load(file: &Path, legacy: Preferences) -> Result<Preferences> {
    with_file_lock(file, || async {
        if file.exists() {
            anyhow::ensure!(
                tokio::fs::metadata(file).await?.len() <= 64 * 1024,
                "Preference file exceeds its limit"
            );
            let value: Preferences = serde_json::from_slice(&tokio::fs::read(file).await?)?;
            value.validate()?;
            return Ok(value);
        }
        legacy.validate()?;
        let mut value = legacy;
        value.revision = 0;
        write_file_atomic(file, &serde_json::to_string(&value)?).await?;
        Ok(value)
    })
    .await
}

#[tauri::command]
pub async fn save_shell_preferences(
    app: tauri::AppHandle,
    window: WebviewWindow,
    revision: u64,
    preferences: Preferences,
) -> Result<Preferences, String> {
    crate::windows::authorize(&window)?;
    let sessions = app.state::<crate::workspaces::Sessions>();
    let _work = sessions.gate.read().await;
    app.state::<crate::windows::Windows>()
        .allow_save(window.label())?;
    let file = app
        .path()
        .app_config_dir()
        .map_err(|e| e.to_string())?
        .join("shell-preferences.json");
    let value = save(&file, revision, preferences)
        .await
        .map_err(|e| format!("{e:#}"))?;
    let _ = app.emit("shell-preferences", value.revision);
    Ok(value)
}

async fn save(file: &Path, revision: u64, preferences: Preferences) -> Result<Preferences> {
    with_file_lock(file, || async {
        anyhow::ensure!(
            tokio::fs::metadata(file).await?.len() <= 64 * 1024,
            "Preference file exceeds its limit"
        );
        let old: Preferences = serde_json::from_slice(&tokio::fs::read(file).await?)?;
        old.validate()?;
        anyhow::ensure!(
            old.revision == revision,
            "Desktop preferences changed in another process; reload preferences before saving"
        );
        preferences.validate()?;
        let mut value = preferences;
        value.revision = revision
            .checked_add(1)
            .context("Preference revision exhausted")?;
        write_file_atomic(file, &serde_json::to_string(&value)?).await?;
        Ok(value)
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    fn preferences() -> Preferences {
        Preferences {
            version: 1,
            revision: 0,
            appearance: serde_json::json!({}),
            shortcuts: serde_json::json!({}),
            theme: "system".into(),
        }
    }
    #[tokio::test]
    async fn preference_writers_use_revisions_and_preserve_corrupt_or_invalid_inputs() {
        let root = tempfile::tempdir().unwrap();
        let file = root.path().join("preferences.json");
        load(&file, preferences()).await.unwrap();
        let (a, b) = tokio::join!(save(&file, 0, preferences()), save(&file, 0, preferences()));
        assert_ne!(a.is_ok(), b.is_ok());
        assert_eq!(load(&file, preferences()).await.unwrap().revision, 1);
        let bytes = std::fs::read(&file).unwrap();
        let mut invalid = preferences();
        invalid.theme = "invalid".into();
        assert!(save(&file, 1, invalid).await.is_err());
        assert_eq!(std::fs::read(&file).unwrap(), bytes);
        std::fs::write(&file, "{broken").unwrap();
        assert!(load(&file, preferences()).await.is_err());
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "{broken");
    }
}
