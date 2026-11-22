use anyhow::{Context, Result};
use diffle::git::operations::{self, Action, Outcome};
use std::path::{Path, PathBuf};
pub struct Prepared {
    pub action: Action,
    pub path: PathBuf,
    pub text: String,
    parent: PathBuf,
    stamp: String,
}
pub async fn prepare(action: Action, app: &tauri::AppHandle) -> Result<Prepared> {
    let (path, url, branch) = match &action {
        Action::Init { path, branch } => (path, None, Some(branch)),
        Action::Clone { path, url } => (path, Some(url), None),
        _ => anyhow::bail!("Not a new repository action"),
    };
    anyhow::ensure!(
        path.len() <= 4096 && !path.contains('\0'),
        "Invalid destination path"
    );
    let target = Path::new(path);
    anyhow::ensure!(target.is_absolute(), "Choose an absolute destination path");
    let name = target.file_name().context("Choose a new folder name")?;
    anyhow::ensure!(!name.eq_ignore_ascii_case(".git"), "Reserved folder name");
    let parent = tokio::fs::canonicalize(target.parent().context("Missing parent folder")?).await?;
    let path = parent.join(name);
    anyhow::ensure!(
        !crate::worktree_tools::protected(app, &path),
        "Choose a user-owned destination outside managed reviews"
    );
    anyhow::ensure!(
        tokio::fs::symlink_metadata(&path)
            .await
            .is_err_and(|e| e.kind() == std::io::ErrorKind::NotFound),
        "Destination already exists or is inaccessible"
    );
    if let Some(url) = url {
        operations::validate_url(url)?;
    }
    if let Some(branch) = branch {
        operations::validate_branch(&parent, branch).await?;
    }
    let stamp = operations::path_fingerprint(&parent).await?;
    let text=format!("{} into {}. The new folder is created exclusively. Existing folders are never reused or removed. Failed/interrupted clones retain their partial files for external recovery. Configured templates/hooks, signing/credential helpers can run without terminal stdin.\n{}",action.label(),path.display(),url.map_or("".into(),|url|format!("Source: {url}")));
    Ok(Prepared {
        action,
        path,
        text,
        parent,
        stamp,
    })
}
pub async fn execute(
    plan: Prepared,
    cancel: tokio::sync::watch::Receiver<bool>,
) -> Result<Outcome> {
    anyhow::ensure!(
        !*cancel.borrow(),
        "Cancelled before reserving a destination"
    );
    anyhow::ensure!(
        operations::path_fingerprint(&plan.parent).await? == plan.stamp,
        "Destination parent changed; preview again"
    );
    tokio::fs::create_dir(&plan.path)
        .await
        .context("Destination changed; refusing to reuse an existing folder")?;
    let args = match &plan.action {
        Action::Init { branch, .. } => vec![
            "init".into(),
            format!("--initial-branch={branch}"),
            "--".into(),
            ".".into(),
        ],
        Action::Clone { url, .. } => vec![
            "clone".into(),
            "--no-recurse-submodules".into(),
            "--".into(),
            url.clone(),
            ".".into(),
        ],
        _ => unreachable!(),
    };
    let exit =
        operations::runner::run(&plan.path, &args, None, None, cancel, plan.action.network())
            .await?;
    let after = operations::inspect(
        &plan.path,
        &Action::Commit {
            message: String::new(),
            amend: false,
        },
    )
    .await
    .ok();
    Ok(Outcome{status:if exit.code==Some(0)&&!exit.interrupted{"completed"}else{"needsInspection"}.into(),message:if exit.code==Some(0)&&!exit.interrupted{"Git completed in the new user-owned folder. Open it explicitly to review; language-server trust remains off."}else{"Git failed or was interrupted. The reserved destination and any partial files were kept. Inspect them externally; nothing was deleted or retried."}.into(),after})
}
#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn destinations_are_exclusive_and_failed_clones_keep_their_owned_folder() {
        let temp = tempfile::tempdir().unwrap();
        let parent = std::fs::canonicalize(temp.path()).unwrap();
        let path = parent.join("new repo 'quoted'\n folder");
        let stamp = operations::path_fingerprint(&parent).await.unwrap();
        let plan = Prepared {
            action: Action::Init {
                path: path.to_str().unwrap().into(),
                branch: "main".into(),
            },
            path: path.clone(),
            parent: parent.clone(),
            stamp,
            text: String::new(),
        };
        let result = execute(plan, tokio::sync::watch::channel(false).1)
            .await
            .unwrap();
        assert_eq!(result.status, "completed");
        assert!(path.join(".git").is_dir());
        let original = path.join("keep");
        tokio::fs::write(&original, "user owned").await.unwrap();
        let plan = Prepared {
            action: Action::Init {
                path: path.to_str().unwrap().into(),
                branch: "main".into(),
            },
            path: path.clone(),
            parent: parent.clone(),
            stamp: operations::path_fingerprint(&parent).await.unwrap(),
            text: String::new(),
        };
        assert!(execute(plan, tokio::sync::watch::channel(false).1)
            .await
            .is_err());
        assert_eq!(
            tokio::fs::read_to_string(original).await.unwrap(),
            "user owned"
        );
        let failed = parent.join("failed clone");
        let plan = Prepared {
            action: Action::Clone {
                path: failed.to_str().unwrap().into(),
                url: parent.join("missing.git").to_str().unwrap().into(),
            },
            path: failed.clone(),
            parent: parent.clone(),
            stamp: operations::path_fingerprint(&parent).await.unwrap(),
            text: String::new(),
        };
        assert_eq!(
            execute(plan, tokio::sync::watch::channel(false).1)
                .await
                .unwrap()
                .status,
            "needsInspection"
        );
        assert!(failed.exists());
    }
}
