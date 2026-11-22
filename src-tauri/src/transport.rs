//! Session-scoped native IPC. Routes are dispatched in process, never over a socket.
use crate::engine::Engine;
use axum::{
    body::{to_bytes, Body},
    http::Request,
};
use std::{
    collections::{hash_map::Entry, HashMap},
    sync::Mutex,
    time::{Duration, Instant},
};
use tauri::State;
use tokio::sync::oneshot;
use tower::ServiceExt;

type RequestKey = (String, String);
const MAX_ACTIVE_REQUESTS: usize = 256;
const MAX_EARLY_CANCELLATIONS: usize = 128;
const CANCELLATION_TTL: Duration = Duration::from_secs(60);

#[derive(Default)]
struct RequestEntries {
    active: HashMap<RequestKey, Option<oneshot::Sender<()>>>,
    early: HashMap<RequestKey, Instant>,
}

/// Cancellation ids are scoped to a review. Short-lived tombstones cover an abort that
/// reaches Rust before its request. Tombstones and running requests have separate limits.
#[derive(Default)]
pub struct Requests(Mutex<RequestEntries>);
pub(crate) struct RequestGuard<'a> {
    requests: &'a Requests,
    key: RequestKey,
}
impl Drop for RequestGuard<'_> {
    fn drop(&mut self) {
        self.requests.0.lock().unwrap().active.remove(&self.key);
    }
}
impl Requests {
    pub(crate) fn start(
        &self,
        key: RequestKey,
    ) -> Result<(RequestGuard<'_>, oneshot::Receiver<()>), String> {
        let mut requests = self.0.lock().unwrap();
        if requests
            .early
            .remove(&key)
            .is_some_and(|at| at.elapsed() < CANCELLATION_TTL)
        {
            return Err("Request cancelled".into());
        }
        if requests.active.len() >= MAX_ACTIVE_REQUESTS {
            return Err("Too many review requests".into());
        }
        let Entry::Vacant(entry) = requests.active.entry(key.clone()) else {
            return Err("Duplicate review request ID".into());
        };
        let (tx, rx) = oneshot::channel();
        entry.insert(Some(tx));
        Ok((
            RequestGuard {
                requests: self,
                key,
            },
            rx,
        ))
    }
    pub(crate) fn cancel_owner(&self, owner: &str) {
        let mut requests = self.0.lock().unwrap();
        requests.active.retain(|(id, _), sender| {
            if id == owner {
                if let Some(sender) = sender.take() {
                    let _ = sender.send(());
                }
                false
            } else {
                true
            }
        });
        requests.early.retain(|(id, _), _| id != owner);
    }
    pub(crate) fn cancel(&self, key: RequestKey) {
        let mut requests = self.0.lock().unwrap();
        if let Some(sender) = requests.active.get_mut(&key) {
            if let Some(sender) = sender.take() {
                let _ = sender.send(());
            }
            return;
        }
        let early = &mut requests.early;
        if early.len() >= MAX_EARLY_CANCELLATIONS && !early.contains_key(&key) {
            let oldest = early
                .iter()
                .min_by_key(|(_, at)| *at)
                .map(|(key, _)| key.clone())
                .unwrap();
            early.remove(&oldest);
        }
        early.insert(key, Instant::now());
    }
}
#[tauri::command]
pub async fn cancel_review_request(
    state: State<'_, Engine>,
    repositories: State<'_, crate::repository::Repositories>,
    window: tauri::WebviewWindow,
    requests: State<'_, Requests>,
    id: String,
    request_id: String,
) -> Result<(), String> {
    if id.starts_with("history-") {
        repositories.owns(&id, window.label()).await?;
    } else {
        state.owns(&id, window.label()).await?;
    }
    requests.cancel((id, request_id));
    Ok(())
}

pub struct Reply {
    pub status: u16,
    pub content_type: String,
    pub body: axum::body::Bytes,
}

pub async fn dispatch(
    router: axum::Router,
    method: &str,
    path: &str,
    body: Option<String>,
) -> Result<Reply, String> {
    if !path.starts_with("/api/") || path.contains('#') {
        return Err("Invalid review operation".into());
    }
    if body.as_ref().is_some_and(|b| b.len() > 1024 * 1024) {
        return Err("Request too large".into());
    }
    let request = Request::builder()
        .method(method)
        .uri(path)
        .header("content-type", "application/json")
        .body(Body::from(body.unwrap_or_default()))
        .map_err(|e| e.to_string())?;
    let response = router
        .oneshot(request)
        .await
        .expect("Axum router is infallible");
    let status = response.status().as_u16();
    let content_type = response
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("application/octet-stream")
        .to_owned();
    let bytes = to_bytes(response.into_body(), 256 * 1024 * 1024)
        .await
        .map_err(|e| e.to_string())?;
    Ok(Reply {
        status,
        content_type,
        body: bytes,
    })
}

