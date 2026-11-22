//! Durable write receipts. Pending/unknown operations are reconciled, never blindly reposted.
use super::types::*;
use anyhow::{bail, Result};
use diffle::persist::{with_file_lock, write_file_atomic};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::PathBuf;
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Receipt {
    pub operation: String,
    pub status: String,
    pub reviewed_sha: String,
    pub url: String,
    pub message: String,
    pub remote_id: Option<String>,
    #[serde(default)]
    pub result_sha: Option<String>,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Record {
    pub fingerprint: String,
    pub actor: String,
    pub review: ReviewRef,
    pub action: String,
    pub intent: Value,
    pub receipt: Receipt,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Document {
    version: u8,
    records: Vec<Record>,
}
pub struct Ledger {
    pub file: PathBuf,
}
impl Ledger {
    async fn read(&self) -> Result<Document> {
        if !self.file.exists() {
            return Ok(Document {
                version: 1,
                records: vec![],
            });
        }
        anyhow::ensure!(
            tokio::fs::metadata(&self.file).await?.len() <= 1024 * 1024,
            "Review operation history exceeds its limit; original file was preserved"
        );
        let doc: Document = serde_json::from_slice(&tokio::fs::read(&self.file).await?)?;
        anyhow::ensure!(
            doc.version == 1 && doc.records.len() <= 256,
            "Unsupported review operation history; original file was preserved"
        );
        Ok(doc)
    }
    pub async fn find(&self, id: &str) -> Result<Record> {
        self.read()
            .await?
            .records
            .into_iter()
            .find(|r| r.receipt.operation == id)
            .ok_or_else(|| anyhow::anyhow!("Operation receipt is unavailable"))
    }
    pub async fn begin(&self, request: &Write, actor: &str) -> Result<(Record, bool)> {
        identifier(&request.operation)?;
        let fingerprint = crate::storage::digest(&serde_json::to_string(&(
            actor,
            &request.review,
            &request.expected_head,
            &request.action,
        ))?);
        with_file_lock(&self.file, || async {
            let mut doc = self.read().await?;
            if let Some(existing) = doc
                .records
                .iter()
                .find(|r| r.receipt.operation == request.operation)
            {
                if existing.fingerprint != fingerprint {
                    bail!("Operation identifier was already used for different content");
                }
                return Ok((existing.clone(), false));
            }
            if let Some(existing) = doc
                .records
                .iter()
                .find(|r| r.fingerprint == fingerprint && r.receipt.status != "rejected")
            {
                return Ok((existing.clone(), false));
            }
            while doc.records.len() >= 256 {
                let at = doc
                    .records
                    .iter()
                    .position(|r| matches!(r.receipt.status.as_str(), "succeeded" | "rejected"))
                    .ok_or_else(|| {
                        anyhow::anyhow!("Reconcile pending review operations before starting more")
                    })?;
                doc.records.remove(at);
            }
            let action = match request.action {
                Action::Comment { .. } => "comment",
                Action::Reply { .. } => "reply",
                Action::Resolve { .. } => "resolve",
                Action::Submit { .. } => "submit",
                Action::Merge { .. } => "merge",
            }
            .into();
            let mut intent = serde_json::to_value(&request.action)?;
            if let Some(obj) = intent.as_object_mut() {
                obj.remove("body");
            }
            let record = Record {
                intent,
                fingerprint,
                actor: actor.into(),
                review: request.review.clone(),
                action,
                receipt: Receipt {
                    operation: request.operation.clone(),
                    status: "pending".into(),
                    reviewed_sha: request.expected_head.clone(),
                    url: request.review.url(),
                    message:
                        "Request prepared. Reconcile before retrying if its outcome is unknown."
                            .into(),
                    remote_id: None,
                    result_sha: None,
                },
            };
            doc.records.push(record.clone());
            write_file_atomic(&self.file, &encode(&doc)?).await?;
            Ok((record, true))
        })
        .await
    }
    pub async fn finish(
        &self,
        mut record: Record,
        status: &str,
        message: &str,
        value: Option<&Value>,
    ) -> Result<Receipt> {
        record.receipt.result_sha = if status == "succeeded" {
            value.and_then(|value| {
                ["mergeCommit", "merge_commit_sha", "sha"]
                    .iter()
                    .find_map(|key| {
                        value[*key]
                            .as_str()
                            .filter(|value| sha(value).is_ok())
                            .map(str::to_owned)
                    })
            })
        } else {
            None
        };
        record.receipt.status = status.into();
        record.receipt.message = message.chars().take(2048).collect();
        record.receipt.remote_id = value.and_then(|v| v.get("id")).map(|v| {
            v.as_str()
                .map(str::to_owned)
                .unwrap_or_else(|| v.to_string())
        });
        with_file_lock(&self.file, || async {
            let mut doc = self.read().await?;
            let target = doc
                .records
                .iter_mut()
                .find(|r| r.receipt.operation == record.receipt.operation)
                .ok_or_else(|| anyhow::anyhow!("Operation record disappeared"))?;
            *target = record.clone();
            write_file_atomic(&self.file, &encode(&doc)?).await?;
            Ok(record.receipt)
        })
        .await
    }
}

fn encode(doc: &Document) -> Result<String> {
    let value = serde_json::to_string(doc)?;
    anyhow::ensure!(
        value.len() <= 1024 * 1024,
        "Review operation history is full; existing receipts were preserved"
    );
    Ok(value)
}
