//! Patches, file contents, images and search, all behind the snapshot's allowlist.
use std::collections::HashMap;
use std::sync::Arc;

use axum::Json;
use axum::Router;
use axum::body::Bytes;
use axum::extract::{RawQuery, State};
use axum::http::header;
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use serde::Deserialize;

use super::{ApiDeps, ApiError, ApiResult, body, query};
use crate::concurrency::map_limit;
use crate::git::GrepOptions;
use crate::git::is_binary;
use crate::protocol::{
    ChangeStatus, FileQuery, FileResponse, ImageQuery, PatchQuery, SearchContent, SearchMatch, SearchQuery,
    SearchResponse, SearchScope, Side,
};
use crate::server::image::image_type;

/// The most paths one `POST /api/patch` takes: a client loads a review in bounded batches.
const MAX_PATCH_PATHS: usize = 200;
/// Matches one search returns before it reports truncation.
const SEARCH_LIMIT: usize = 500;

pub fn routes() -> Router<Arc<ApiDeps>> {
    Router::new()
        .route("/api/patch", get(patch).post(patch_many))
        .route("/api/patch-page", axum::routing::post(patch_page))
        .route("/api/file", get(file))
        .route("/api/image", get(image))
        .route("/api/search", get(search))
}

fn text(body: String) -> Response {
    ([(header::CONTENT_TYPE, "text/plain; charset=UTF-8")], body).into_response()
}

async fn patch(State(d): State<Arc<ApiDeps>>, RawQuery(raw): RawQuery) -> ApiResult {
    let q: PatchQuery = query(raw.as_deref())?;
    let snapshotter = d.session.snapshotter()?;
    if let Some(version) = q.version {
        let expected = version.parse::<i64>().map_err(|_| ApiError::bad_request("invalid version"))?;
        if snapshotter.current().await?.version != expected {
            return Err(ApiError::bad_request("review changed; retry"));
        }
    }
    let Some(path) = q.path.filter(|p| !p.is_empty()) else { return Ok(text(snapshotter.patch_all().await?)) };
    match snapshotter.patch(&path).await? {
        Some(patch) => Ok(text(patch)),
        None => Err(ApiError::not_found("not a changed file")),
    }
}

#[derive(Deserialize)]
struct PatchRequest {
    paths: Vec<String>,
    version: Option<i64>,
}

async fn patch_many(State(d): State<Arc<ApiDeps>>, raw: Bytes) -> ApiResult {
    let req: PatchRequest = body(&raw)?;
    if req.paths.len() > MAX_PATCH_PATHS {
        return Err(ApiError::bad_request(format!("paths: at most {MAX_PATCH_PATHS} per request")));
    }
    let snapshotter = d.session.snapshotter()?;
    if let Some(version) = req.version
        && snapshotter.current().await?.version != version
    {
        return Err(ApiError::bad_request("review changed; retry"));
    }
    Ok(text(crate::scheduler::background(snapshotter.patch_many(&req.paths)).await?))
}

#[derive(Deserialize)]
struct PageRequest {
    path: String,
    version: i64,
    #[serde(default)]
    offset: usize,
    digest: Option<String>,
}
async fn patch_page(State(d): State<Arc<ApiDeps>>, raw: Bytes) -> ApiResult {
    let req: PageRequest = body(&raw)?;
    Ok(Json(d.session.snapshotter()?.patch_page(&req.path, req.version, req.offset, req.digest.as_deref()).await?)
        .into_response())
}

async fn file(State(d): State<Arc<ApiDeps>>, RawQuery(raw): RawQuery) -> ApiResult {
    let q: FileQuery = query(raw.as_deref())?;
    let path = q.path.to_string();
    let snap = d.session.snapshotter()?.current().await?;
    // Outside the snapshot only a file the language server named is readable (see `Locator::read_external`).
    let mut found = d.session.read_side(&snap, &path, q.rev).await?;
    if found.is_none()
        && q.rev == Side::New
        && let Some(pool) = &d.lsp
    {
        found = pool.read_external(&path).await;
    }
    let Some(buf) = found else {
        return Err(ApiError::not_found("absent on that side"));
    };
    let binary = is_binary(&buf);
    let contents = if binary { String::new() } else { String::from_utf8_lossy(&buf).into_owned() };
    Ok(Json(FileResponse { path, contents, binary }).into_response())
}

