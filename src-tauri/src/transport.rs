//! Session-scoped native IPC. Routes are dispatched in process, never over a socket.
use crate::engine::Engine;
use axum::{
    body::{to_bytes, Body},
    http::Request,
};
use tauri::State;
use tower::ServiceExt;

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
    let response = router.oneshot(request).await.map_err(|e| e.to_string())?;
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
pub async fn review_request(
    state: State<'_, Engine>,
    id: String,
    method: String,
    path: String,
    body: Option<String>,
) -> Result<tauri::ipc::Response, String> {
    // Release the lifecycle lock before expensive reads and LSP requests.
    let router = {
        let slot = state.0.lock().await;
        let runtime = slot
            .as_ref()
            .filter(|r| r.id == id)
            .ok_or("This review has closed")?;
        runtime.router.clone()
    };
    let reply = dispatch(router, &method, &path, body).await?;
    Ok(tauri::ipc::Response::new(pack(reply)?))
}

/// A little-endian header length, JSON metadata, then raw bytes. Images never become base64.
fn pack(reply: Reply) -> Result<Vec<u8>, String> {
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
