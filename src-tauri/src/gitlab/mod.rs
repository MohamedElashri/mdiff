//! Host-bound GitLab REST client shared by GitLab.com and self-managed instances.
mod position;
mod repository;
mod review;
use crate::engine::Engine;
use anyhow::{bail, Context, Result};
use reqwest::{Client, Method};
use serde_json::{json, Value};
use url::Url;

pub struct Gitlab {
    pub origin: Url,
    pub project: String,
    pub iid: u64,
    pub metadata: Value,
    token: Option<String>,
    client: Client,
    writes: tokio::sync::Mutex<()>,
}
impl Gitlab {
    pub async fn connect(input: &str, token: Option<String>, host: Option<&str>) -> Result<Self> {
        let (origin, project, iid) = parse(input)?;
        let token = token.filter(|s| !s.trim().is_empty());
        if token.is_some() && host != Some(origin.authority()) {
            bail!("GitLab token host must match {}", origin.authority());
        }
        let client = Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(std::time::Duration::from_secs(45))
            .build()?;
        let mut this = Self {
            origin,
            project,
            iid,
            metadata: Value::Null,
            token,
            client,
            writes: tokio::sync::Mutex::new(()),
        };
        this.metadata = this.call(Method::GET, "", None).await?;
        for name in ["base_sha", "head_sha", "start_sha"] {
            let sha = this.metadata["diff_refs"][name]
                .as_str()
                .context("GitLab is still preparing the merge request diff. Try again shortly.")?;
            if ![40, 64].contains(&sha.len()) || !sha.bytes().all(|c| c.is_ascii_hexdigit()) {
                bail!("Invalid GitLab diff revision");
            }
        }
        Ok(this)
    }
    fn endpoint(&self, suffix: &str) -> Url {
        let mut url = self.origin.clone();
        url.path_segments_mut().unwrap().extend([
            "api",
            "v4",
            "projects",
            &self.project,
            "merge_requests",
            &self.iid.to_string(),
        ]);
        url.set_path(&format!("{}{suffix}", url.path()));
        url
    }
    async fn send(
        &self,
        method: Method,
        url: Url,
        body: Option<Value>,
    ) -> Result<(Value, Option<String>)> {
        let mut req = self.client.request(method, url);
        if let Some(token) = &self.token {
            req = req.header("PRIVATE-TOKEN", token);
        }
        if let Some(body) = body {
            req = req.json(&body);
        }
        let response = req.send().await.context("Cannot reach GitLab")?;
        let status = response.status();
        if !status.is_success() {
            bail!(
                "GitLab returned {status}. Check the host, project access, and token permissions."
            );
        }
        let next = response
            .headers()
            .get("x-next-page")
            .and_then(|h| h.to_str().ok())
            .filter(|s| !s.is_empty())
            .map(str::to_owned);
        let bytes = response.bytes().await?;
        Ok((
            if bytes.is_empty() {
                Value::Null
            } else {
                serde_json::from_slice(&bytes)?
            },
            next,
        ))
    }
    pub async fn call(&self, method: Method, suffix: &str, body: Option<Value>) -> Result<Value> {
        Ok(self.send(method, self.endpoint(suffix), body).await?.0)
    }
    pub async fn list(&self, suffix: &str) -> Result<Vec<Value>> {
        let mut result = vec![];
        let mut page = 1u64;
        loop {
            let mut url = self.endpoint(suffix);
            url.query_pairs_mut()
                .append_pair("per_page", "100")
                .append_pair("page", &page.to_string());
            let (items, next) = self.send(Method::GET, url, None).await?;
            result.extend(
                items
                    .as_array()
                    .context("Unexpected GitLab response")?
                    .iter()
                    .cloned(),
            );
            match next {
                Some(next) => {
                    let n: u64 = next.parse()?;
                    if n <= page || n > 10000 {
                        bail!("Invalid GitLab pagination");
                    }
                    page = n;
                }
                None => break,
            }
        }
        Ok(result)
    }
    pub fn request(&self) -> diffle::protocol::EnterRequest {
        diffle::protocol::EnterRequest::Revspec {
            args: vec!["refs/mdiff/base".into(), "refs/mdiff/head".into()],
        }
    }
}
fn parse(input: &str) -> Result<(Url, String, u64)> {
    let mut url = Url::parse(input).context("Enter a full GitLab merge request URL")?;
    if url.scheme() != "https" || !url.username().is_empty() || url.password().is_some() {
        bail!("Use an HTTPS GitLab merge request URL without embedded credentials");
    }
    let (project, number) = url
        .path()
        .trim_matches('/')
        .rsplit_once("/-/merge_requests/")
        .context("Expected /group/project/-/merge_requests/123")?;
    if project.split('/').any(|s| s.is_empty() || s.contains('%')) {
        bail!("Use the unescaped GitLab project URL");
    }
    let iid = number
        .split('/')
        .next()
        .unwrap_or_default()
        .parse::<u64>()
        .context("Invalid merge request number")?;
    if iid == 0 || !project.contains('/') {
        bail!("Invalid GitLab merge request URL");
    }
    let project = project.to_owned();
    url.set_path("");
    url.set_query(None);
    url.set_fragment(None);
    Ok((url, project, iid))
}

