//! Desktop GitHub workflow. Uses the review's host-bound client and in-process IPC router.
use anyhow::{bail, Context};
use axum::{extract::State, http::StatusCode, response::IntoResponse, routing::get, Json, Router};
use diffle::{
    git::GitRepo,
    github::{
        client::GithubClient,
        hosts::{github_repository, Github},
        pulls::pull_requests_for_head,
    },
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    path::Path,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::Duration,
};

const TIMEOUT: Duration = Duration::from_secs(20);

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Connections {
    pub github_enabled: bool,
}
impl Default for Connections {
    fn default() -> Self {
        Self {
            github_enabled: true,
        }
    }
}
impl Connections {
    pub async fn load(config: &Path) -> anyhow::Result<Self> {
        match tokio::fs::read(config.with_file_name("connections.json")).await {
            Ok(bytes) => {
                Ok(serde_json::from_slice(&bytes).context("Cannot read GitHub preference")?)
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(e) => Err(e.into()),
        }
    }
    pub async fn save(&self, config: &Path) -> anyhow::Result<()> {
        diffle::persist::write_file_atomic(
            &config.with_file_name("connections.json"),
            &serde_json::to_string_pretty(self)?,
        )
        .await
    }
}

/// Wrap the client itself so internal engine operations also respect a live disable.
struct GuardedClient {
    inner: Arc<dyn GithubClient>,
    enabled: Arc<AtomicBool>,
}
impl GithubClient for GuardedClient {
    fn git_token(&self) -> Option<&str> {
        self.inner.git_token()
    }
    fn graphql(
        &self,
        query: &str,
        variables: Value,
        timeout: Option<Duration>,
    ) -> std::pin::Pin<
        Box<
            dyn std::future::Future<Output = Result<Value, diffle::error::GithubError>> + Send + '_,
        >,
    > {
        if !self.enabled.load(Ordering::SeqCst) {
            return Box::pin(async {
                Err(diffle::error::GithubError::conflict(
                    "GitHub integration is disabled in Preferences → Connections.",
                ))
            });
        }
        self.inner.graphql(query, variables, timeout)
    }
}
pub fn guarded(mut github: Github, enabled: Arc<AtomicBool>) -> Github {
    github.client = github
        .client
        .map(|inner| Arc::new(GuardedClient { inner, enabled }) as Arc<dyn GithubClient>);
    github
}

pub struct Integration {
    pub enabled: Arc<AtomicBool>,
    github: Github,
    repo: Arc<GitRepo>,
    writes: tokio::sync::Mutex<()>,
}
#[derive(Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct CreateContext {
    connected: bool,
    host: String,
    repository: Option<String>,
    head_repository: Option<String>,
    branch: Option<String>,
    head: Option<String>,
    base: Option<String>,
    title: Option<String>,
    reason: Option<String>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CreateRequest {
    repository: String,
    head_repository: String,
    branch: String,
    head: String,
    base: String,
    title: String,
    body: String,
    draft: bool,
}

impl Integration {
    pub fn new(github: Github, repo: Arc<GitRepo>, enabled: Arc<AtomicBool>) -> Arc<Self> {
        Arc::new(Self {
            github,
            repo,
            enabled,
            writes: Default::default(),
        })
    }
    fn client(&self) -> anyhow::Result<&dyn GithubClient> {
        if !self.enabled.load(Ordering::SeqCst) {
            bail!("GitHub integration is disabled in Preferences → Connections.");
        }
        self.github
            .client
            .as_deref()
            .context("Connect GitHub in Preferences → Connections, then reopen this review.")
    }
    async fn query(&self, query: &str, variables: Value) -> anyhow::Result<Value> {
        Ok(self
            .client()?
            .graphql(query, variables, Some(TIMEOUT))
            .await?)
    }
    async fn repository(&self, repository: &str, branch: Option<&str>) -> anyhow::Result<Value> {
        let (owner, name) = repository
            .split_once('/')
            .filter(|(o, n)| !o.is_empty() && !n.is_empty() && !n.contains('/'))
            .context("Enter a repository as owner/name.")?;
        let data = self.query(
            "query($owner:String!,$name:String!,$ref:String!){repository(owner:$owner,name:$name){id defaultBranchRef{name} ref(qualifiedName:$ref){target{oid}}}}",
            json!({"owner":owner,"name":name,"ref":format!("refs/heads/{}", branch.unwrap_or(""))}),
        ).await?;
        data.get("repository").filter(|v| v.get("id").and_then(Value::as_str).is_some()).cloned().context("GitHub could not find that repository. Check your connection and repository access.")
    }
    async fn context(&self) -> anyhow::Result<CreateContext> {
        let mut context = CreateContext {
            connected: self.github.client.is_some(),
            host: self.github.instance.host.clone(),
            repository: None,
            head_repository: None,
            branch: None,
            head: None,
            base: None,
            title: None,
            reason: None,
        };
        if self.client().is_err() {
            context.reason = Some(
                "Connect GitHub in Preferences → Connections, then reopen this review.".into(),
            );
            return Ok(context);
        }
        let remotes = self.repo.remotes().await?;
        let repositories: Vec<_> = remotes
            .iter()
            .filter_map(|r| {
                github_repository(&r.url, &self.github.instance)
                    .map(|repository| (r.name.as_str(), repository))
            })
            .collect();
        let repository = repositories.iter().find(|(name, _)| *name == "origin")
            .or_else(|| (repositories.len() == 1).then(|| &repositories[0]))
            .map(|(_, repository)| repository.clone())
            .context("No unambiguous GitHub remote. Add an origin on this GitHub host before creating a PR.")?;
        context.repository = Some(repository.clone());
        let base = self.repository(&repository, None).await?;
        context.base = base
            .pointer("/defaultBranchRef/name")
            .and_then(Value::as_str)
            .map(str::to_owned);
        let Some(local) = self.repo.branch_ref("HEAD").await? else {
            context.reason =
                Some("Check out a local branch before creating a pull request.".into());
            return Ok(context);
        };
        context.title = self
            .repo
            .commit_info("HEAD")
            .await
            .ok()
            .flatten()
            .map(|c| c.message.lines().next().unwrap_or_default().to_owned());
        let Some(upstream) = self.repo.upstream_branch("HEAD").await? else {
            context.reason = Some("Push this branch and set its upstream in Git, then refresh. Only pushed commits are included in a PR.".into());
            return Ok(context);
        };
        let head_repository = remotes.iter().find(|r| r.name == upstream.remote).and_then(|r| github_repository(&r.url, &self.github.instance)).context("This branch's upstream is on another host. Choose a branch pushed to this GitHub host.")?;
        let head = self.repo.resolve(&local).await?;
        let remote = self
            .repository(&head_repository, Some(&upstream.branch))
            .await?;
        if remote.pointer("/ref/target/oid").and_then(Value::as_str) != Some(head.as_str()) {
            context.reason = Some("The GitHub branch differs from your local HEAD. Push or sync your branch, then refresh.".into());
        }
        context.head_repository = Some(head_repository);
        context.branch = Some(upstream.branch);
        context.head = Some(head);
        Ok(context)
    }
    async fn create(&self, request: CreateRequest) -> anyhow::Result<Value> {
        let _write = self.writes.lock().await;
        if request.title.trim().is_empty()
            || request.title.len() > 256
            || request.body.len() > 65536
            || request.base.trim().is_empty()
        {
            bail!("Enter a title (up to 256 bytes), base branch, and description (up to 64 KiB).");
        }
        // Validate the checked-out branch again; the form may have been open across a Git switch.
        let current = self.context().await?;
        if let Some(reason) = current.reason {
            bail!("{reason}");
        }
        if current.branch.as_deref() != Some(&request.branch)
            || current.head.as_deref() != Some(&request.head)
            || current.head_repository.as_deref() != Some(&request.head_repository)
        {
            bail!("Your branch changed. Refresh before creating the pull request.");
        }
        if request.repository == request.head_repository && request.base == request.branch {
            bail!("Choose a base branch different from the head branch.");
        }
        let target = self
            .repository(&request.repository, Some(&request.base))
            .await?;
        if target.pointer("/ref/target/oid").is_none() {
            bail!("The base branch does not exist on GitHub.");
        }
        let owner = request
            .head_repository
            .split('/')
            .next()
            .unwrap_or_default();
        let existing = pull_requests_for_head(
            self.client()?,
            &request.repository,
            &request.branch,
            Some(&request.base),
            Some(TIMEOUT),
        )
        .await?;
        if let Some(pr) = existing
            .iter()
            .find(|pr| pr.head_repository.as_deref() == Some(&request.head_repository))
        {
            return Ok(json!({"url":pr.url,"number":pr.number,"existing":true}));
        }
        let head_repository = self
            .repository(&request.head_repository, Some(&request.branch))
            .await?;
        if head_repository
            .pointer("/ref/target/oid")
            .and_then(Value::as_str)
            != Some(&request.head)
            || self.repo.resolve("HEAD").await? != request.head
        {
            bail!("Your branch changed. Refresh before creating the pull request.");
        }
        let data = self.query(
            "mutation($input:CreatePullRequestInput!){createPullRequest(input:$input){pullRequest{url number}}}",
            json!({"input":{"repositoryId":target["id"],"headRepositoryId":head_repository["id"],"baseRefName":request.base,"headRefName":format!("{owner}:{}",request.branch),"title":request.title.trim(),"body":request.body,"draft":request.draft}}),
        ).await?;
        let pr = data
            .pointer("/createPullRequest/pullRequest")
            .filter(|p| p["url"].is_string() && p["number"].is_number())
            .context("GitHub did not return a pull request. Check GitHub before trying again.")?;
        Ok(json!({"url":pr["url"],"number":pr["number"],"existing":false}))
    }
    pub fn routes(self: &Arc<Self>) -> Router {
        Router::new()
            .route("/api/desktop/github", get(context).post(create))
            .with_state(self.clone())
    }
}
fn failure(error: anyhow::Error) -> axum::response::Response {
    (
        StatusCode::CONFLICT,
        Json(json!({"error":error.to_string()})),
    )
        .into_response()
}
async fn context(State(integration): State<Arc<Integration>>) -> axum::response::Response {
    match integration.context().await {
        Ok(value) => Json(value).into_response(),
        Err(error) => failure(error),
    }
}
async fn create(
    State(integration): State<Arc<Integration>>,
    Json(request): Json<CreateRequest>,
) -> axum::response::Response {
    match integration.create(request).await {
        Ok(value) => Json(value).into_response(),
        Err(error) => failure(error),
    }
}

pub fn router(router: Router, integration: &Arc<Integration>) -> Router {
    router
        .merge(integration.routes())
        .layer(axum::middleware::from_fn_with_state(
            integration.enabled.clone(),
            gate,
        ))
}

async fn gate(
    State(enabled): State<Arc<AtomicBool>>,
    request: axum::extract::Request,
    next: axum::middleware::Next,
) -> axum::response::Response {
    if enabled.load(Ordering::SeqCst) {
        return next.run(request).await;
    }
    let path = request.uri().path();
    let mut blocked = path == "/api/github"
        || path.starts_with("/api/github/")
        || path == "/api/permalink"
        || path.starts_with("/api/desktop/github");
    let request = if path == "/api/review" {
        let (parts, body) = request.into_parts();
        let Ok(bytes) = axum::body::to_bytes(body, 1024 * 1024).await else {
            return StatusCode::PAYLOAD_TOO_LARGE.into_response();
        };
        blocked |= serde_json::from_slice::<Value>(&bytes)
            .ok()
            .is_some_and(|v| v["kind"] == "pr");
        axum::extract::Request::from_parts(parts, axum::body::Body::from(bytes))
    } else {
        request
    };
    if blocked {
        return (
            StatusCode::FORBIDDEN,
            Json(json!({"error":"GitHub integration is disabled in Preferences → Connections."})),
        )
            .into_response();
    }
    next.run(request).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transport::dispatch;
    use std::sync::Mutex;

    #[derive(Default)]
    struct Fake {
        oid: Mutex<String>,
        calls: Mutex<Vec<(String, Value)>>,
        existing: AtomicBool,
    }
    impl GithubClient for Fake {
        fn graphql(
            &self,
            query: &str,
            variables: Value,
            _: Option<Duration>,
        ) -> std::pin::Pin<
            Box<
                dyn std::future::Future<Output = Result<Value, diffle::error::GithubError>>
                    + Send
                    + '_,
            >,
        > {
            self.calls
                .lock()
                .unwrap()
                .push((query.into(), variables.clone()));
            let oid = self.oid.lock().unwrap().clone();
            let data = if query.contains("mutation") {
                json!({"createPullRequest":{"pullRequest":{"url":"https://github.com/upstream/project/pull/7","number":7}}})
            } else if query.contains("pullRequests(") {
                let nodes = if self.existing.load(Ordering::SeqCst) {
                    json!([{"number":3,"url":"https://github.com/upstream/project/pull/3","title":"Already open","state":"OPEN","isDraft":true,"baseRefOid":oid,"baseRefName":"main","headRefName":"feature","headRefOid":oid,"headRepository":{"name":"project","owner":{"login":"user"}}}])
                } else {
                    json!([])
                };
                json!({"repository":{"pullRequests":{"nodes":nodes}}})
            } else {
                json!({"repository":{"id":format!("{}/{}",variables["owner"].as_str().unwrap(),variables["name"].as_str().unwrap()),"defaultBranchRef":{"name":"main"},"ref":{"target":{"oid":oid}}}})
            };
            Box::pin(async { Ok(data) })
        }
    }
    fn git(dir: &Path, args: &[&str]) {
        let out = std::process::Command::new("git")
            .current_dir(dir)
            .args(["-c", "commit.gpgsign=false"])
            .args(args)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
    }
    async fn fixture() -> (tempfile::TempDir, Arc<Integration>, Arc<Fake>) {
        let dir = tempfile::tempdir().unwrap();
        git(dir.path(), &["init", "-q", "-b", "feature"]);
        git(
            dir.path(),
            &[
                "-c",
                "user.name=Test",
                "-c",
                "user.email=test@example.com",
                "commit",
                "--allow-empty",
                "-qm",
                "A useful change",
            ],
        );
        git(
            dir.path(),
            &[
                "remote",
                "add",
                "origin",
                "https://github.com/user/project.git",
            ],
        );
        git(
            dir.path(),
            &["update-ref", "refs/remotes/origin/feature", "HEAD"],
        );
        git(
            dir.path(),
            &["branch", "--set-upstream-to=origin/feature", "feature"],
        );
        let repo = Arc::new(GitRepo::open(dir.path()).await.unwrap());
        let fake = Arc::new(Fake {
            oid: Mutex::new(repo.resolve("HEAD").await.unwrap()),
            ..Default::default()
        });
        let enabled = Arc::new(AtomicBool::new(true));
        let github = guarded(
            Github {
                instance: diffle::github::hosts::GithubInstance::github_com(),
                client: Some(fake.clone()),
            },
            enabled.clone(),
        );
        (dir, Integration::new(github, repo, enabled), fake)
    }
    async fn input(integration: &Integration) -> CreateRequest {
        let context = integration.context().await.unwrap();
        CreateRequest {
            repository: "upstream/project".into(),
            head_repository: context.head_repository.unwrap(),
            branch: context.branch.unwrap(),
            head: context.head.unwrap(),
            base: "main".into(),
            title: " A useful change ".into(),
            body: "Tested locally".into(),
            draft: true,
        }
    }

    #[tokio::test]
    async fn preference_persists_and_corrupt_settings_do_not_enable_github() {
        let dir = tempfile::tempdir().unwrap();
        let config = dir.path().join("review.json");
        assert!(Connections::load(&config).await.unwrap().github_enabled);
        Connections {
            github_enabled: false,
        }
        .save(&config)
        .await
        .unwrap();
        assert!(!Connections::load(&config).await.unwrap().github_enabled);
        let contents = tokio::fs::read_to_string(config.with_file_name("connections.json"))
            .await
            .unwrap();
        assert_eq!(
            serde_json::from_str::<Value>(&contents).unwrap(),
            json!({"githubEnabled":false})
        );
        tokio::fs::write(config.with_file_name("connections.json"), "broken")
            .await
            .unwrap();
        assert!(Connections::load(&config).await.is_err());
    }

    #[tokio::test]
    async fn creates_a_draft_from_the_verified_fork_and_returns_existing_pr_on_retry() {
        let (_dir, integration, fake) = fixture().await;
        let result = integration.create(input(&integration).await).await.unwrap();
        assert_eq!(result["number"], 7);
        let mutations: Vec<_> = fake
            .calls
            .lock()
            .unwrap()
            .iter()
            .filter(|(q, _)| q.contains("mutation"))
            .cloned()
            .collect();
        assert_eq!(mutations.len(), 1);
        assert_eq!(
            mutations[0].1["input"],
            json!({"repositoryId":"upstream/project","headRepositoryId":"user/project","headRefName":"user:feature","baseRefName":"main","title":"A useful change","body":"Tested locally","draft":true})
        );
        fake.existing.store(true, Ordering::SeqCst);
        let retry = integration.create(input(&integration).await).await.unwrap();
        assert_eq!(retry["number"], 3);
        assert_eq!(retry["existing"], true);
        assert_eq!(
            fake.calls
                .lock()
                .unwrap()
                .iter()
                .filter(|(q, _)| q.contains("mutation"))
                .count(),
            1
        );
    }

    #[tokio::test]
    async fn refuses_changed_local_or_remote_heads_and_missing_upstreams() {
        let (dir, integration, fake) = fixture().await;
        let mut request = input(&integration).await;
        request.head = "old-head".into();
        assert!(integration
            .create(request)
            .await
            .unwrap_err()
            .to_string()
            .contains("branch changed"));
        let request = input(&integration).await;
        *fake.oid.lock().unwrap() = "a-different-commit".into();
        assert!(integration
            .create(request)
            .await
            .unwrap_err()
            .to_string()
            .contains("differs from your local HEAD"));
        git(dir.path(), &["branch", "--unset-upstream"]);
        assert!(integration
            .context()
            .await
            .unwrap()
            .reason
            .unwrap()
            .contains("set its upstream"));
        git(dir.path(), &["checkout", "--detach", "-q"]);
        assert!(integration
            .context()
            .await
            .unwrap()
            .reason
            .unwrap()
            .contains("Check out a local branch"));
        assert!(!fake
            .calls
            .lock()
            .unwrap()
            .iter()
            .any(|(q, _)| q.contains("mutation")));
    }

    #[tokio::test]
    async fn disable_blocks_all_github_routes_and_internal_client_but_keeps_local_review() {
        let (_dir, integration, fake) = fixture().await;
        integration.enabled.store(false, Ordering::SeqCst);
        let local = Router::new()
            .route(
                "/api/review",
                axum::routing::post(|body: String| async move { body }),
            )
            .route("/api/snapshot", get(|| async { "local snapshot" }));
        let router = router(local, &integration);
        for (method, path, body) in [
            ("GET", "/api/github", None),
            ("POST", "/api/github/export", Some("{}")),
            ("POST", "/api/permalink", Some("{}")),
            ("GET", "/api/desktop/github", None),
            ("POST", "/api/desktop/github", Some("{}")),
            ("POST", "/api/review", Some(r#"{"kind":"pr","pr":"7"}"#)),
        ] {
            assert_eq!(
                dispatch(router.clone(), method, path, body.map(str::to_owned))
                    .await
                    .unwrap()
                    .status,
                403,
                "{path}"
            );
        }
        assert_eq!(
            dispatch(router.clone(), "GET", "/api/snapshot", None)
                .await
                .unwrap()
                .status,
            200
        );
        let local = dispatch(
            router,
            "POST",
            "/api/review",
            Some(r#"{"kind":"working"}"#.into()),
        )
        .await
        .unwrap();
        assert_eq!(local.status, 200);
        assert_eq!(&local.body[..], br#"{"kind":"working"}"#);
        assert!(integration
            .github
            .client
            .as_ref()
            .unwrap()
            .graphql("query", json!({}), None)
            .await
            .is_err());
        assert!(fake.calls.lock().unwrap().is_empty());
    }
}
