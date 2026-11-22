use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use tauri::Manager;
#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Preferences {
    pub revision: u64,
    pub enabled: bool,
    pub hooks: bool,
    pub codex: Option<String>,
    pub claude: Option<String>,
    pub focus: String,
}
impl Preferences {
    fn validate(&self) -> Result<()> {
        anyhow::ensure!(self.focus.len() <= 4000, "Review focus exceeds 4 KB");
        for path in [&self.codex, &self.claude].into_iter().flatten() {
            anyhow::ensure!(
                path.len() <= 4096 && Path::new(path).is_absolute(),
                "Tool path must be absolute"
            );
        }
        Ok(())
    }
}
pub fn file(app: &tauri::AppHandle) -> Result<PathBuf> {
    Ok(app
        .path()
        .app_config_dir()?
        .join("agent-preferences-v1.json"))
}
pub async fn load(path: &Path) -> Result<Preferences> {
    if !path.exists() {
        return Ok(Preferences::default());
    }
    anyhow::ensure!(
        tokio::fs::metadata(path).await?.len() <= 16384,
        "Agent preferences exceed limit"
    );
    let p: Preferences = serde_json::from_slice(&tokio::fs::read(path).await?)?;
    p.validate()?;
    Ok(p)
}
pub async fn save(path: &Path, revision: u64, mut p: Preferences) -> Result<Preferences> {
    p.validate()?;
    diffle::persist::with_file_lock(path, || async {
        anyhow::ensure!(
            load(path).await?.revision == revision,
            "Agent preferences changed; reload before saving"
        );
        p.revision = revision
            .checked_add(1)
            .context("Preference revision exhausted")?;
        diffle::persist::write_file_atomic(path, &serde_json::to_string(&p)?).await?;
        Ok(p)
    })
    .await
}
pub fn tools(p: &Preferences) -> Vec<(String, PathBuf)> {
    use std::os::unix::fs::PermissionsExt;
    let paths =
        std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default()).collect::<Vec<_>>();
    [("codex", &p.codex), ("claude", &p.claude)]
        .into_iter()
        .filter_map(|(id, configured)| {
            let candidates = if let Some(path) = configured {
                vec![PathBuf::from(path)]
            } else {
                paths.iter().map(|p| p.join(id)).collect()
            };
            candidates
                .into_iter()
                .find(|p| {
                    p.is_file()
                        && p.metadata()
                            .is_ok_and(|m| m.permissions().mode() & 0o111 != 0)
                })
                .and_then(|p| p.canonicalize().ok())
                .map(|p| (id.into(), p))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn opt_in_preferences_use_cas_and_preserve_malformed_files() {
        let temp = tempfile::tempdir().unwrap();
        let file = temp.path().join("prefs");
        assert!(!load(&file).await.unwrap().enabled);
        let p = save(
            &file,
            0,
            Preferences {
                enabled: true,
                ..Default::default()
            },
        )
        .await
        .unwrap();
        assert_eq!(p.revision, 1);
        assert!(save(&file, 0, p.clone()).await.is_err());
        let before = tokio::fs::read(&file).await.unwrap();
        let mut bad = p;
        bad.codex = Some("codex; echo injected".into());
        assert!(save(&file, 1, bad).await.is_err());
        assert_eq!(tokio::fs::read(&file).await.unwrap(), before);
        tokio::fs::write(&file, "{broken").await.unwrap();
        assert!(load(&file).await.is_err());
        assert!(save(&file, 1, Preferences::default()).await.is_err());
        assert_eq!(tokio::fs::read_to_string(&file).await.unwrap(), "{broken");
    }
}