#[tauri::command]
pub async fn gitlab_action(
    state: tauri::State<'_, Engine>,
    id: String,
    action: String,
    payload: Option<Value>,
) -> Result<Value, String> {
    let (gitlab, router) = {
        let slot = state.0.lock().await;
        let runtime = slot
            .as_ref()
            .filter(|r| r.id == id)
            .ok_or("This review has closed")?;
        (
            runtime
                .gitlab
                .clone()
                .ok_or("This is not a GitLab review")?,
            runtime.router.clone(),
        )
    };
    let result = async {
        match action.as_str() {
            "load" => Ok(json!({"mergeRequest":gitlab.metadata,"discussions":gitlab.list("/discussions").await?,"drafts":if gitlab.token.is_some() {gitlab.list("/draft_notes").await?} else {vec![]}})),
            "export" => { let _guard=gitlab.writes.lock().await; gitlab.export(router).await },
            "publish" => { let _guard=gitlab.writes.lock().await; gitlab.publish().await },
            "reply" => {
                let p=payload.context("Missing reply")?;
                let discussion=p["discussion"].as_str().context("Missing discussion")?;
                if !discussion.bytes().all(|c|c.is_ascii_alphanumeric()) { bail!("Invalid discussion"); }
                let body=p["body"].as_str().filter(|s|!s.trim().is_empty()).context("Reply is empty")?;
                gitlab.call(Method::POST,&format!("/discussions/{discussion}/notes"),Some(json!({"body":body}))).await
            },
            "resolve" => {
                let p=payload.context("Missing discussion")?;
                let discussion=p["discussion"].as_str().context("Missing discussion")?;
                if !discussion.bytes().all(|c|c.is_ascii_alphanumeric()) { bail!("Invalid discussion"); }
                gitlab.call(Method::PUT,&format!("/discussions/{discussion}"),Some(json!({"resolved":p["resolved"].as_bool().context("Missing resolution")?}))).await
            },
            _ => bail!("Unknown GitLab operation"),
        }
    }.await;
    result.map_err(|e: anyhow::Error| format!("{e:#}"))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn nested_and_self_managed() {
        let (origin, project, iid) =
            parse("https://git.company.test:8443/group/sub/project/-/merge_requests/27/diffs")
                .unwrap();
        assert_eq!(origin.authority(), "git.company.test:8443");
        assert_eq!(project, "group/sub/project");
        assert_eq!(iid, 27);
        assert!(parse("https://token@gitlab.com/a/b/-/merge_requests/1").is_err());
        assert!(parse("http://gitlab.com/a/b/-/merge_requests/1").is_err());
        assert!(parse("https://gitlab.com/a/b/-/merge_requests/0").is_err());
    }
    #[tokio::test]
    async fn token_is_rejected_before_network() {
        assert!(Gitlab::connect(
            "https://wrong.invalid/a/b/-/merge_requests/1",
            Some("secret".into()),
            Some("gitlab.com")
        )
        .await
        .is_err());
    }
}

