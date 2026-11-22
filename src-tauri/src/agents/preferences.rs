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
    pub(super) fn validate(&self) -> Result<()> {
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
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Tool {
    pub id: String,
    pub path: Option<PathBuf>,
    pub source: &'static str,
    pub error: Option<String>,
}
fn executable(path: &Path) -> Result<PathBuf> {
    use std::os::unix::fs::PermissionsExt;
    anyhow::ensure!(path.is_absolute(), "Choose an absolute executable path");
    let metadata = path.metadata().context("File not found or inaccessible")?;
    anyhow::ensure!(
        metadata.is_file() && metadata.permissions().mode() & 0o111 != 0,
        "Choose an executable file, not a folder or non-executable file"
    );
    Ok(path.canonicalize()?)
}
fn discover_in(p: &Preferences, paths: &[PathBuf]) -> Vec<Tool> {
    [("codex", &p.codex), ("claude", &p.claude)].into_iter().map(|(id, configured)| {
        let source = if configured.is_some() { "configured" } else { "automatic" };
        let result = if let Some(path) = configured {
            executable(Path::new(path))
        } else {
            paths.iter().find_map(|dir| executable(&dir.join(id)).ok())
                .context("Not found. Choose the installed executable or add it to PATH, then check again.")
        };
        match result {
            Ok(path) => Tool { id: id.into(), path: Some(path), source, error: None },
            Err(error) => Tool { id: id.into(), path: configured.as_ref().map(PathBuf::from), source, error: Some(error.to_string()) },
        }
    }).collect()
}
pub fn discover(p: &Preferences) -> Vec<Tool> {
    let mut paths =
        std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default()).collect::<Vec<_>>();
    // GUI-launched apps may not inherit package manager/user CLI directories.
    for path in ["/opt/homebrew/bin", "/usr/local/bin", "/usr/bin"] {
        let path = PathBuf::from(path);
        if !paths.contains(&path) {
            paths.push(path);
        }
    }
    if let Some(home) = std::env::var_os("HOME") {
        let home = PathBuf::from(home);
        for relative in [
            ".local/bin",
            ".cargo/bin",
            ".bun/bin",
            ".npm-global/bin",
            ".volta/bin",
        ] {
            paths.push(home.join(relative));
        }
    }
    discover_in(p, &paths)
}
pub fn tools(p: &Preferences) -> Vec<(String, PathBuf)> {
    discover(p)
        .into_iter()
        .filter(|t| t.error.is_none())
        .filter_map(|t| t.path.map(|path| (t.id, path)))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn discovery_reports_both_tools_and_never_substitutes_an_invalid_override() {
        use std::os::unix::fs::PermissionsExt;
        let temp = tempfile::tempdir().unwrap();
        let codex = temp.path().join("codex");
        std::fs::write(&codex, "#!/bin/sh\nexit 99\n").unwrap();
        std::fs::set_permissions(&codex, std::fs::Permissions::from_mode(0o700)).unwrap();
        let paths = vec![temp.path().to_owned()];
        let tools = discover_in(&Preferences::default(), &paths);
        assert!(tools[0].error.is_none());
        assert!(tools[1].error.is_some());
        let p = Preferences {
            codex: Some(temp.path().join("missing").to_string_lossy().into_owned()),
            ..Default::default()
        };
        let tools = discover_in(&p, &paths);
        assert!(tools[0].error.as_ref().unwrap().contains("not found"));
        assert_eq!(tools[0].source, "configured");
    }
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
