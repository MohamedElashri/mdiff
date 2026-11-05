use diffle::{
    cli::{
        lsp,
        repository::{open_review_repository, open_review_repository_with},
    },
    git::GitRepo,
    github::{
        client::HttpClient,
        hosts::{choose_instance, Github},
    },
    lsp::pool::LspPool,
    protocol::EnterRequest,
    server::{desktop_router, hub::Hub, ApiDeps},
    session::{Session, SessionOptions},
    user_config::UserConfigStore,
};
use serde::Serialize;
use std::{path::PathBuf, sync::Arc};
use tauri::{Emitter, Manager, State};
use tokio::sync::Mutex;

#[derive(Default)]
pub struct Engine(pub Mutex<Option<ReviewRuntime>>);
pub struct ReviewRuntime {
    pub(crate) config: Arc<UserConfigStore>,
    pub(crate) router: axum::Router,
    pub(crate) id: String,
    pub(crate) gitlab: Option<Arc<crate::gitlab::Gitlab>>,
    events: Option<tokio::task::JoinHandle<()>>,
    hub: Arc<Hub>,
    session: Arc<Session>,
    repo: Arc<GitRepo>,
    lsp: Option<Arc<LspPool>>,
    _clone: Option<tempfile::TempDir>,
}
impl ReviewRuntime {
    pub(crate) fn repository_root(&self) -> &std::path::Path {
        &self.repo.root
    }
    pub(crate) fn temporary_repository(&self) -> bool {
        self._clone.is_some()
    }

