use super::Gitlab;
use anyhow::{bail, Context, Result};
use reqwest::Method;
use serde_json::{json, Value};
use std::collections::HashMap;

struct DraftWrite {
    id: String,
    path: String,
    position: Value,
    note: String,
    existing: Option<u64>,
}

struct ExportPlan {
    version: Value,
    drafts: Vec<DraftWrite>,
    skipped: Vec<Value>,
}

impl ExportPlan {
    fn preview(&self) -> Value {
        let drafts: Vec<_> = self
            .drafts
            .iter()
            .map(|draft| json!({"id":draft.id,"path":draft.path,"position":draft.position}))
            .collect();
        json!({"version":self.version,"drafts":drafts,"skipped":self.skipped})
    }
}

impl Gitlab {
    async fn fresh(&self) -> Result<()> {
        let latest = self.call(Method::GET, "", None).await?;
        if latest["diff_refs"] != self.metadata["diff_refs"] {
            bail!("This merge request changed. Reopen it before exporting or publishing comments.");
        }
        Ok(())
    }
    async fn plan_export(&self, router: axum::Router) -> Result<ExportPlan> {
        self.fresh().await?;
        let snapshot = read(router.clone(), "/api/snapshot").await?;
        if snapshot["newSha"] != self.metadata["diff_refs"]["head_sha"]
            || snapshot["oldSha"] != self.metadata["diff_refs"]["base_sha"]
        {
            bail!("Return to the original merge request comparison before exporting comments.");
        }
        let (threads, drafts, discussions) = tokio::try_join!(
            read(router.clone(), "/api/threads?state=open"),
            self.list("/draft_notes"),
            self.list("/discussions"),
        )?;
        let changed: HashMap<_, _> = snapshot["changed"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|file| Some((file["path"].as_str()?, file)))
            .collect();
        // Threads from the review store are ordered by path. Keep only one patch in memory.
        let mut current_patch: Option<(&str, axum::body::Bytes)> = None;
        let mut plan = ExportPlan {
            version: snapshot["version"].clone(),
            drafts: Vec::new(),
            skipped: Vec::new(),
        };
        for thread in threads.as_array().context("Invalid review threads")? {
            let id = thread["id"].as_str().context("Invalid thread")?;
            let a = &thread["anchor"];
            let path = a["path"].as_str().unwrap_or(id);
            if thread["stale"].as_bool() == Some(true) {
                plan.skipped.push(
                    json!({"id":id,"path":path,"reason":"The lines changed. Choose a new anchor."}),
                );
                continue;
            }
            let marker = format!(
                "<!-- mdiff:{}:{} -->",
                self.metadata["diff_refs"]["head_sha"].as_str().unwrap(),
                id
            );
            if discussions.iter().any(|d| {
                d["notes"].as_array().is_some_and(|notes| {
                    notes
                        .iter()
                        .any(|n| n["body"].as_str().is_some_and(|s| s.contains(&marker)))
                })
            }) {
                plan.skipped
                    .push(json!({"id":id,"path":path,"reason":"Already published"}));
                continue;
            }
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
            let existing = existing.and_then(|d| d["id"].as_u64());
            let position = if let Some(file) = changed.get(path) {
                let patch = if a["kind"] == "file" {
                    Default::default()
                } else {
                    if current_patch
                        .as_ref()
                        .is_none_or(|(loaded, _)| *loaded != path)
                    {
                        let endpoint = {
                            let mut query = url::form_urlencoded::Serializer::new(String::new());
                            query.append_pair("path", path);
                            format!("/api/patch?{}", query.finish())
                        };
                        let patch =
                            crate::transport::dispatch(router.clone(), "GET", &endpoint, None)
                                .await
                                .map_err(anyhow::Error::msg)?;
                        if patch.status != 200 {
                            bail!("Unable to read the diff for {path}");
                        }
                        current_patch = Some((path, patch.body));
                    }
                    String::from_utf8_lossy(&current_patch.as_ref().unwrap().1)
                };
                super::position::position(a, file, &self.metadata["diff_refs"], &patch)
            } else {
                None
            };
            let Some(position) = position else {
                plan.skipped.push(json!({"id":id,"path":location,"reason":"No matching diff lines. Choose a new anchor."}));
                continue;
            };
            plan.drafts.push(DraftWrite {
                id: id.into(),
                path: path.into(),
                position,
                note,
                existing,
            });
        }
        Ok(plan)
    }

    pub async fn preview_export(&self, router: axum::Router) -> Result<Value> {
        Ok(self.plan_export(router).await?.preview())
    }

    pub async fn export(&self, router: axum::Router, preview: Value) -> Result<Value> {
        let plan = self.plan_export(router.clone()).await?;
        if preview != plan.preview()
            || read(router, "/api/snapshot").await?["version"] != plan.version
        {
            bail!("The comparison or comment anchors changed. Preview your drafts again.");
        }
        for draft in &plan.drafts {
            let payload = json!({"note":draft.note,"position":draft.position});
            if let Some(id) = draft.existing {
                self.call(Method::PUT, &format!("/draft_notes/{id}"), Some(payload))
                    .await?;
            } else {
                self.call(Method::POST, "/draft_notes", Some(payload))
                    .await?;
            }
        }
        Ok(json!({"exported":plan.drafts.len(),"skipped":plan.skipped.len()}))
    }

    pub fn owns_draft(&self, draft: &Value) -> bool {
        let Some(head) = self.metadata["diff_refs"]["head_sha"].as_str() else {
            return false;
        };
        let marker = format!("<!-- mdiff:{head}:");
        draft["note"].as_str().is_some_and(|note| {
            note.trim_end()
                .lines()
                .last()
                .is_some_and(|line| line.starts_with(&marker) && line.ends_with(" -->"))
        })
    }

    pub fn publishable_draft(&self, draft: &Value) -> bool {
        let position = &draft["position"];
        let refs = &self.metadata["diff_refs"];
        self.owns_draft(draft)
            && matches!(position["position_type"].as_str(), Some("text" | "file"))
            && ["base_sha", "start_sha", "head_sha"]
                .iter()
                .all(|key| refs[*key].is_string() && position[*key] == refs[*key])
    }

    pub async fn publish(&self, confirmed: Value) -> Result<Value> {
        self.fresh().await?;
        let drafts = self.list("/draft_notes").await?;
        let selected = confirmed.as_array().context("Missing confirmed drafts")?;
        let mut ids = std::collections::HashSet::new();
        for draft in selected {
            let id = draft["id"].as_u64().context("Invalid draft id")?;
            if !ids.insert(id)
                || !drafts.iter().any(|saved| {
                    saved["id"] == id
                        && saved["position"] == draft["position"]
                        && self.publishable_draft(saved)
                })
            {
                bail!("The draft positions changed. Refresh and review them before publishing.");
            }
        }
        for draft in selected {
            let id = draft["id"].as_u64().context("Invalid draft id")?;
            self.call(Method::PUT, &format!("/draft_notes/{id}/publish"), None)
                .await?;
        }
        Ok(json!({"published":selected.len()}))
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
