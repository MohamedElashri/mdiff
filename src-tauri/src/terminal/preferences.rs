use anyhow::{Context, Result};
use diffle::persist::{with_file_lock, write_file_atomic};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use tauri::{Emitter, Manager, WebviewWindow};
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Preferences {
    version: u8,
    pub revision: u64,
    pub enabled: bool,
    pub font_size: u8,
    pub height: u16,
}
impl Default for Preferences {
    fn default() -> Self {
        Self {
            version: 1,
            revision: 0,
            enabled: true,
            font_size: 13,
            height: 280,
        }
    }
}
impl Preferences {
    fn validate(&self) -> Result<()> {
        anyhow::ensure!(
            self.version == 1
                && (10..=24).contains(&self.font_size)
                && (160..=600).contains(&self.height),
            "Invalid terminal preferences"
        );
        Ok(())
    }
}
pub fn file(app: &tauri::AppHandle) -> Result<PathBuf> {
    Ok(app
        .path()
        .app_config_dir()?
        .join("terminal-preferences-v1.json"))
}
pub async fn load(path: &Path) -> Result<Preferences> {
    if !path.exists() {
        return Ok(Preferences::default());
    }
    anyhow::ensure!(
        tokio::fs::metadata(path).await?.len() <= 4096,
        "Terminal preferences exceed their limit"
    );
    let value: Preferences = serde_json::from_slice(&tokio::fs::read(path).await?)?;
    value.validate()?;
    Ok(value)
}
async fn save(path: &Path, revision: u64, mut value: Preferences) -> Result<Preferences> {
    value.validate()?;
    with_file_lock(path, || async {
        let old = load(path).await?;
        anyhow::ensure!(
            old.revision == revision,
            "Terminal preferences changed in another window; reload before saving"
        );
        value.revision = revision
            .checked_add(1)
            .context("Terminal preference revision exhausted")?;
        write_file_atomic(path, &serde_json::to_string(&value)?).await?;
        Ok(value)
    })
    .await
}
#[tauri::command]
pub async fn terminal_preferences(
    app: tauri::AppHandle,
    window: WebviewWindow,
    revision: Option<u64>,
    preferences: Option<Preferences>,
) -> Result<Preferences, String> {
    crate::windows::authorize(&window)?;
    let sessions = app.state::<crate::workspaces::Sessions>();
    let _gate = sessions.gate.read().await;
    app.state::<crate::windows::Windows>()
        .allow_save(window.label())?;
    let result = async {
        let path = file(&app)?;
        if let Some(value) = preferences {
            let value = save(
                &path,
                revision.context("Missing preference revision")?,
                value,
            )
            .await?;
            let _ = app.emit("terminal-preferences-changed", value.revision);
            Ok(value)
        } else {
            load(&path).await
        }
    }
    .await;
    result.map_err(|e: anyhow::Error| e.to_string())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn preferences_are_bounded_revisioned_and_preserve_invalid_originals() {
        let tmp = tempfile::tempdir().unwrap();
        let file = tmp.path().join("preferences.json");
        let value = save(&file, 0, Preferences::default()).await.unwrap();
        assert_eq!(value.revision, 1);
        assert!(save(&file, 0, value).await.is_err());
        tokio::fs::write(&file, "future data").await.unwrap();
        assert!(save(&file, 1, Preferences::default()).await.is_err());
        assert_eq!(
            tokio::fs::read_to_string(&file).await.unwrap(),
            "future data"
        );
    }
}
