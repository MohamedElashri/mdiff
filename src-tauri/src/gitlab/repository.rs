use super::Gitlab;
use anyhow::{bail, Result};
use base64::{engine::general_purpose::STANDARD, Engine as _};
use diffle::git::exec::{exec_git, ExecOptions};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
impl Gitlab {
    /// Persistent, app-owned checkouts preserve review comments between launches.
    pub async fn prepare(&self, cache: PathBuf) -> Result<PathBuf> {
        let key = format!(
            "{:x}",
            Sha256::digest(format!("{}{}!{}", self.origin, self.project, self.iid))
        );
        let path = cache.join(key);
        tokio::fs::create_dir_all(&path).await?;
        if !path.join(".git").exists() {
            self.git(&path, &["init", "-q"]).await?;
        }
        let mut remote = self.origin.clone();
        remote.set_path(&format!("/{}.git", self.project));
        // Only our own checkout is touched, with the remote derived from the bound origin.
        let head = self.metadata["diff_refs"]["head_sha"].as_str().unwrap();
        let base = self.metadata["diff_refs"]["base_sha"].as_str().unwrap();
        self.git(
            &path,
            &[
                "fetch",
                "--no-tags",
                remote.as_str(),
                &format!("refs/merge-requests/{}/head", self.iid),
                base,
            ],
        )
        .await?;
        self.git(&path, &["cat-file", "-e", &format!("{head}^{{commit}}")])
            .await?;
        self.git(&path, &["checkout", "--detach", head]).await?;
        // Stable comparison names keep comment storage and iterations attached to this MR.
        self.git(&path, &["update-ref", "refs/mdiff/base", base])
            .await?;
        self.git(&path, &["update-ref", "refs/mdiff/head", head])
            .await?;
        Ok(path)
    }
    async fn git(&self, path: &Path, args: &[&str]) -> Result<()> {
        let auth = self
            .token
            .as_ref()
            .map(|t| {
                format!(
                    "Authorization: Basic {}",
                    STANDARD.encode(format!("oauth2:{t}"))
                )
            })
            .unwrap_or_default();
        let key = format!("http.{}.extraHeader", self.origin);
        let env = [
            ("GIT_TERMINAL_PROMPT", "0"),
            ("GIT_TRACE", "0"),
            ("GIT_TRACE_CURL", "0"),
            ("GIT_CURL_VERBOSE", "0"),
            ("GIT_CONFIG_COUNT", "4"),
            ("GIT_CONFIG_KEY_0", key.as_str()),
            ("GIT_CONFIG_VALUE_0", auth.as_str()),
            ("GIT_CONFIG_KEY_1", "http.followRedirects"),
            ("GIT_CONFIG_VALUE_1", "false"),
            ("GIT_CONFIG_KEY_2", "credential.helper"),
            ("GIT_CONFIG_VALUE_2", ""),
            ("GIT_CONFIG_KEY_3", "core.hooksPath"),
            ("GIT_CONFIG_VALUE_3", "/dev/null"),
        ];
        let result = tokio::time::timeout(
            std::time::Duration::from_secs(180),
            exec_git(
                path,
                args,
                ExecOptions {
                    env: &env,
                    ..Default::default()
                },
            ),
        )
        .await;
        match result { Ok(Ok(_))=>Ok(()), Ok(Err(_))=>bail!("Git could not prepare the GitLab checkout. Check repository access and token read_repository permission."),Err(_)=>bail!("GitLab repository fetch timed out") }
    }
}
