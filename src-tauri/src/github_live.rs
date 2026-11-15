//! Opt-in checks against real GitHub. Uses a disposable clone and pre-pushed test branches.
//! Run only with MDIFF_LIVE_GITHUB=1, MDIFF_LIVE_REPO, and --ignored --nocapture.
//! The clone must target MohamedElashri/mdiff and have a fresh `mdiff-live-test/` branch
//! tracking its pushed origin branch. Change README.md line 1 and add
//! docs/mdiff-github-live-test.md with at least eight lines. These are comment anchors.
//! This creates a real draft PR and pending review; close the PR and remove the remote
//! test branch afterward. Routine tests deliberately skip it.
use super::*;
use crate::transport::dispatch;
use diffle::github::client::{resolve_token, GithubClient};
use serde_json::{json, Value};

async fn call(
    router: &axum::Router,
    method: &str,
    path: &str,
    body: Option<Value>,
) -> (u16, Value) {
    let response = dispatch(router.clone(), method, path, body.map(|v| v.to_string()))
        .await
        .unwrap();
    let value = serde_json::from_slice(&response.body)
        .unwrap_or_else(|_| json!(String::from_utf8_lossy(&response.body)));
    (response.status, value)
}
async fn ok(router: &axum::Router, method: &str, path: &str, body: Option<Value>) -> Value {
    let (status, value) = call(router, method, path, body).await;
    assert!(status < 300, "{method} {path}: {status} {value}");
    value
}
fn record(report: &mut Value, path: &std::path::Path, name: &str, value: Value) {
    println!("LIVE {name}: {value}");
    report[name] = value;
    std::fs::write(path, serde_json::to_string_pretty(report).unwrap()).unwrap();
}

