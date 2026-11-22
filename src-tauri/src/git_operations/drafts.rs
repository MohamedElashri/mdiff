use anyhow::{Context, Result};
use diffle::persist::{with_file_lock, write_file_atomic};
use serde::{Deserialize, Serialize};
use std::path::Path;
use tauri::{Emitter, Manager, WebviewWindow};
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Draft {
    version: u8,
    pub revision: u64,
    pub message: String,
}
async fn read(path: &Path) -> Result<Draft> {
    if !path.exists() {
        return Ok(Draft {
            version: 1,
            revision: 0,
            message: String::new(),
        });
    }
    anyhow::ensure!(
        tokio::fs::metadata(path).await?.len() <= 512 * 1024,
        "Git draft exceeds its limit; original was preserved"
    );
    let draft: Draft = serde_json::from_slice(&tokio::fs::read(path).await?)?;
    anyhow::ensure!(
        draft.version == 1 && draft.message.len() <= 64 * 1024,
        "Unsupported Git draft; original was preserved"
    );
    Ok(draft)
}
async fn save(path: &Path, revision: u64, message: String) -> Result<Draft> {
    anyhow::ensure!(
        message.len() <= 64 * 1024 && !message.contains('\0'),
        "Git draft exceeds its 64 KiB limit"
    );
    with_file_lock(path, || async {
        let previous = read(path).await?;
        anyhow::ensure!(
            previous.revision == revision,
            "Git draft changed in another window. Copy your message before reloading."
        );
        let draft = Draft {
            version: 1,
            revision: revision
                .checked_add(1)
                .context("Draft revision exhausted")?,
            message,
        };
        write_file_atomic(path, &serde_json::to_string(&draft)?).await?;
        Ok(draft)
    })
    .await
}
#[tauri::command]
pub async fn git_operation_draft(
    app: tauri::AppHandle,
    window: WebviewWindow,
    context: String,
    purpose: Option<String>,
    revision: Option<u64>,
    message: Option<String>,
) -> Result<Draft, String> {
    crate::windows::authorize(&window)?;
    let result = async {
        let sessions = app.state::<crate::workspaces::Sessions>();
        let _gate = sessions.gate.read().await;
        // Draft persistence is part of the existing window close flush. Authorization
        // remains required, but closing windows must be able to save metadata.
        anyhow::ensure!(
            !sessions
                .quit_ready
                .load(std::sync::atomic::Ordering::SeqCst),
            "Application shutdown already completed"
        );
        let target = super::target(&app, window.label(), &context).await?;
        let identity = diffle::git::operations::identity(&target.root).await?;
        let mut authority = format!("{:?}:{:?}", identity.git, identity.inodes);
        if let Some(purpose) = purpose {
            anyhow::ensure!(
                purpose.starts_with("resolution/") && purpose.len() <= 8192,
                "Invalid resolution draft key"
            );
            authority.push_str(&purpose);
        }
        let key = crate::storage::digest(&authority);
        let file = app
            .path()
            .app_data_dir()?
            .join("git-drafts-v1")
            .join(format!("{key}.json"));
        if let Some(message) = message {
            let draft = save(&file, revision.context("Missing draft revision")?, message).await?;
            let _ = app.emit(
                "git-draft-changed",
                serde_json::json!({"key":key,"revision":draft.revision}),
            );
            Ok(draft)
        } else {
            read(&file).await
        }
    }
    .await;
    result.map_err(|e: anyhow::Error| e.to_string())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn drafts_survive_failed_saves_and_conflicting_windows() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("draft.json");
        let old = save(&path, 0, "Git draft".into()).await.unwrap();
        assert_eq!(old.revision, 1);
        assert!(save(&path, 0, "other window".into()).await.is_err());
        assert_eq!(read(&path).await.unwrap().message, "Git draft");
        tokio::fs::write(&path, "future data").await.unwrap();
        assert!(save(&path, 1, String::new()).await.is_err());
        assert_eq!(
            tokio::fs::read_to_string(&path).await.unwrap(),
            "future data"
        );
    }
}
