//! Window-owned worktree reads and external tool targets. No Git write operations.
use anyhow::{bail, Context, Result};
use diffle::git::worktrees::{self, Worktree};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    path::{Path, PathBuf},
    sync::Arc,
};
use tauri::{Manager, State, WebviewWindow};
pub struct Worktrees {
    pub(crate) requests: crate::transport::Requests,
    slots: tokio::sync::Semaphore,
}
impl Default for Worktrees {
    fn default() -> Self {
        Self {
            requests: Default::default(),
            slots: tokio::sync::Semaphore::new(2),
        }
    }
}
pub(crate) struct Target {
    pub root: PathBuf,
    pub owned: Option<Arc<crate::storage::Checkout>>,
    pub temporary: Option<Arc<tempfile::TempDir>>,
    pub closing: tokio::sync::watch::Receiver<bool>,
}
impl Target {
    pub fn managed(&self) -> bool {
        self.owned.is_some() || self.temporary.is_some()
    }
}
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Association {
    pub path: String,
    pub tab: String,
    pub label: String,
    pub workspace: String,
    pub workspace_name: String,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Row {
    id: String,
    path: String,
    head: Option<String>,
    branch: Option<String>,
    detached: bool,
    bare: bool,
    locked: Option<String>,
    prunable: Option<String>,
    current: bool,
    can_open: bool,
    can_inspect: bool,
    reason: Option<String>,
    reviews: Vec<Association>,
}
#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum Query {
    List,
    Status { worktree: String },
    Open { worktree: String },
}
pub(crate) async fn context(app: &tauri::AppHandle, owner: &str, id: &str) -> Result<Target> {
    if id.starts_with("history-") {
        return app
            .state::<crate::repository::Repositories>()
            .tool_target(id, owner)
            .await
            .map_err(anyhow::Error::msg);
    }
    let engine = app.state::<crate::engine::Engine>();
    let runtime = engine
        .owned_runtime(id, owner)
        .await
        .map_err(anyhow::Error::msg)?;
    Ok(Target {
        root: runtime.repository_root().into(),
        owned: runtime.owned_checkout(),
        temporary: runtime.temporary_checkout(),
        closing: runtime.closing(),
    })
}
fn entry_id(context: &str, row: &Worktree) -> String {
    crate::storage::digest(&format!("{context}:{:?}", row.path.as_os_str()))
}
async fn inventory(target: &Target) -> Result<Vec<Worktree>> {
    worktrees::list(&target.root).await
}
async fn same_repository(anchor: &Path, selected: &Path) -> Result<PathBuf> {
    let (_, _, expected) = diffle::git::repository_paths(anchor).await?;
    let (root, _, actual) = diffle::git::repository_paths(selected).await?;
    anyhow::ensure!(
        tokio::fs::canonicalize(expected).await? == tokio::fs::canonicalize(actual).await?,
        "This folder is no longer a worktree of the selected repository. Refresh worktrees."
    );
    Ok(tokio::fs::canonicalize(root).await?)
}
pub(crate) fn protected(app: &tauri::AppHandle, path: &Path) -> bool {
    app.path().app_data_dir().is_ok_and(|root| {
        let canonical = std::fs::canonicalize(&root).unwrap_or_else(|_| root.clone());
        [root, canonical].iter().any(|base| {
            path.starts_with(base.join("reviews")) || path.starts_with(base.join("repositories"))
        })
    })
}
async fn selected(
    app: &tauri::AppHandle,
    target: &Target,
    id: &str,
    key: &str,
) -> Result<Worktree> {
    let row = inventory(target)
        .await?
        .into_iter()
        .find(|row| entry_id(id, row) == key)
        .context("Worktree changed or is no longer registered. Refresh the list.")?;
    anyhow::ensure!(
        !row.bare && row.prunable.is_none(),
        "This entry has no available working folder. Repair it with your Git client, then refresh."
    );
    let path = same_repository(&target.root, &row.path).await?;
    let current = tokio::fs::canonicalize(&target.root).await?;
    anyhow::ensure!(
        !(target.managed() || protected(app, &path)) || path == current,
        "App-managed peer checkouts must be opened from their owning review tab"
    );
    Ok(Worktree { path, ..row })
}
pub(crate) async fn tool_target(
    app: &tauri::AppHandle,
    owner: &str,
    id: &str,
    worktree: Option<&str>,
) -> Result<Target> {
    let mut target = context(app, owner, id).await?;
    if let Some(key) = worktree {
        target.root = selected(app, &target, id, key).await?.path;
    }
    anyhow::ensure!(
        !*target.closing.borrow(),
        "The repository context has closed"
    );
    Ok(target)
}
#[tauri::command]
pub async fn worktree_read(
    app: tauri::AppHandle,
    window: WebviewWindow,
    state: State<'_, Worktrees>,
    id: String,
    request_id: String,
    query: Query,
) -> Result<Value, String> {
    crate::windows::authorize(&window)?;
    if request_id.len() > 100 {
        return Err("Invalid worktree request ID".into());
    }
    let (_request, cancelled) = state.requests.start((window.label().into(), request_id))?;
    let work = async {
        let sessions = app.state::<crate::workspaces::Sessions>();
        let _work = sessions.gate.read().await;
        app.state::<crate::windows::Windows>()
            .admit(window.label())
            .map_err(anyhow::Error::msg)?;
        let _slot = state.slots.acquire().await?;
        let target = context(&app, window.label(), &id).await?;
        let mut closing = target.closing.clone();
        let read = async {
            match query {
                Query::List => {
                    let rows = inventory(&target).await?;
                    let associations = crate::workspaces::worktree_associations(&app).await?;
                    let current = tokio::fs::canonicalize(&target.root).await?;
                    let rows = rows.into_iter().map(|row| {
                        let native = row.path.to_str();
                        let path = row.path.to_string_lossy().into_owned();
                        let is_current = row.path == current;
                        let peer = (target.managed() || protected(&app, &row.path)) && !is_current;
                        let reason = if row.bare {
                            Some("Bare repository: no working folder".into())
                        } else if row.prunable.is_some() {
                            Some("Working folder unavailable; repair externally and refresh".into())
                        } else if peer {
                            Some("App-managed checkout: use its owning review tab".into())
                        } else if native.is_none() {
                            Some("Non-UTF-8 path: external tools are available, but review paths require UTF-8".into())
                        } else if target.managed() {
                            Some("Open downloaded reviews through their saved source tab".into())
                        } else {
                            None
                        };
                        Row {
                            id: entry_id(&id, &row),
                            reviews: associations.iter().filter(|a| a.path == path).cloned().collect(),
                            path,
                            head: row.head,
                            branch: row.branch,
                            detached: row.detached,
                            bare: row.bare,
                            locked: row.locked,
                            prunable: row.prunable.clone(),
                            current: is_current,
                            can_inspect: !row.bare && row.prunable.is_none() && !peer,
                            can_open: reason.is_none(),
                            reason,
                        }
                    }).collect::<Vec<_>>();
                    Ok(
                        json!({"root":target.root.to_string_lossy(),"managed":target.managed(),"items":rows}),
                    )
                }
                Query::Status { worktree } => {
                    let row = selected(&app, &target, &id, &worktree).await?;
                    Ok(serde_json::to_value(worktrees::status(&row.path).await?)?)
                }
                Query::Open { worktree } => {
                    let row = selected(&app, &target, &id, &worktree).await?;
                    anyhow::ensure!(
                        !target.managed() && !protected(&app, &row.path),
                        "App-managed reviews must be opened through their saved source tab"
                    );
                    let path = row.path.to_str().context(
                        "Review paths require UTF-8; open this folder in an external tool",
                    )?;
                    Ok(json!({"path":path}))
                }
            }
        };
        tokio::select! {result=read=>result,_=closing.wait_for(|value|*value)=>{bail!("Repository context closed; worktree read cancelled")}}
    };
    tokio::select! {result=work=>result.map_err(|e:anyhow::Error|e.to_string()),_=cancelled=>Err("Worktree request cancelled".into())}
}
#[tauri::command]
pub async fn worktree_cancel(
    window: WebviewWindow,
    state: State<'_, Worktrees>,
    request_id: String,
) -> Result<(), String> {
    crate::windows::authorize(&window)?;
    if request_id.len() > 100 {
        return Err("Invalid worktree request ID".into());
    }
    state.requests.cancel((window.label().into(), request_id));
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn revalidation_rejects_replaced_or_foreign_worktrees() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("main");
        let foreign = dir.path().join("other");
        let linked = dir.path().join("linked");
        for folder in [&root, &foreign] {
            std::fs::create_dir(folder).unwrap();
            assert!(std::process::Command::new("git")
                .args(["init", "-q"])
                .arg(folder)
                .status()
                .unwrap()
                .success());
        }
        // Unborn checkouts still have a stable, distinguishable common directory.
        assert!(same_repository(&root, &root).await.is_ok());
        assert!(same_repository(&root, &foreign).await.is_err());
        std::os::unix::fs::symlink(&foreign, &linked).unwrap();
        assert!(same_repository(&root, &linked).await.is_err());
    }
    #[test]
    fn cached_entry_ids_are_scoped_to_the_original_native_context() {
        let row = Worktree {
            path: "/tmp/worktree".into(),
            head: None,
            branch: None,
            detached: true,
            bare: false,
            locked: None,
            prunable: None,
        };
        assert_ne!(entry_id("first", &row), entry_id("second", &row));
    }
}