#[tauri::command]
#[allow(clippy::too_many_arguments)] // Window/state are injected; keep the request wire shape stable.
pub async fn review_request(
    window: tauri::WebviewWindow,
    state: State<'_, Engine>,
    requests: State<'_, Requests>,
    id: String,
    request_id: Option<String>,
    method: String,
    path: String,
    body: Option<String>,
) -> Result<tauri::ipc::Response, String> {
    let route = path
        .split_once('?')
        .map_or(path.as_str(), |(route, _)| route);
    // Config updates share a file across review runtimes. Serialize them with opening and
    // settings writes so a newly prepared review picks up the committed settings.
    let _transition = if method == "PUT" && route == "/api/config" {
        Some(state.lifecycle.lock().await)
    } else {
        None
    };
    let runtime = state.owned_runtime(&id, window.label()).await?;
    let router = runtime.router.clone();
    let mut closing = runtime.closing();
    // Mutations finish atomically even if their UI caller goes away.
    let cancellable = method == "GET"
        || (method == "POST"
            && matches!(
                route,
                "/api/patch"
                    | "/api/patch-page"
                    | "/api/lsp/hover"
                    | "/api/lsp/references"
                    | "/api/lsp/definition"
                    | "/api/lsp/type-definition"
                    | "/api/lsp/occurrences"
            ));
    let reply = if cancellable {
        drop(runtime);
        let (_guard, cancelled) = match request_id {
            Some(request_id) => {
                let (guard, cancelled) = requests.start((id, request_id))?;
                (Some(guard), Some(cancelled))
            }
            None => (None, None),
        };
        tokio::select! {
            biased;
            _ = closing.wait_for(|closed| *closed) => return Err("This review has closed".into()),
            _ = async {
                match cancelled {
                    Some(cancelled) => { let _ = cancelled.await; }
                    None => std::future::pending().await,
                }
            } => return Err("Request cancelled".into()),
            reply = dispatch(router, &method, &path, body) => reply?,
        }
    } else {
        // A mutation owns the runtime until it settles, even if the caller drops its reply.
        // Teardown waits for this guard; no registry lock spans the operation.
        tokio::spawn(async move {
            let _owner = runtime;
            dispatch(router, &method, &path, body).await
        })
        .await
        .map_err(|e| e.to_string())??
    };
    Ok(tauri::ipc::Response::new(pack(reply)?))
}

/// A little-endian header length, JSON metadata, then raw bytes. Images never become base64.
pub(crate) fn pack(reply: Reply) -> Result<Vec<u8>, String> {
    let header = serde_json::to_vec(
        &serde_json::json!({"status":reply.status,"contentType":reply.content_type}),
    )
    .map_err(|e| e.to_string())?;
    let mut packet = Vec::with_capacity(4 + header.len() + reply.body.len());
    packet.extend_from_slice(&(header.len() as u32).to_le_bytes());
    packet.extend_from_slice(&header);
    packet.extend_from_slice(&reply.body);
    Ok(packet)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn raw_packet_preserves_binary_and_status() {
        let packet = pack(Reply {
            status: 200,
            content_type: "image/png".into(),
            body: vec![0, 255, 128, 10].into(),
        })
        .unwrap();
        let len = u32::from_le_bytes(packet[..4].try_into().unwrap()) as usize;
        let header: serde_json::Value = serde_json::from_slice(&packet[4..4 + len]).unwrap();
        assert_eq!(header["contentType"], "image/png");
        assert_eq!(&packet[4 + len..], &[0, 255, 128, 10]);
    }
}

#[cfg(test)]
mod cancellation_tests {
    use super::*;
    #[tokio::test]
    async fn abort_before_start_and_during_request_are_scoped_and_bounded() {
        let requests = Requests::default();
        requests.cancel(("a".into(), "1".into()));
        assert!(requests.start(("a".into(), "1".into())).is_err());
        let (guard, rx) = requests.start(("b".into(), "1".into())).unwrap();
        assert!(requests.start(("b".into(), "1".into())).is_err());
        requests.cancel(("b".into(), "1".into()));
        requests.cancel(("b".into(), "1".into()));
        assert!(rx.await.is_ok());
        drop(guard);
        assert!(requests.start(("b".into(), "1".into())).is_ok());
        for i in 0..1000 {
            requests.cancel(("b".into(), i.to_string()));
        }
        assert!(requests.0.lock().unwrap().early.len() <= MAX_EARLY_CANCELLATIONS);
        assert!(requests.start(("b".into(), "999".into())).is_err());
        let active = (0..MAX_ACTIVE_REQUESTS)
            .map(|i| requests.start(("c".into(), i.to_string())).unwrap())
            .collect::<Vec<_>>();
        assert!(requests.start(("c".into(), "over-limit".into())).is_err());
        drop(active);
        assert!(requests.start(("c".into(), "over-limit".into())).is_ok());
    }
}
