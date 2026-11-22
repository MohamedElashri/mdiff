use super::Gitlab;
use anyhow::{bail, Result};
use base64::{engine::general_purpose::STANDARD, Engine as _};
use diffle::git::exec::{exec_git, ExecOptions};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
impl Gitlab {
    pub fn cached_path(&self, cache: &Path) -> PathBuf {
        cache.join(format!(
            "{:x}",
            Sha256::digest(format!("{}{}!{}", self.origin, self.project, self.iid))
        ))
    }
    /// Prepare a caller-owned staging checkout. Live reviews never share this mutable path.
    pub async fn prepare(&self, cache: PathBuf) -> Result<PathBuf> {
        let path = self.cached_path(&cache);
        tokio::fs::create_dir_all(&path).await?;
        if !path.join(".git").exists() {
            self.git(&path, &["init", "-q"]).await?;
        }
        let mut remote = self.origin.clone();
        remote.set_path(&format!("/{}.git", self.project));
        self.git(&path, &["config", "remote.origin.url", remote.as_str()])
            .await?;
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
        let env = git_environment(&key, &auth);
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

fn git_environment<'a>(key: &'a str, auth: &'a str) -> Vec<(&'a str, &'a str)> {
    vec![
        ("GIT_TERMINAL_PROMPT", "0"),
        ("GIT_TRACE", "0"),
        ("GIT_TRACE_CURL", "0"),
        ("GIT_CURL_VERBOSE", "0"),
        ("GIT_TRACE_PACKET", "0"),
        ("GIT_TRACE2", "0"),
        ("GIT_TRACE2_EVENT", "0"),
        ("GIT_TRACE2_PERF", "0"),
        ("GIT_CONFIG_COUNT", "4"),
        ("GIT_CONFIG_KEY_0", key),
        ("GIT_CONFIG_VALUE_0", auth),
        ("GIT_CONFIG_KEY_1", "http.followRedirects"),
        ("GIT_CONFIG_VALUE_1", "false"),
        ("GIT_CONFIG_KEY_2", "credential.helper"),
        ("GIT_CONFIG_VALUE_2", ""),
        ("GIT_CONFIG_KEY_3", "core.hooksPath"),
        ("GIT_CONFIG_VALUE_3", "/dev/null"),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn inherited_trace2_cannot_record_the_gitlab_authentication_header() {
        let dir = tempfile::tempdir().unwrap();
        assert!(std::process::Command::new("git")
            .args(["init", "-q"])
            .arg(dir.path())
            .status()
            .unwrap()
            .success());
        let trace = dir.path().join("trace.json");
        let auth = "Authorization: Basic mdiff-test-only-secret";
        let output = std::process::Command::new("git")
            .current_dir(dir.path())
            .args(["status", "--porcelain"])
            .env("GIT_TRACE2_EVENT", &trace)
            .env("GIT_TRACE2_CONFIG_PARAMS", "*")
            .env("GIT_TRACE2_ENV_VARS", "GIT_CONFIG_VALUE_0")
            .envs(git_environment(
                "http.https://gitlab.example/.extraHeader",
                auth,
            ))
            .output()
            .unwrap();
        assert!(output.status.success());
        let trace = std::fs::read_to_string(trace).unwrap_or_default();
        assert!(
            !trace.contains(auth),
            "GitLab authorization was written to a Git trace"
        );
        assert!(
            trace.is_empty(),
            "Network Git operations must disable inherited diagnostic tracing"
        );
    }
}
