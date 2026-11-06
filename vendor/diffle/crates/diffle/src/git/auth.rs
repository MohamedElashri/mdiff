//! Credentials live only in the session and the environment of a network Git command.
use crate::error::GithubError;
use base64::{Engine as _, engine::general_purpose::STANDARD};

#[derive(Clone)]
pub struct GitAuth {
    origin: String,
    header: String,
    token: String,
}

impl GitAuth {
    pub fn github(host: &str, token: &str) -> Result<Self, GithubError> {
        let origin =
            url::Url::parse(&format!("https://{host}/")).map_err(|_| GithubError::conflict("Invalid GitHub host"))?;
        if origin.authority() != host
            || origin.path() != "/"
            || origin.query().is_some()
            || origin.fragment().is_some()
            || !origin.username().is_empty()
            || origin.password().is_some()
        {
            return Err(GithubError::conflict("Invalid GitHub host"));
        }
        Ok(Self {
            origin: origin.into(),
            header: format!("Authorization: Basic {}", STANDARD.encode(format!("x-access-token:{token}"))),
            token: token.into(),
        })
    }

    pub(super) fn apply(&self, cmd: &mut tokio::process::Command) {
        let scoped = format!("http.{}.extraHeader", self.origin);
        let config = [
            ("http.extraHeader", ""),
            (scoped.as_str(), ""),
            (scoped.as_str(), self.header.as_str()),
            ("http.followRedirects", "false"),
            ("credential.helper", ""),
            ("core.hooksPath", "/dev/null"),
        ];
        cmd.env("GIT_CONFIG_COUNT", config.len().to_string());
        for (i, (key, value)) in config.into_iter().enumerate() {
            cmd.env(format!("GIT_CONFIG_KEY_{i}"), key);
            cmd.env(format!("GIT_CONFIG_VALUE_{i}"), value);
        }
        for name in [
            "GIT_TRACE",
            "GIT_TRACE_CURL",
            "GIT_CURL_VERBOSE",
            "GIT_TRACE_PACKET",
            "GIT_TRACE2",
            "GIT_TRACE2_EVENT",
            "GIT_TRACE2_PERF",
        ] {
            cmd.env(name, "0");
        }
        cmd.env("GIT_TERMINAL_PROMPT", "0");
    }