    async fn close(mut self) {
        self.hub.close();
        if let Some(events) = self.events.take() {
            events.abort();
        }
        let close_lsp = async {
            if let Some(pool) = &self.lsp {
                pool.close().await;
            }
        };
        self.session.close().await;
        let _ = tokio::time::timeout(std::time::Duration::from_secs(5), async {
            close_lsp.await;
        })
        .await;
        let _ = self.repo.clean_review_refs().await;
    }
}
impl Engine {
    pub async fn close(&self) {
        if let Some(runtime) = self.0.lock().await.take() {
            runtime.close().await;
        }
    }
}
#[derive(Serialize)]
pub struct OpenedReview {
    pub id: String,
    root: String,
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenOptions {
    path: String,
    request: EnterRequest,
    token: Option<String>,
    token_host: Option<String>,
    enable_lsp: bool,
    gitlab_url: Option<String>,
    gitlab_token: Option<String>,
    gitlab_host: Option<String>,
}
#[tauri::command]
pub async fn open_review(
    app: tauri::AppHandle,
    state: State<'_, Engine>,
    options: OpenOptions,
) -> Result<OpenedReview, String> {
    let OpenOptions {
        path,
        request,
        token,
        token_host,
        enable_lsp,
        gitlab_url,
        gitlab_token,
        gitlab_host,
    } = options;
    let mut slot = state.0.lock().await;
    if let Some(previous) = slot.take() {
        previous.close().await;
    }
    let gitlab = match gitlab_url {
        Some(url) => Some(Arc::new(
            crate::gitlab::Gitlab::connect(&url, gitlab_token, gitlab_host.as_deref())
                .await
                .map_err(|e| e.to_string())?,
        )),
        None => None,
    };
    let path = if let Some(gl) = &gitlab {
        gl.prepare(
            app.path()
                .app_data_dir()
                .map_err(|e| e.to_string())?
                .join("repositories"),
        )
        .await
        .map_err(|e| e.to_string())?
    } else {
        PathBuf::from(path)
    };
    let request = if let Some(gl) = &gitlab {
        gl.request()
    } else {
        request
    };
    let result = start(
        app.path()
            .app_config_dir()
            .map_err(|e| e.to_string())?
            .join("review.json"),
        path,
        request,
        token,
        token_host,
        enable_lsp,
        gitlab,
    )
    .await
    .map_err(|e| format!("{e:#}"))?;
    let (mut runtime, opened) = result;
    let mut messages = runtime.hub.subscribe();
    let id = runtime.id.clone();
    runtime.events = Some(tokio::spawn(async move {
        loop {
            let message = match messages.recv().await {
                Ok(message) => message,
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => "resync".into(),
                Err(_) => break,
            };
            if app
                .emit(
                    "review-update",
                    serde_json::json!({"id":id,"message":message}),
                )
                .is_err()
            {
                break;
            }
        }
    }));
    *slot = Some(runtime);
    Ok(opened)
}
async fn start(
    config_path: PathBuf,
    path: PathBuf,
    mut request: EnterRequest,
    token: Option<String>,
    token_host: Option<String>,
    enable_lsp: bool,
    gitlab: Option<Arc<crate::gitlab::Gitlab>>,
) -> anyhow::Result<(ReviewRuntime, OpenedReview)> {
    let config = Arc::new(UserConfigStore::open(config_path).await);
    let settings = config.get();
    let opened = if gitlab.is_some() {
        diffle::cli::repository::Opened {
            repo: Arc::new(GitRepo::open(&path).await?),
            github: Github::tokenless(),
            token_source: None,
            clone: None,
        }
    } else if let Some(token) = token.filter(|t| !t.trim().is_empty()) {
        let selector = match &request {
            EnterRequest::Pr { pr } => pr.clone(),
            _ => None,
        };
        open_review_repository_with(&mut request, &path, async |local| {
            let instance = choose_instance(&settings.github_hosts, local, selector.as_deref()).await;
            // Never send an explicitly entered token to a different Enterprise host.
            if instance.host != token_host.as_deref().unwrap_or("github.com") {
                return Err(diffle::error::GithubError::conflict("This repository uses a different GitHub host. Update the host in Desktop settings before connecting.").into());
            }
            let client = HttpClient::new(token, &instance.api_base_url())?;
            Ok((Github { instance, client: Some(Arc::new(client)) }, Some("mdiff desktop session".into())))
        }).await?
    } else {
        open_review_repository(&mut request, &path, &settings.github_hosts).await?
    };
    let hub = Hub::new();
    let session = Session::new(
        opened.repo.clone(),
        hub.clone(),
        SessionOptions {
            watch: true,
            github: opened.github,
            context: settings.context_lines.clamp(0, 10000) as u32,
            follow_refs: settings.follow_refs,
        },
    );
    let pool = enable_lsp.then(|| lsp::start(&session, &hub, settings.lsp_commands, vec![]));
    static NEXT_ID: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
    let router = desktop_router(ApiDeps {
        session: session.clone(),
        config: config.clone(),
        extra_auto_viewed: vec![],
        hub: hub.clone(),
        lsp: pool.clone(),
    });
    let runtime = ReviewRuntime {
        config: config.clone(),
        router,
        id: NEXT_ID
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed)
            .to_string(),
        gitlab,
        events: None,
        hub,
        session: session.clone(),
        repo: opened.repo,
        lsp: pool.clone(),
        _clone: opened.clone,
    };
    let launch = async {
        let review = session.resolve(request).await?;
        session.start(review).await?;
        Ok::<_, anyhow::Error>(())
    }
    .await;
    match launch {
        Ok(()) => {
            let root = runtime.repo.root.to_string_lossy().into_owned();
            let id = runtime.id.clone();
            Ok((runtime, OpenedReview { id, root }))
        }
        Err(e) => {
            runtime.close().await;
            Err(e)
        }
    }
}
#[tauri::command]
pub async fn close_review(state: State<'_, Engine>) -> Result<(), String> {
    state.close().await;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transport::dispatch;
    #[tokio::test]
    async fn native_review_reads_writes_and_reopens_without_a_server() {
        let dir = tempfile::tempdir().unwrap();
        assert!(std::process::Command::new("git")
            .args(["init", "-q"])
            .arg(dir.path())
            .status()
            .unwrap()
            .success());
        std::fs::write(dir.path().join("example.txt"), "desktop review\n").unwrap();
        let config = dir.path().join("settings/review.json");
        let (runtime, opened) = start(
            config.clone(),
            dir.path().into(),
            EnterRequest::Working,
            None,
            None,
            false,
            None,
        )
        .await
        .unwrap();
        let router = runtime.router.clone();
        let snapshot = dispatch(router.clone(), "GET", "/api/snapshot", None)
            .await
            .unwrap();
        let snapshot: serde_json::Value = serde_json::from_slice(&snapshot.body).unwrap();
        assert_eq!(snapshot["changed"][0]["path"], "example.txt");
        let mut events = runtime.hub.subscribe();
        let added = dispatch(
            router.clone(),
            "POST",
            "/api/threads",
            Some(
                serde_json::json!({"path":"example.txt","body":"Native review comment"})
                    .to_string(),
            ),
        )
        .await
        .unwrap();
        assert!(added.status < 300, "{:?}", added.body);
        assert!(
            tokio::time::timeout(std::time::Duration::from_secs(2), events.recv())
                .await
                .is_ok()
        );
        let forbidden = dispatch(
            router.clone(),
            "GET",
            "/api/file?path=../secret&rev=new",
            None,
        )
        .await
        .unwrap();
        assert!(forbidden.status >= 400);
        let prompt = dispatch(
            router.clone(),
            "GET",
            "/api/threads/export?state=open",
            None,
        )
        .await
        .unwrap();
        assert_eq!(prompt.status, 200);
        let prompt = String::from_utf8(prompt.body.to_vec()).unwrap();
        assert!(prompt.contains("Native review comment"));
        assert!(prompt.contains("example.txt"));
        assert!(
            dispatch(router, "GET", "https://evil.invalid/api/snapshot", None)
                .await
                .is_err()
        );
        runtime.close().await;
        let (runtime, next) = start(
            config,
            dir.path().into(),
            EnterRequest::Working,
            None,
            None,
            false,
            None,
        )
        .await
        .unwrap();
        assert_ne!(opened.id, next.id);
        let comments = dispatch(runtime.router.clone(), "GET", "/api/threads", None)
            .await
            .unwrap();
        assert!(String::from_utf8_lossy(&comments.body).contains("Native review comment"));
        runtime.close().await;
    }
}

#[cfg(test)]
#[path = "desktop_performance.rs"]
mod desktop_performance;