/// Snapshot files only, under the key `image_key` gives their side.
async fn image(State(d): State<Arc<ApiDeps>>, RawQuery(raw): RawQuery) -> ApiResult {
    let q: ImageQuery = query(raw.as_deref())?;
    let snap = d.session.snapshotter()?.current().await?;
    let buf = d.session.read_image(&snap, &q.path, q.rev, &q.key).await?;
    let Some((buf, kind)) = buf.and_then(|b| image_type(&b).map(|kind| (b, kind))) else {
        return Err(ApiError::not_found("no image under that key"));
    };
    let headers = [
        (header::CONTENT_TYPE, kind),
        (header::CACHE_CONTROL, "private, max-age=31536000, immutable"),
        (header::X_CONTENT_TYPE_OPTIONS, "nosniff"),
        // An <img> from another site sends no Origin, so the origin guard passes it; this blocks the embed.
        (header::HeaderName::from_static("cross-origin-resource-policy"), "same-origin"),
    ];
    Ok((headers, buf).into_response())
}

async fn search(State(d): State<Arc<ApiDeps>>, RawQuery(raw): RawQuery) -> ApiResult {
    let q: SearchQuery = query(raw.as_deref())?;
    let flag = |v: Option<String>| v.as_deref() == Some("1");
    let snapshotter = d.session.snapshotter()?;
    let snap = snapshotter.current().await?;
    let sides: &[Side] = match q.content {
        SearchContent::Removed => &[Side::Old],
        SearchContent::Both => &[Side::Old, Side::New],
        _ => &[Side::New],
    };
    let mut matches = Vec::new();
    let mut truncated = false;
    for &side in sides {
        let mut paths: Option<Vec<String>> = match q.scope {
            SearchScope::Repo => None,
            SearchScope::File => Some(snap.tree.iter().filter(|p| Some(*p) == q.path.as_ref()).cloned().collect()),
            SearchScope::Diff => {
                Some(snap.changed.iter().filter(|f| f.status != ChangeStatus::D).map(|f| f.path.clone()).collect())
            }
        };
        let mut ranges = None;
        let mut renamed = HashMap::new();
        if q.content != SearchContent::Full {
            let candidates: Vec<_> = snap
                .changed
                .iter()
                .filter(|f| !f.binary)
                .filter(|f| q.scope != SearchScope::File || Some(&f.path) == q.path.as_ref())
                .cloned()
                .collect();
            let snapshotter = &snapshotter;
            let entries = map_limit(candidates, 8, |file| async move {
                let spans = snapshotter.search_ranges(&file.path, side, q.content == SearchContent::Diff).await?;
                let source = if side == Side::Old {
                    file.old_path.clone().unwrap_or_else(|| file.path.clone())
                } else {
                    file.path.clone()
                };
                Ok::<_, crate::error::Error>((source, file.path, spans))
            })
            .await
            .into_iter()
            .collect::<Result<Vec<_>, _>>()?;
            paths = Some(entries.iter().filter(|(_, _, spans)| !spans.is_empty()).map(|(p, _, _)| p.clone()).collect());
            let mut indexed = HashMap::new();
            for (source, path, spans) in entries {
                renamed.insert(source.clone(), path);
                indexed.insert(source, spans);
            }
            ranges = Some(indexed);
        }
        let opts = GrepOptions {
            word: flag(q.word.map(|w| w.to_string())),
            ignore_case: flag(q.i.map(|w| w.to_string())),
            regex: flag(q.re.map(|w| w.to_string())),
            paths: paths.as_deref(),
            ranges: ranges.as_ref(),
        };
        let rev = if side == Side::Old { &snap.old_sha } else { &snap.new_sha };
        let found = d.session.repo.grep(&q.q, rev, SEARCH_LIMIT, opts).await.map_err(crate::error::Error::from)?;
        truncated |= found.truncated;
        matches.extend(found.matches.into_iter().map(|m| SearchMatch {
            path: renamed.get(&m.path).cloned().unwrap_or(m.path),
            line: m.line,
            text: m.text,
            side: Some(side),
        }));
    }
    if snapshotter.current().await?.version != snap.version {
        return Err(crate::error::Error::Invalid("Review changed during search; search again".into()).into());
    }
    matches.sort_by(|a, b| a.path.cmp(&b.path).then(a.line.cmp(&b.line)));
    truncated |= matches.len() > SEARCH_LIMIT;
    matches.truncate(SEARCH_LIMIT);
    Ok(Json(SearchResponse { query: q.q, matches, truncated }).into_response())
}
