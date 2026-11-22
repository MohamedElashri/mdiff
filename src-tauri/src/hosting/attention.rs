//! Summary-only reads for the process-owned inbox. No threads, logs, mutations or CLI discovery here.
use super::{http::Broker, types::*, Connection};
use anyhow::Result;
use reqwest::Method;
use serde_json::Value;
pub struct Requested {
    pub reviews: Vec<(ReviewRef, String)>,
    pub complete: bool,
}
pub struct Checks {
    pub head: String,
    pub failed: Vec<String>,
    pub complete: bool,
    pub open: bool,
}
impl Broker {
    pub async fn attention_requested(
        &self,
        c: &Connection,
        repository: &str,
        actor: Option<u64>,
    ) -> Result<Requested> {
        anyhow::ensure!(
            c.token.is_some(),
            "Connect this host to monitor requested reviews"
        );
        let (path, args, key) = if c.site.provider == Provider::Github {
            (
                "/search/issues".into(),
                vec![
                    (
                        "q",
                        format!("is:pr is:open review-requested:@me repo:{repository}"),
                    ),
                    ("per_page", "20".into()),
                    ("sort", "updated".into()),
                ],
                "items",
            )
        } else {
            let id =
                actor.ok_or_else(|| anyhow::anyhow!("Connect an identifiable GitLab account"))?;
            let encoded =
                url::form_urlencoded::byte_serialize(repository.as_bytes()).collect::<String>();
            (
                format!("/projects/{encoded}/merge_requests"),
                vec![
                    ("state", "opened".into()),
                    ("reviewer_id", id.to_string()),
                    ("per_page", "20".into()),
                    ("order_by", "updated_at".into()),
                ],
                "",
            )
        };
        let data = self.read(c, Method::GET, &path, &args, None, false).await?;
        let rows = if key.is_empty() { &data } else { &data[key] };
        anyhow::ensure!(
            rows.is_array(),
            "Requested review summary schema is unavailable"
        );
        let values = items(rows);
        let count = values.len();
        let complete = values.len() < 20 && data["incomplete_results"] != true;
        let mut reviews = vec![];
        for v in values {
            let url = v
                .get("html_url")
                .or_else(|| v.get("web_url"))
                .and_then(Value::as_str)
                .unwrap_or("");
            if let Ok(mut r) = ReviewRef::parse(url, c.site.provider) {
                if c.site.provider == Provider::Github {
                    r.repository.make_ascii_lowercase();
                }
                if r.site == c.site
                    && if c.site.provider == Provider::Github {
                        r.repository.eq_ignore_ascii_case(repository)
                    } else {
                        r.repository == repository
                    }
                {
                    reviews.push((r, text(&v, "title")));
                }
            }
        }
        let complete = complete && reviews.len() == count;
        Ok(Requested { reviews, complete })
    }
    pub async fn attention_checks(&self, c: &Connection, r: &ReviewRef) -> Result<Checks> {
        r.validate()?;
        anyhow::ensure!(r.site == c.site, "Review host mismatch");
        let raw = self
            .read(c, Method::GET, &c.review_path(r), &[], None, false)
            .await?;
        let gh = c.site.provider == Provider::Github;
        let head = if gh {
            raw.pointer("/head/sha")
        } else {
            raw.get("sha")
                .or_else(|| raw.pointer("/diff_refs/head_sha"))
        }
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow::anyhow!("Review head unavailable"))?
        .to_owned();
        sha(&head)?;
        let open = raw["state"] == "open" || raw["state"] == "opened";
        if !open {
            return Ok(Checks {
                head,
                failed: vec![],
                complete: true,
                open,
            });
        }
        let project = c.project(r);
        let mut failed = vec![];
        let mut complete = true;
        if gh {
            let runs = self
                .read(
                    c,
                    Method::GET,
                    &format!("{project}/commits/{head}/check-runs"),
                    &[("per_page", "20".into())],
                    None,
                    false,
                )
                .await?;
            anyhow::ensure!(
                runs["check_runs"].is_array(),
                "Check summary schema unavailable"
            );
            let values = items(&runs["check_runs"]);
            complete &= runs["total_count"]
                .as_u64()
                .is_some_and(|n| n <= values.len() as u64);
            for v in values {
                if failed_status(v["conclusion"].as_str().unwrap_or("")) {
                    failed.push(text(&v, "name"));
                }
            }
            let statuses = self
                .read(
                    c,
                    Method::GET,
                    &format!("{project}/commits/{head}/status"),
                    &[("per_page", "20".into())],
                    None,
                    false,
                )
                .await?;
            anyhow::ensure!(
                statuses["statuses"].is_array(),
                "Status summary schema unavailable"
            );
            let values = items(&statuses["statuses"]);
            complete &= statuses["total_count"]
                .as_u64()
                .is_some_and(|n| n <= values.len() as u64);
            for v in values {
                if failed_status(v["state"].as_str().unwrap_or("")) {
                    failed.push(text(&v, "context"));
                }
            }
        } else {
            let runs = self
                .read(
                    c,
                    Method::GET,
                    &format!("{project}/pipelines"),
                    &[("sha", head.clone()), ("per_page", "20".into())],
                    None,
                    false,
                )
                .await?;
            anyhow::ensure!(runs.is_array(), "Pipeline summary schema unavailable");
            let values = items(&runs);
            complete &= values.len() < 20;
            // Only the newest pipeline for a ref is current; an older failed retry is not an outstanding failure.
            let mut refs = std::collections::HashSet::new();
            for v in values {
                if refs.insert(text(&v, "ref")) && failed_status(v["status"].as_str().unwrap_or(""))
                {
                    failed.push(format!("Pipeline {}", v["id"]));
                }
            }
        }
        Ok(Checks {
            head,
            failed,
            complete,
            open,
        })
    }
}
fn failed_status(s: &str) -> bool {
    matches!(
        s,
        "failure" | "failed" | "error" | "timed_out" | "action_required" | "startup_failure"
    )
}
