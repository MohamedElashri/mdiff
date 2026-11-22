//! The same repository API is dispatched by HTTP and the native adapter.
use super::routes::{ApiError, ApiResult};
use crate::history::{Query, Repository};
use axum::{
    Json, Router,
    extract::{DefaultBodyLimit, Path, State},
    routing::{get, post},
};
use serde::Deserialize;
use std::sync::Arc;
fn error(e: anyhow::Error) -> ApiError {
    ApiError::bad_request(e.to_string())
}
pub fn router(repo: Arc<Repository>) -> Router {
    Router::new()
        .route("/api/history/info", get(info))
        .route("/api/history/query", post(begin))
        .route("/api/history/query/{id}", axum::routing::delete(close))
        .route("/api/history/page/{id}/{page}", get(page))
        .route("/api/history/commit/{sha}", get(detail))
        .route("/api/history/tree/{sha}", get(tree))
        .route("/api/history/compare", post(compare))
        .layer(DefaultBodyLimit::max(1024 * 1024))
        .with_state(repo)
}
async fn info(State(repo): State<Arc<Repository>>) -> ApiResult<Json<crate::history::Info>> {
    Ok(Json(repo.info().await.map_err(error)?))
}
async fn begin(State(repo): State<Arc<Repository>>, Json(query): Json<Query>) -> ApiResult<Json<crate::history::Page>> {
    Ok(Json(repo.begin(query).await.map_err(error)?))
}
async fn close(State(repo): State<Arc<Repository>>, Path(id): Path<String>) -> Json<bool> {
    repo.close_generation(&id).await;
    Json(true)
}
async fn page(
    State(repo): State<Arc<Repository>>,
    Path((id, page)): Path<(String, usize)>,
) -> ApiResult<Json<crate::history::Page>> {
    Ok(Json(repo.page(&id, page).await.map_err(error)?))
}
async fn detail(
    State(repo): State<Arc<Repository>>,
    Path(sha): Path<String>,
) -> ApiResult<Json<crate::history::Detail>> {
    Ok(Json(repo.detail(&sha).await.map_err(error)?))
}
async fn tree(State(repo): State<Arc<Repository>>, Path(sha): Path<String>) -> ApiResult<Json<crate::history::Tree>> {
    Ok(Json(repo.tree(&sha).await.map_err(error)?))
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Compare {
    kind: String,
    commits: Vec<String>,
    parent: Option<String>,
}
async fn compare(
    State(repo): State<Arc<Repository>>,
    Json(req): Json<Compare>,
) -> ApiResult<Json<crate::protocol::EnterRequest>> {
    use crate::protocol::{ComparisonBase, EnterRequest};
    if req.commits.is_empty() || req.commits.len() > 250 {
        return Err(ApiError::bad_request("Choose 1–250 commits"));
    }
    let commits = tokio::time::timeout(std::time::Duration::from_secs(10), async {
        let mut commits = Vec::new();
        for sha in &req.commits {
            let mut detail = repo.detail(sha).await.map_err(error)?;
            if req.kind == "series" && detail.parents.len() > 64 {
                return Err(ApiError::bad_request("This series contains a commit with too many parents"));
            }
            detail.message = String::new();
            detail.author = String::new();
            commits.push(detail);
        }
        Ok::<_, ApiError>(commits)
    })
    .await
    .map_err(|_| ApiError::bad_request("Comparison timed out; select fewer commits"))??;
    let (old, new, base) = match req.kind.as_str() {
        "commit" if commits.len() == 1 => {
            let c = &commits[0];
            let parent = req.parent.or_else(|| c.parents.first().cloned());
            if parent.as_ref().is_some_and(|p| !c.parents.contains(p)) {
                return Err(ApiError::bad_request("Choose an actual parent of this commit"));
            }
            if let Some(parent) = &parent {
                repo.detail(parent).await.map_err(error)?;
            }
            if parent.as_ref() == c.parents.first() {
                return Ok(Json(EnterRequest::Revspec { args: vec![format!("{}^!", c.sha)] }));
            }
            (parent, c.sha.clone(), ComparisonBase::Direct)
        }
        "direct" | "merge-base" if commits.len() == 2 => (
            Some(commits[0].sha.clone()),
            commits[1].sha.clone(),
            if req.kind == "direct" { ComparisonBase::Direct } else { ComparisonBase::MergeBase },
        ),
        "series" => {
            for pair in commits.windows(2) {
                if pair[0].parents.first() != Some(&pair[1].sha) {
                    return Err(ApiError::bad_request(
                        "Selected commits must be a contiguous first-parent series, newest first",
                    ));
                }
            }
            (commits.last().unwrap().parents.first().cloned(), commits[0].sha.clone(), ComparisonBase::Direct)
        }
        _ => return Err(ApiError::bad_request("Invalid comparison selection")),
    };
    Ok(Json(EnterRequest::Comparison { old, new, base }))
}