#[tokio::test]
#[ignore = "Creates real GitHub PRs and pending comments; requires explicit live-test environment"]
async fn github_live_create_transfer_update_and_disable() {
    assert_eq!(std::env::var("MDIFF_LIVE_GITHUB").as_deref(), Ok("1"));
    let path = PathBuf::from(
        std::env::var("MDIFF_LIVE_REPO").expect("set MDIFF_LIVE_REPO to an isolated clone"),
    );
    let repo = GitRepo::open(&path).await.unwrap();
    let branch = repo.branch_ref("HEAD").await.unwrap().unwrap();
    assert!(
        branch.starts_with("refs/heads/mdiff-live-test/"),
        "Refusing to write from a normal development branch"
    );
    assert!(repo
        .remotes()
        .await
        .unwrap()
        .iter()
        .any(|r| r.name == "origin"
            && r.url.trim_end_matches(".git") == "https://github.com/MohamedElashri/mdiff"));
    let config = repo.git_dir.join("mdiff-live-settings/review.json");
    let report_path = repo.git_dir.join("mdiff-live-report.json");
    let mut report = json!({"repository":"MohamedElashri/mdiff","branch":branch});
    let (mut runtime, _) = start(
        config.clone(),
        path.clone(),
        EnterRequest::Working,
        None,
        None,
        None,
    )
    .await
    .unwrap();
    assert!(runtime.lsp.is_none());
    let router = runtime.router.clone();
    let context = ok(&router, "GET", "/api/desktop/github", None).await;
    assert_eq!(context["connected"], true);
    assert_eq!(context["reason"], Value::Null);
    record(&mut report, &report_path, "connection", context.clone());
    let request = json!({"repository":"MohamedElashri/mdiff","headRepository":context["headRepository"],"branch":context["branch"],"head":context["head"],"base":"main","title":"[mdiff live test] Draft PR and comment transfer — do not merge","body":"Disposable live integration test authorized by the repository owner. Tests native PR creation, draft review comments, updates, duplicate detection, and disabling. Do not merge; this PR will be closed after testing.","draft":true});
    let mut invalid = request.clone();
    invalid["head"] = json!("outdated-head");
    let (status, value) = call(&router, "POST", "/api/desktop/github", Some(invalid)).await;
    assert_eq!(status, 409);
    assert!(value["error"].as_str().unwrap().contains("branch changed"));
    record(&mut report, &report_path, "reject_changed_head", value);
    let mut invalid = request.clone();
    invalid["base"] = json!("mdiff-live-test/nonexistent-base");
    let (status, value) = call(&router, "POST", "/api/desktop/github", Some(invalid)).await;
    assert_eq!(status, 409);
    assert!(value["error"].as_str().unwrap().contains("base branch"));
    record(&mut report, &report_path, "reject_missing_base", value);
    let created = ok(
        &router,
        "POST",
        "/api/desktop/github",
        Some(request.clone()),
    )
    .await;
    let url = created["url"].as_str().unwrap().to_owned();
    let number = created["number"].as_i64().unwrap();
    record(&mut report, &report_path, "draft_pr", created.clone());
    let duplicate = ok(
        &router,
        "POST",
        "/api/desktop/github",
        Some(request.clone()),
    )
    .await;
    assert_eq!(duplicate["number"], number);
    assert_eq!(duplicate["existing"], true);
    record(&mut report, &report_path, "duplicate_pr", duplicate);
    let opened = ok(
        &router,
        "POST",
        "/api/review",
        Some(json!({"kind":"pr","pr":url})),
    )
    .await;
    assert!(
        runtime.lsp.is_none(),
        "Opening a GitHub PR must not start language servers"
    );
    record(
        &mut report,
        &report_path,
        "opened_pr",
        json!({"version":opened["version"],"languageServersStarted":false}),
    );
    let metadata = ok(&router, "GET", "/api/github", None).await;
    assert_eq!(metadata["reason"], Value::Null, "{metadata}");
    assert_eq!(metadata["pullRequest"]["isDraft"], true);
    record(&mut report, &report_path, "matching_pr", metadata);
    let path = "docs/mdiff-github-live-test.md";
    let added = ok(&router, "POST", "/api/threads", Some(json!([
        {"path":path,"body":"[mdiff live test] Whole-file comment. This is disposable test feedback."},
        {"path":path,"side":"new","startLine":5,"body":"[mdiff live test] Single-line comment."},
        {"path":path,"side":"new","startLine":6,"endLine":7,"body":"[mdiff live test] Multi-line suggestion.\n\n```suggestion\nBeta line (suggested)\nGamma line (suggested)\n```"},
        {"path":"README.md","side":"old","startLine":1,"body":"[mdiff live test] Comment on a deleted line."},
        {"path":path,"side":"new","startLine":8,"body":"[mdiff live test] Resolved locally; must not be exported."}
    ]))).await;
    assert_eq!(added.as_array().unwrap().len(), 5, "{added}");
    let resolved = added
        .as_array()
        .unwrap()
        .iter()
        .find(|t| {
            t["messages"][0]["body"]
                .as_str()
                .unwrap()
                .contains("Resolved locally")
        })
        .unwrap();
    ok(
        &router,
        "PUT",
        &format!("/api/threads/{}/resolved", resolved["id"].as_str().unwrap()),
        Some(json!({"resolved":true})),
    )
    .await;
    let exported = ok(&router, "POST", "/api/github/export", Some(json!({}))).await;
    assert_eq!(exported["posted"], 4, "{exported}");
    record(&mut report, &report_path, "exported", exported);
    let unchanged = ok(&router, "POST", "/api/github/export", Some(json!({}))).await;
    assert_eq!(unchanged["posted"], 0, "{unchanged}");
    assert_eq!(unchanged["updated"], 0);
    assert_eq!(unchanged["skipped"].as_array().unwrap().len(), 4);
    record(&mut report, &report_path, "repeat_export", unchanged);
    let thread = added
        .as_array()
        .unwrap()
        .iter()
        .find(|t| {
            t["messages"][0]["body"]
                .as_str()
                .unwrap()
                .contains("Single-line")
        })
        .unwrap();
    ok(
        &router,
        "PATCH",
        &format!(
            "/api/threads/{}/messages/{}",
            thread["id"].as_str().unwrap(),
            thread["messages"][0]["id"].as_str().unwrap()
        ),
        Some(json!({"body":"[mdiff live test] Single-line comment updated in place."})),
    )
    .await;
    let updated = ok(&router, "POST", "/api/github/export", Some(json!({}))).await;
    assert_eq!(updated["posted"], 0, "{updated}");
    assert_eq!(updated["updated"], 1);
    record(&mut report, &report_path, "update_export", updated);
    let (token, _) = resolve_token("github.com", |name| std::env::var(name).ok())
        .await
        .unwrap();
    let client = HttpClient::new(token, "https://api.github.com").unwrap();
    let remote = client.graphql("query($n:Int!){repository(owner:\"MohamedElashri\",name:\"mdiff\"){pullRequest(number:$n){url isDraft reviews(last:10,states:[PENDING]){nodes{id state comments(first:100){nodes{body path line startLine diffHunk}}}} reviewThreads(first:100){nodes{path line startLine diffSide subjectType comments(first:10){nodes{body}}}}}}}", json!({"n":number}), None).await.unwrap();
    let pr = &remote["repository"]["pullRequest"];
    let pending = pr["reviews"]["nodes"].as_array().unwrap();
    assert_eq!(pending.len(), 1, "{remote}");
    assert_eq!(pending[0]["state"], "PENDING");
    assert_eq!(
        pending[0]["comments"]["nodes"].as_array().unwrap().len(),
        4,
        "{remote}"
    );
    let remote_text = remote.to_string();
    assert!(remote_text.contains("updated in place"));
    assert!(remote_text.contains("```suggestion"));
    assert!(!remote_text.contains("Resolved locally"));
    record(&mut report, &report_path, "remote_verification", remote);
    let local = ok(&router, "GET", "/api/threads", None).await;
    assert_eq!(local.as_array().unwrap().len(), 5);
    record(&mut report, &report_path, "local_comments_kept", json!(5));
    ok(
        &router,
        "POST",
        "/api/review",
        Some(json!({"kind":"working"})),
    )
    .await;
    let (status, value) = call(&router, "POST", "/api/github/export", Some(json!({}))).await;
    assert_eq!(status, 409, "{value}");
    record(&mut report, &report_path, "reject_worktree_export", value);
    crate::github::Connections {
        github_enabled: false,
    }
    .save(&config)
    .await
    .unwrap();
    runtime
        .github
        .enabled
        .store(false, std::sync::atomic::Ordering::SeqCst);
    for (method, endpoint, payload) in [
        ("GET", "/api/github", None),
        ("GET", "/api/desktop/github", None),
        ("POST", "/api/desktop/github", Some(request)),
        ("POST", "/api/github/export", Some(json!({}))),
        ("POST", "/api/review", Some(json!({"kind":"pr","pr":url}))),
    ] {
        assert_eq!(
            call(&router, method, endpoint, payload).await.0,
            403,
            "{endpoint}"
        );
    }
    runtime.enable_lsp().await.unwrap();
    assert_eq!(
        call(&runtime.router, "GET", "/api/desktop/github", None)
            .await
            .0,
        403
    );
    assert_eq!(
        call(&runtime.router, "GET", "/api/snapshot", None).await.0,
        200
    );
    runtime.close().await;
    let (runtime, opened) = start(
        config.clone(),
        repo.root.clone(),
        EnterRequest::Working,
        None,
        None,
        None,
    )
    .await
    .unwrap();
    assert!(!opened.github_enabled);
    assert_eq!(
        call(&runtime.router, "GET", "/api/github", None).await.0,
        403
    );
    runtime.close().await;
    record(
        &mut report,
        &report_path,
        "disable_and_reopen",
        json!("passed"),
    );
    crate::github::Connections {
        github_enabled: true,
    }
    .save(&config)
    .await
    .unwrap();
    let (runtime, _) = start(
        config,
        repo.root.clone(),
        EnterRequest::Pr { pr: Some(url) },
        None,
        None,
        None,
    )
    .await
    .unwrap();
    assert!(runtime.lsp.is_none());
    assert_eq!(
        ok(&runtime.router, "GET", "/api/github", None).await["reason"],
        Value::Null
    );
    runtime.close().await;
    record(
        &mut report,
        &report_path,
        "reenable_and_reopen",
        json!("passed"),
    );
    println!("LIVE REPORT {}", report_path.display());
}