    pub(super) fn redact(&self, message: &str) -> String {
        let encoded = self.header.strip_prefix("Authorization: Basic ").unwrap();
        let message = message.replace(encoded, "[redacted]");
        if self.token.is_empty() { message } else { message.replace(&self.token, "[redacted]") }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rejects_hosts_with_credentials_or_url_components() {
        for host in ["user@github.com", "github.com/path", "github.com?x", "github.com#x", "github.com/"] {
            assert!(GitAuth::github(host, "secret").is_err(), "{host}");
        }
        assert!(GitAuth::github("github.com", "secret").is_ok());
        assert!(GitAuth::github("git.company.test:8443", "secret").is_ok());
    }

    #[test]
    fn scopes_credentials_without_putting_them_in_arguments() {
        let auth = GitAuth::github("git.company.test", "synthetic-secret").unwrap();
        let mut cmd = tokio::process::Command::new("git");
        cmd.args(["fetch", "origin"]);
        auth.apply(&mut cmd);
        let command = cmd.as_std();
        assert_eq!(command.get_args().collect::<Vec<_>>(), ["fetch", "origin"]);
        let env: std::collections::HashMap<_, _> = command
            .get_envs()
            .map(|(k, v)| (k.to_string_lossy().into_owned(), v.unwrap().to_string_lossy().into_owned()))
            .collect();
        assert_eq!(env["GIT_CONFIG_KEY_2"], "http.https://git.company.test/.extraHeader");
        assert_eq!(env["GIT_CONFIG_VALUE_3"], "false");
        assert_eq!(env["GIT_CONFIG_VALUE_4"], "");
        assert_eq!(
            auth.redact(&format!("{} synthetic-secret", auth.header)),
            "Authorization: Basic [redacted] [redacted]"
        );
    }

    #[tokio::test]
    async fn authenticates_clone_and_fetch_without_persisting_or_forwarding_the_token() {
        use crate::git::GitRepo;
        use axum::{
            Router,
            body::Body,
            http::{HeaderMap, StatusCode, Uri},
            response::{IntoResponse, Response},
        };
        use std::sync::{
            Arc,
            atomic::{AtomicUsize, Ordering},
        };

        fn git(root: &std::path::Path, args: &[&str]) -> String {
            let out = std::process::Command::new("git")
                .args([
                    "-c",
                    "commit.gpgsign=false",
                    "-c",
                    "user.name=Audit",
                    "-c",
                    "user.email=audit@example.invalid",
                    "-c",
                    "core.hooksPath=/dev/null",
                ])
                .args(args)
                .current_dir(root)
                .output()
                .unwrap();
            assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
            String::from_utf8_lossy(&out.stdout).trim().into()
        }
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("source");
        std::fs::create_dir(&source).unwrap();
        git(&source, &["init", "-qb", "main"]);
        std::fs::write(source.join("file.txt"), "base\n").unwrap();
        git(&source, &["add", "."]);
        git(&source, &["commit", "-qm", "base"]);
        git(&source, &["update-server-info"]);
        let hits = Arc::new(AtomicUsize::new(0));
        let other_hits = Arc::new(AtomicUsize::new(0));
        let leaked_headers = Arc::new(AtomicUsize::new(0));
        let leaked = leaked_headers.clone();
        let count = other_hits.clone();
        let other = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let other_url = format!("http://{}/", other.local_addr().unwrap());
        let other_origin = other_url.clone();
        let other_server = tokio::spawn(async move {
            axum::serve(
                other,
                Router::new().fallback(move |headers: HeaderMap| {
                    count.fetch_add(1, Ordering::SeqCst);
                    if headers.contains_key("authorization") {
                        leaked.fetch_add(1, Ordering::SeqCst);
                    }
                    async { StatusCode::UNAUTHORIZED }
                }),
            )
            .await
            .unwrap();
        });
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let origin = format!("http://{}/", listener.local_addr().unwrap());
        let mut auth = GitAuth::github("github.com", "synthetic-secret").unwrap();
        // Plain HTTP is only used by this loopback test server.
        auth.origin = origin.clone();
        let expected = auth.header.strip_prefix("Authorization: ").unwrap().to_owned();
        let files = source.join(".git");
        let count = hits.clone();
        let app = Router::new().fallback(move |uri: Uri, headers: HeaderMap| {
            let files = files.clone();
            let expected = expected.clone();
            let count = count.clone();
            let other_url = other_url.clone();
            async move {
                if headers.get("authorization").and_then(|h| h.to_str().ok()) != Some(expected.as_str()) {
                    return StatusCode::UNAUTHORIZED.into_response();
                }
                count.fetch_add(1, Ordering::SeqCst);
                if uri.path().starts_with("/redirect/") {
                    return (StatusCode::FOUND, [("location", other_url)]).into_response();
                }
                let path = uri.path().trim_start_matches('/');
                match tokio::fs::read(files.join(path)).await {
                    Ok(bytes) => {
                        Response::builder().header("content-type", "text/plain").body(Body::from(bytes)).unwrap()
                    }
                    Err(_) => StatusCode::NOT_FOUND.into_response(),
                }
            }
        });
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let cloned = GitRepo::clone_with_auth(
            &origin,
            &{
                let path = dir.path().join("clone");
                std::fs::create_dir(&path).unwrap();
                path
            },
            Some(auth.clone()),
        )
        .await
        .unwrap();
        assert!(hits.load(Ordering::SeqCst) > 1);
        std::fs::write(source.join("file.txt"), "updated\n").unwrap();
        git(&source, &["commit", "-qam", "update"]);
        git(&source, &["update-server-info"]);
        cloned.fetch("origin", &["+refs/heads/main:refs/diffle/test/head".into()]).await.unwrap();
        assert_eq!(cloned.resolve("refs/diffle/test/head").await.unwrap(), git(&source, &["rev-parse", "HEAD"]));
        let config = std::fs::read_to_string(cloned.git_dir.join("config")).unwrap();
        assert!(
            !config.contains("synthetic-secret")
                && !config.contains("extraHeader")
                && !config.contains("Authorization")
        );

        let forbidden = dir.path().join("wrong-host");
        std::fs::create_dir(&forbidden).unwrap();
        let previous = hits.load(Ordering::SeqCst);
        let mut wrong = auth.clone();
        wrong.origin = "https://other.invalid/".into();
        assert!(GitRepo::clone_with_auth(&origin, &forbidden, Some(wrong)).await.is_err());
        assert_eq!(hits.load(Ordering::SeqCst), previous);
        // Git URL rewriting must not forward a host-scoped header either.
        git(&cloned.root, &["config", &format!("url.{other_origin}.insteadOf"), &origin]);
        assert!(cloned.fetch("origin", &["+refs/heads/main:refs/diffle/test/rewritten".into()]).await.is_err());
        assert!(other_hits.load(Ordering::SeqCst) > 0);
        assert_eq!(leaked_headers.load(Ordering::SeqCst), 0);
        let previous_other = other_hits.load(Ordering::SeqCst);
        let redirected = dir.path().join("redirect");
        std::fs::create_dir(&redirected).unwrap();
        assert!(GitRepo::clone_with_auth(&format!("{origin}redirect/"), &redirected, Some(auth)).await.is_err());
        assert_eq!(other_hits.load(Ordering::SeqCst), previous_other);
        server.abort();
        other_server.abort();
    }
}
