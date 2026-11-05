//! Desktop workloads exercise the in-process transport, without a browser or TCP server.
use super::*;
use crate::transport::dispatch;
use std::time::{Duration, Instant};

#[tokio::test]
async fn large_review_snapshot_reads_and_reopen_stay_bounded() {
    let dir = tempfile::tempdir().unwrap();
    assert!(std::process::Command::new("git")
        .args(["init", "-q"])
        .arg(dir.path())
        .status()
        .unwrap()
        .success());
    for i in 0..1000 {
        std::fs::write(
            dir.path().join(format!("file-{i}.txt")),
            "desktop fixture\n",
        )
        .unwrap();
    }
    let config = dir.path().join(".git/mdiff-test-settings.json");
    let started = Instant::now();
    // A generous ceiling detects hangs/leaked sessions on slower CI machines.
    tokio::time::timeout(Duration::from_secs(30), async {
        for _ in 0..3 {
            let (runtime, _) = start(
                config.clone(),
                dir.path().into(),
                EnterRequest::Working,
                None,
                None,
                false,
                None,
            )
            .await
            .unwrap();
            for _ in 0..20 {
                let response = dispatch(runtime.router.clone(), "GET", "/api/snapshot", None)
                    .await
                    .unwrap();
                assert_eq!(response.status, 200);
                let snapshot: serde_json::Value = serde_json::from_slice(&response.body).unwrap();
                assert_eq!(snapshot["changed"].as_array().unwrap().len(), 1000);
            }
            runtime.close().await;
        }
    })
    .await
    .expect("large desktop review workload exceeded 30 seconds");
    eprintln!(
        "1000 files, 60 native snapshots, 3 session lifecycles: {:?}",
        started.elapsed()
    );
}