#[cfg(test)]
mod integration_tests {
    use super::*;
    use axum::{
        extract::{Request, State},
        response::IntoResponse,
        Router,
    };
    use std::future::IntoFuture;
    use std::sync::{Arc, Mutex};
    #[derive(Default)]
    struct Mock {
        calls: Mutex<Vec<(String, String, Value)>>,
    }
    async fn handler(State(state): State<Arc<Mock>>, request: Request) -> axum::response::Response {
        assert_eq!(request.headers()["private-token"], "test-token");
        let method = request.method().to_string();
        let uri = request.uri().to_string();
        let bytes = axum::body::to_bytes(request.into_body(), 1024 * 1024)
            .await
            .unwrap();
        let body = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
        state
            .calls
            .lock()
            .unwrap()
            .push((method, uri.clone(), body));
        let (next, value) = if uri.contains("draft_notes") {
            (
                "",
                json!([{"id":1,"note":"<!-- mdiff:abc:owned -->"},{"id":2,"note":"My unrelated draft"},{"id":3,"note":"<!-- mdiff:older:owned -->"}]),
            )
        } else if uri.contains("discussions") && uri.contains("page=2") {
            ("", json!([{"id":"second","notes":[]}]))
        } else if uri.contains("discussions") {
            ("2", json!([{"id":"first","notes":[]}]))
        } else {
            ("", json!({"diff_refs":{"head_sha":"abc"}}))
        };
        ([("x-next-page", next)], axum::Json(value)).into_response()
    }
    #[tokio::test]
    async fn export_updates_owned_draft_and_refuses_changed_reviews() {
        use axum::routing::get;
        let state = Arc::new(Mock::default());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let origin = Url::parse(&format!("http://{}/", listener.local_addr().unwrap())).unwrap();
        let server = tokio::spawn(
            axum::serve(
                listener,
                Router::new().fallback(handler).with_state(state.clone()),
            )
            .into_future(),
        );
        let mut gitlab = Gitlab {
            origin,
            project: "group/sub/project".into(),
            iid: 17,
            metadata: json!({"diff_refs":{"head_sha":"abc"}}),
            token: Some("test-token".into()),
            client: Client::new(),
            writes: tokio::sync::Mutex::new(()),
        };
        let router=Router::new()
            .route("/api/snapshot",get(||async {axum::Json(json!({"newSha":"abc","changed":[{"path":"file.rs"}]}))}))
            .route("/api/threads",get(||async {axum::Json(json!([
                {"id":"owned","anchor":{"kind":"line","path":"file.rs","side":"new","startLine":2,"endLine":2,"quoted":"new"},"messages":[{"body":"Please handle empty input."}],"stale":false},
                {"id":"stale","stale":true}
            ]))}))
            .route("/api/patch",get(||async {"@@ -1 +1,2 @@\n same\n+new\n"}));
        let result = gitlab.export(router.clone()).await.unwrap();
        assert_eq!(result["exported"], 1);
        assert_eq!(result["skipped"], 1);
        {
            let calls = state.calls.lock().unwrap();
            let writes: Vec<_> = calls
                .iter()
                .filter(|(m, _, _)| m == "PUT" || m == "POST")
                .collect();
            assert_eq!(writes.len(), 1);
            assert!(writes[0].1.ends_with("/draft_notes/1"));
            assert_eq!(writes[0].2["position"]["new_line"], 2);
            assert!(writes[0].2["note"]
                .as_str()
                .unwrap()
                .contains("Please handle empty input."));
        }
        gitlab.metadata["diff_refs"]["head_sha"] = "outdated".into();
        assert!(gitlab
            .export(router)
            .await
            .unwrap_err()
            .to_string()
            .contains("changed"));
        server.abort();
    }
    #[tokio::test]
    async fn pagination_and_scoped_publish() {
        let state = Arc::new(Mock::default());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let origin = Url::parse(&format!("http://{}/", listener.local_addr().unwrap())).unwrap();
        let router = Router::new().fallback(handler).with_state(state.clone());
        let server = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
        let gitlab = Gitlab {
            origin,
            project: "group/sub/project".into(),
            iid: 17,
            metadata: json!({"diff_refs":{"head_sha":"abc"}}),
            token: Some("test-token".into()),
            client: Client::new(),
            writes: tokio::sync::Mutex::new(()),
        };
        assert_eq!(gitlab.list("/discussions").await.unwrap().len(), 2);
        assert_eq!(gitlab.publish().await.unwrap()["published"], 1);
        let calls = state.calls.lock().unwrap();
        assert!(calls.iter().all(|(_, uri, _)| uri
            .starts_with("/api/v4/projects/group%2Fsub%2Fproject/merge_requests/17")));
        let writes: Vec<_> = calls
            .iter()
            .filter(|(method, _, _)| method == "PUT")
            .collect();
        assert_eq!(writes.len(), 1);
        assert!(writes[0].1.ends_with("/draft_notes/1/publish"));
        server.abort();
    }
}
