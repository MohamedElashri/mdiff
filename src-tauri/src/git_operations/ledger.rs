use anyhow::{Context, Result};
use diffle::{
    git::operations::{Outcome, RemoteProof, Snapshot},
    persist::{with_file_lock, write_file_atomic},
};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Receipt {
    pub operation: String,
    #[serde(default)]
    pub target_root: Option<String>,
    #[serde(default)]
    pub intent: serde_json::Value,
    pub root: String,
    pub label: String,
    pub status: String,
    pub message: String,
    pub before: Option<Snapshot>,
    pub after: Option<Snapshot>,
    pub remote: Option<RemoteProof>,
}
impl Receipt {
    pub fn finish(&mut self, outcome: Outcome) {
        self.status = outcome.status;
        self.message = outcome.message.chars().take(2048).collect();
        self.after = outcome.after;
    }
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Document {
    version: u8,
    records: Vec<Receipt>,
}
pub struct Ledger(pub PathBuf);
impl Ledger {
    pub async fn list(&self) -> Result<Vec<Receipt>> {
        if !self.0.exists() {
            return Ok(vec![]);
        }
        anyhow::ensure!(
            tokio::fs::metadata(&self.0).await?.len() <= 2 * 1024 * 1024,
            "Git operation receipts exceed their limit; original file was preserved"
        );
        let doc: Document = serde_json::from_slice(&tokio::fs::read(&self.0).await?)?;
        anyhow::ensure!(
            doc.version == 1 && doc.records.len() <= 128,
            "Unsupported Git operation receipts; original file was preserved"
        );
        Ok(doc.records)
    }
    pub async fn put(&self, receipt: Receipt) -> Result<()> {
        with_file_lock(&self.0, || async {
            let mut records = self.list().await?;
            if let Some(old) = records
                .iter_mut()
                .find(|r| r.operation == receipt.operation)
            {
                *old = receipt;
            } else {
                while records.len() >= 128 {
                    let at = records
                        .iter()
                        .position(|r| {
                            matches!(
                                r.status.as_str(),
                                "completed" | "rejected" | "observed" | "noChanges"
                            )
                        })
                        .context("Inspect outstanding Git operations before starting more")?;
                    records.remove(at);
                }
                records.push(receipt);
            }
            let text = serde_json::to_string(&Document {
                version: 1,
                records,
            })?;
            anyhow::ensure!(
                text.len() <= 2 * 1024 * 1024,
                "Git receipt limit reached; no operation was started"
            );
            write_file_atomic(&self.0, &text).await
        })
        .await
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn record(id: &str, status: &str) -> Receipt {
        Receipt {
            operation: id.into(),
            target_root: None,
            intent: serde_json::Value::Null,
            root: "/fixture".into(),
            label: "Commit staged changes".into(),
            status: status.into(),
            message: "Native job accepted".into(),
            before: None,
            after: None,
            remote: None,
        }
    }
    #[tokio::test]
    async fn pending_jobs_survive_reload_and_corrupt_ledgers_are_preserved() {
        let temp = tempfile::tempdir().unwrap();
        let file = temp.path().join("operations.json");
        let ledger = Ledger(file.clone());
        ledger.put(record("one", "pending")).await.unwrap();
        assert_eq!(
            Ledger(file.clone()).list().await.unwrap()[0].status,
            "pending"
        );
        tokio::fs::write(&file, "future version").await.unwrap();
        assert!(ledger.put(record("two", "pending")).await.is_err());
        assert_eq!(
            tokio::fs::read_to_string(file).await.unwrap(),
            "future version"
        );
    }
    #[tokio::test]
    async fn receipts_are_bounded_and_do_not_evict_unknown_operations() {
        let temp = tempfile::tempdir().unwrap();
        let ledger = Ledger(temp.path().join("ops.json"));
        let records = (0..128)
            .map(|n| record(&n.to_string(), "needsInspection"))
            .collect();
        let doc = Document {
            version: 1,
            records,
        };
        tokio::fs::write(&ledger.0, serde_json::to_string(&doc).unwrap())
            .await
            .unwrap();
        assert!(ledger.put(record("extra", "pending")).await.is_err());
        ledger.put(record("0", "observed")).await.unwrap();
        ledger.put(record("extra", "pending")).await.unwrap();
        assert_eq!(ledger.list().await.unwrap().len(), 128);
    }
}
