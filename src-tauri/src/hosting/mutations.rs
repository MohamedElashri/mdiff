//! Provider writes are dispatched once, after native ownership/revision validation.
use super::{
    http::{Broker, Connection},
    ledger::Record,
    types::*,
};
use anyhow::{bail, Result};
use reqwest::Method;
use serde_json::{json, Value};
pub fn validate(action: &Action) -> Result<()> {
    match action {
        Action::Comment { body: b }
        | Action::Reply { body: b, .. }
        | Action::Submit { body: b, .. } => body(b)?,
        _ => (),
    }
    match action {
        Action::Reply {
            thread, comment, ..
        } => {
            identifier(thread)?;
            if let Some(comment) = comment {
                if comment.parse::<u64>().ok().filter(|id| *id > 0).is_none() {
                    bail!("Invalid discussion comment");
                }
            }
        }
        Action::Resolve { thread, .. } => identifier(thread)?,
        Action::Submit { verdict, .. } => {
            if !["comment", "approve", "requestChanges"].contains(&verdict.as_str()) {
                bail!("Unsupported review verdict");
            }
        }
        Action::Merge { method } if !["merge", "squash", "rebase"].contains(&method.as_str()) => {
            bail!("Unsupported merge method");
        }
        _ => (),
    }
    Ok(())
}
fn marker(id: &str) -> String {
    format!("<!-- mdiff-operation:{id} -->")
}
fn marked(body: &str, id: &str, sha: &str) -> String {
    format!("{body}\n\nReviewed revision: `{sha}`\n\n{}", marker(id))
}
impl Broker {
    pub async fn prepare_action(&self, c: &Connection, w: &Write) -> Result<()> {
        match &w.action {
            Action::Reply {
                thread, comment, ..
            } => {
                self.thread(
                    c,
                    &w.review,
                    thread,
                    if c.site.provider == Provider::Github {
                        comment.clone()
                    } else {
                        None
                    }
                    .as_deref(),
                )
                .await?;
            }
            Action::Resolve { thread, .. } => {
                let target = self.thread(c, &w.review, thread, None).await?;
                anyhow::ensure!(
                    target["canResolve"] == true,
                    "The provider does not permit this resolution action"
                );
            }
            _ => (),
        }
        Ok(())
    }
    pub async fn mutate(&self, c: &Connection, w: &Write) -> Result<Value> {
        let r = &w.review;
        let path = c.review_path(r);
        let project = c.project(r);
        let gh = c.site.provider == Provider::Github;
        let note = |b: &str| marked(b, &w.operation, &w.expected_head);
        match &w.action {
            Action::Comment { body } => {
                self.write(
                    c,
                    Method::POST,
                    &if gh {
                        format!("{project}/issues/{}/comments", r.number)
                    } else {
                        format!("{path}/notes")
                    },
                    json!({"body":note(body)}),
                )
                .await
            }
            Action::Reply {
                thread,
                comment,
                body,
            } => {
                let endpoint = if gh {
                    format!(
                        "{path}/comments/{}/replies",
                        comment.as_deref().ok_or_else(|| anyhow::anyhow!(
                            "Select a discussion comment to reply to"
                        ))?
                    )
                } else {
                    format!("{path}/discussions/{thread}/notes")
                };
                self.write(c, Method::POST, &endpoint, json!({"body":note(body)}))
                    .await
            }
            Action::Resolve { thread, resolved } => {
                if gh {
                    self.graphql(c,if *resolved{"mutation($id:ID!){resolveReviewThread(input:{threadId:$id}){thread{id isResolved}}}"}else{"mutation($id:ID!){unresolveReviewThread(input:{threadId:$id}){thread{id isResolved}}}"},json!({"id":thread}),true).await
                } else {
                    self.write(
                        c,
                        Method::PUT,
                        &format!("{path}/discussions/{thread}"),
                        json!({"resolved":resolved}),
                    )
                    .await
                }
            }
            Action::Submit { verdict, body } => {
                if gh {
                    let event = match verdict.as_str() {
                        "approve" => "APPROVE",
                        "requestChanges" => "REQUEST_CHANGES",
                        _ => "COMMENT",
                    };
                    self.write(
                        c,
                        Method::POST,
                        &format!("{path}/reviews"),
                        json!({"commit_id":w.expected_head,"body":note(body),"event":event}),
                    )
                    .await
                } else {
                    match verdict.as_str(){
                    "approve"=>self.write(c,Method::POST,&format!("{path}/approve"),json!({"sha":w.expected_head})).await,
                    "comment"=>self.write(c,Method::POST,&format!("{path}/notes"),json!({"body":note(body)})).await,
                    _=>bail!("GitLab request-changes submission is not supported by this integration; leave a revision-labelled comment")}
                }
            }
            Action::Merge { method } => {
                if gh {
                    self.write(
                        c,
                        Method::PUT,
                        &format!("{path}/merge"),
                        json!({"sha":w.expected_head,"merge_method":method}),
                    )
                    .await
                } else {
                    if method == "rebase" {
                        bail!("This GitLab merge method is unsupported");
                    }
                    self.write(
                        c,
                        Method::PUT,
                        &format!("{path}/merge"),
                        json!({"sha":w.expected_head,"squash":method=="squash"}),
                    )
                    .await
                }
            }
        }
    }
    async fn thread(
        &self,
        c: &Connection,
        r: &ReviewRef,
        id: &str,
        comment: Option<&str>,
    ) -> Result<Value> {
        // Bounded traversal: older threads remain accessible through the provider link.
        let mut cursor = None;
        for page in 1..=10 {
            let value = self.threads(c, r, page, cursor.as_deref()).await?;
            if let Some(thread) = items(&value["items"]).into_iter().find(|v| v["id"] == id) {
                if let Some(comment) = comment {
                    if !items(&thread["comments"])
                        .iter()
                        .any(|v| v["id"].to_string().trim_matches('"') == comment)
                    {
                        bail!("Reply comment is outside the loaded provider thread");
                    }
                }
                return Ok(thread);
            }
            if value["next"].is_null() {
                break;
            }
            cursor = value["cursor"].as_str().map(str::to_owned);
        }
        bail!("Thread is unavailable in the first 200 discussions. Use the provider page for older threads.")
    }
    pub async fn reconcile(&self, c: &Connection, record: &Record) -> Result<Option<Value>> {
        let r = &record.review;
        let gh = c.site.provider == Provider::Github;
        let path = c.review_path(r);
        if record.action == "merge" {
            let details = self.details(c, r, true).await?;
            if details["merged"] == true && details["head"] == record.receipt.reviewed_sha {
                return Ok(Some(details));
            }
            return Ok(None);
        }
        if record.action == "resolve" {
            let thread = record.intent["thread"]
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("Missing resolution target"))?;
            let current = self.thread(c, r, thread, None).await?;
            return Ok((current["resolved"] == record.intent["resolved"]).then_some(current));
        }
        if !gh && record.intent["verdict"] == "approve" {
            let details = self.details(c, r, true).await?;
            if details["head"] != record.receipt.reviewed_sha {
                return Ok(None);
            }
            let approvals = self
                .read(
                    c,
                    Method::GET,
                    &format!("{path}/approvals"),
                    &[],
                    None,
                    true,
                )
                .await?;
            return Ok(items(&approvals["approved_by"])
                .into_iter()
                .find(|v| v["user"]["id"].as_u64() == record.actor.parse::<u64>().ok()));
        }
        let endpoint = if gh {
            if record.action == "submit" {
                format!("{path}/reviews")
            } else if record.action == "reply" {
                format!("{path}/comments")
            } else {
                format!("{}/issues/{}/comments", c.project(r), r.number)
            }
        } else {
            format!("{path}/notes")
        };
        for page in 1..=10 {
            let value = self
                .read(
                    c,
                    Method::GET,
                    &endpoint,
                    &[("per_page", "100".into()), ("page", page.to_string())],
                    None,
                    true,
                )
                .await?;
            let values = items(&value);
            if let Some(note) = values.iter().find(|v| {
                v[if gh { "user" } else { "author" }]["id"].as_u64()
                    == record.actor.parse::<u64>().ok()
                    && v["body"]
                        .as_str()
                        .is_some_and(|s| s.contains(&marker(&record.receipt.operation)))
            }) {
                return Ok(Some(note.clone()));
            }
            if values.len() < 100 {
                break;
            }
        }
        Ok(None)
    }
}
