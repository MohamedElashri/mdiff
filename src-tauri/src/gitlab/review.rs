use super::Gitlab;
use anyhow::{bail, Context, Result};
use reqwest::Method;
use serde_json::{json, Value};

impl Gitlab {
    async fn fresh(&self) -> Result<()> {
        let latest = self.call(Method::GET, "", None).await?;
        if latest["diff_refs"] != self.metadata["diff_refs"] {
            bail!("This merge request changed. Reopen it before exporting or publishing comments.");
        }
        Ok(())
    }
    pub async fn export(&self, router: axum::Router) -> Result<Value> {
        self.fresh().await?;
        let snapshot = read(router.clone(), "/api/snapshot").await?;
        if snapshot["newSha"] != self.metadata["diff_refs"]["head_sha"]
            || snapshot["oldSha"] != self.metadata["diff_refs"]["base_sha"]
        {
            bail!("Return to the original merge request comparison before exporting comments.");
        }
        let threads = read(router.clone(), "/api/threads?state=open").await?;
        let drafts = self.list("/draft_notes").await?;
        let discussions = self.list("/discussions").await?;
        let mut count = 0;
        let mut skipped = 0;
        for thread in threads.as_array().context("Invalid review threads")? {
            if thread["stale"].as_bool() == Some(true) {
                skipped += 1;
                continue;
            }
            let marker = format!(
                "<!-- mdiff:{}:{} -->",
                self.metadata["diff_refs"]["head_sha"].as_str().unwrap(),
                thread["id"].as_str().context("Invalid thread")?
            );
            if discussions.iter().any(|d| {
                d["notes"].as_array().is_some_and(|notes| {
                    notes
                        .iter()
                        .any(|n| n["body"].as_str().is_some_and(|s| s.contains(&marker)))
                })
            }) {
                skipped += 1;
                continue;
            }
            let a = &thread["anchor"];
            let path = a["path"].as_str().context("Invalid comment path")?;
            let messages = thread["messages"]
                .as_array()
                .context("Invalid messages")?
                .iter()
                .filter_map(|m| m["body"].as_str())
                .collect::<Vec<_>>()
                .join("\n\n---\n\n");
            // Preserve the entire selected range and quote, including file comments and renamed files.
            let location = if a["kind"] == "line" {
                format!(
                    "{}:{}–{} ({})",
                    path,
                    a["startLine"],
                    a["endLine"],
                    a["side"].as_str().unwrap_or("new")
                )
            } else {
                path.into()
            };
            let quoted = a["quoted"]
                .as_str()
                .map(|q| {
                    q.lines()
                        .map(|line| format!("> {line}"))
                        .collect::<Vec<_>>()
                        .join("\n")
                })
                .unwrap_or_default();
            let note = format!(
                "**Review: {}**\n\n{}\n\n{}\n\n{}",
                location.replace('*', "\\*"),
                quoted,
                messages,
                marker
            );
            let existing = drafts
                .iter()
                .find(|d| d["note"].as_str().is_some_and(|s| s.contains(&marker)));
            let suffix = existing
                .and_then(|d| d["id"].as_u64())
                .map(|id| format!("/draft_notes/{id}"));
            let query = {
                let mut query = url::form_urlencoded::Serializer::new(String::new());
                query.append_pair("path", path);
                query.finish()
            };
            let patch = crate::transport::dispatch(
                router.clone(),
                "GET",
                &format!("/api/patch?{}", query),
                None,
            )
            .await
            .map_err(anyhow::Error::msg)?;
            let position = super::position::position(
                a,
                &snapshot["changed"],
                &self.metadata["diff_refs"],
                &String::from_utf8_lossy(&patch.body),
            );
            let mut payload = json!({"note":note});
            if let Some(position) = position {
                payload["position"] = position;
            }
            self.call(
                if suffix.is_some() {
                    Method::PUT
                } else {
                    Method::POST
                },
                suffix.as_deref().unwrap_or("/draft_notes"),
                Some(payload),
            )
            .await?;
            count += 1;
        }
        Ok(json!({"exported":count,"skipped":skipped}))
    }
    pub async fn publish(&self) -> Result<Value> {
        self.fresh().await?;
        let marker = format!(
            "<!-- mdiff:{}:",
            self.metadata["diff_refs"]["head_sha"].as_str().unwrap()
        );
        let mut count = 0;
        for draft in self.list("/draft_notes").await? {
            if draft["note"].as_str().is_some_and(|s| s.contains(&marker)) {
                let id = draft["id"].as_u64().context("Invalid draft id")?;
                self.call(Method::PUT, &format!("/draft_notes/{id}/publish"), None)
                    .await?;
                count += 1;
            }
        }
        Ok(json!({"published":count}))
    }
}
async fn read(router: axum::Router, path: &str) -> Result<Value> {
    let reply = crate::transport::dispatch(router, "GET", path, None)
        .await
        .map_err(anyhow::Error::msg)?;
    if reply.status != 200 {
        bail!("Unable to read active review");
    }
    Ok(serde_json::from_slice(&reply.body)?)
}
