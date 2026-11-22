use super::*;
use axum::{
    body::{to_bytes, Body},
    http::{HeaderMap, Request, Response, StatusCode},
    Router,
};
use reqwest::Method;
use std::sync::{Arc, Mutex};
use url::Url;
const SHA: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const OTHER: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
type Captured = Arc<Mutex<Vec<(String, HeaderMap, Value)>>>;
type Answer = (u16, Vec<(&'static str, String)>, Value);
struct Api {
    base: Url,
    seen: Captured,
    server: tokio::task::JoinHandle<()>,
}
impl Drop for Api {
    fn drop(&mut self) {
        self.server.abort();
    }
}
async fn api(answer: impl Fn(&str, &Value) -> Answer + Send + Sync + 'static) -> Api {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = Url::parse(&format!("http://{}", listener.local_addr().unwrap())).unwrap();
    let seen: Captured = Arc::default();
    let captures = seen.clone();
    let answer = Arc::new(answer);
    let router = Router::new().fallback(move |request: Request<Body>| {
        let seen = captures.clone();
        let answer = answer.clone();
        async move {
            let (parts, body) = request.into_parts();
            let bytes = to_bytes(body, 4 * 1024 * 1024).await.unwrap();
            let body = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
            let path = parts.uri.to_string();
            seen.lock().unwrap().push((
                format!("{} {path}", parts.method),
                parts.headers,
                body.clone(),
            ));
            let (status, headers, value) = answer(&path, &body);
            let mut response = Response::builder().status(status);
            for (k, v) in headers {
                response = response.header(k, v);
            }
            response
                .body(Body::from(if status == 304 {
                    String::new()
                } else {
                    value.to_string()
                }))
                .unwrap()
        }
    });
    let server = tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });
    Api { base, seen, server }
}
fn review(provider: Provider) -> ReviewRef {
    ReviewRef {
        site: Site {
            provider,
            host: "provider.test".into(),
        },
        repository: "team/repo".into(),
        number: 7,
    }
}
fn conn(api: &Api, provider: Provider) -> Connection {
    let mut c = Connection::new(review(provider).site, Some("test-secret".into())).unwrap();
    c.base = api.base.clone();
    c
}
fn write(action: Action) -> Write {
    Write {
        operation: "op-1".into(),
        review: review(Provider::Github),
        expected_head: SHA.into(),
        action,
    }
}
#[test]
fn canonical_hosts_urls_and_provider_paths_do_not_accept_credentials_or_endpoint_injection() {
    for host in [
        "token@github.com",
        "example.com/path",
        "example.com?x=y",
        "EXAMPLE.com",
        "example.com\\other",
        "example.com#x",
    ] {
        assert!(
            Site {
                provider: Provider::Github,
                host: host.into()
            }
            .validate()
            .is_err(),
            "{host}"
        );
    }
    let gh = Connection::new(
        Site {
            provider: Provider::Github,
            host: "enterprise.test:8443".into(),
        },
        None,
    )
    .unwrap();
    assert_eq!(
        gh.url("/graphql", &[]).unwrap().as_str(),
        "https://enterprise.test:8443/api/graphql"
    );
    assert!(gh.url("/../other", &[]).is_err());
    let r = review(Provider::Gitlab);
    let gl = Connection::new(r.site.clone(), None).unwrap();
    assert!(gl
        .url(&gl.review_path(&r), &[])
        .unwrap()
        .as_str()
        .contains("/projects/team%2Frepo/merge_requests/7"));
    assert_eq!(ReviewRef::parse(&r.url(), Provider::Gitlab).unwrap(), r);
    assert!(ReviewRef::parse(
        "https://secret@provider.test/team/repo/pull/7",
        Provider::Github
    )
    .is_err());
}
#[tokio::test]
async fn reads_deduplicate_conditionally_revalidate_and_separate_host_credentials_and_epochs() {
    let api = api(|_, _| (200, vec![("etag", "\"one\"".into())], json!({"ok":true}))).await;
    let c = conn(&api, Provider::Github);
    let broker = Broker::default();
    let (a, b) = tokio::join!(
        broker.read(&c, Method::GET, "/user", &[], None, false),
        broker.read(&c, Method::GET, "/user", &[], None, false)
    );
    assert!(a.is_ok() && b.is_ok());
    assert_eq!(api.seen.lock().unwrap().len(), 1);
    broker
        .read(&c, Method::GET, "/user", &[], None, true)
        .await
        .unwrap();
    assert_eq!(api.seen.lock().unwrap()[1].1["if-none-match"], "\"one\"");
    let mut changed = c.clone();
    changed.token = Some("other-account".into());
    broker
        .read(&changed, Method::GET, "/user", &[], None, false)
        .await
        .unwrap();
    assert_eq!(api.seen.lock().unwrap().len(), 3);
    let pause = broker.pause();
    assert!(broker
        .read(&c, Method::GET, "/user", &[], None, false)
        .await
        .is_err());
    broker.clear().await;
    drop(pause);
    broker
        .read(&c, Method::GET, "/user", &[], None, false)
        .await
        .unwrap();
    assert_eq!(api.seen.lock().unwrap().len(), 4);
}
#[tokio::test]
async fn rate_limits_do_not_sleep_or_retry_mutations_and_response_sizes_are_bounded() {
    let api = api(|_, _| (429, vec![("retry-after", "120".into())], json!({}))).await;
    let c = conn(&api, Provider::Github);
    let b = Broker::default();
    assert!(b
        .write(&c, Method::POST, "/write", json!({}))
        .await
        .unwrap_err()
        .downcast_ref::<http::Rejection>()
        .is_some());
    assert!(b
        .write(&c, Method::POST, "/write", json!({}))
        .await
        .unwrap_err()
        .to_string()
        .contains("rate limit"));
    assert_eq!(api.seen.lock().unwrap().len(), 1);
    let large = api_large().await;
    assert!(Broker::default()
        .read(
            &conn(&large, Provider::Github),
            Method::GET,
            "/large",
            &[],
            None,
            false
        )
        .await
        .unwrap_err()
        .to_string()
        .contains("payload limit"));
}
async fn api_large() -> Api {
    api(|_, _| (200, vec![], json!({"data":"x".repeat(2*1024*1024)}))).await
}
#[tokio::test]
async fn redirects_never_forward_provider_authorization_and_logs_are_bounded() {
    let destination = api(|_, _| (200, vec![], json!({"ok":true}))).await;
    let url = destination.base.to_string();
    let source = api(move |_, _| (302, vec![("location", url.clone())], json!({}))).await;
    let c = conn(&source, Provider::Github);
    let b = Broker::default();
    assert!(b
        .read(&c, Method::GET, "/user", &[], None, false)
        .await
        .is_err());
    assert!(b
        .log(&c, "/logs")
        .await
        .unwrap_err()
        .to_string()
        .contains("Unsafe"));
    assert!(destination.seen.lock().unwrap().is_empty());
    let logs = api(|_, _| (200, vec![], json!("x".repeat(200000)))).await;
    let value = b
        .log(&conn(&logs, Provider::Gitlab), "/trace")
        .await
        .unwrap();
    assert_eq!(value["text"].as_str().unwrap().len(), 128 * 1024);
    assert_eq!(value["truncated"], true);
}
#[tokio::test]
async fn provider_authorization_headers_are_distinct() {
    let api = api(|_, _| (200, vec![], json!({}))).await;
    let b = Broker::default();
    b.user(&conn(&api, Provider::Github)).await.unwrap();
    b.user(&conn(&api, Provider::Gitlab)).await.unwrap();
    let seen = api.seen.lock().unwrap();
    assert_eq!(seen[0].1["authorization"], "Bearer test-secret");
    assert!(!seen[0].1.contains_key("private-token"));
    assert_eq!(seen[1].1["private-token"], "test-secret");
    assert!(!seen[1].1.contains_key("authorization"));
}
#[tokio::test]
async fn exact_sha_checks_exclude_wrong_head_and_jobs_reject_wrong_revision_before_logs() {
    let api=api(|p,_|if p.contains("check-runs"){(200,vec![],json!({"check_runs":[{"id":1,"head_sha":OTHER,"name":"wrong","status":"completed","conclusion":"success"},{"id":2,"head_sha":SHA,"name":"right","status":"in_progress"}]}))}else if p.contains("/statuses"){(403,vec![],json!({}))}else if p.contains("/actions/runs?"){(200,vec![],json!({"workflow_runs":[{"id":3,"head_sha":OTHER,"name":"wrong workflow","run_attempt":2}]}))}else{(200,vec![],json!({"head_sha":OTHER,"run_attempt":1}))}).await;
    let c = conn(&api, Provider::Github);
    let b = Broker::default();
    let result = b
        .checks(&c, &review(Provider::Github), SHA, 1)
        .await
        .unwrap();
    assert_eq!(result["items"].as_array().unwrap().len(), 1);
    assert_eq!(result["items"][0]["name"], "right");
    assert_eq!(result["warnings"].as_array().unwrap().len(), 1);
    assert!(b
        .jobs(&c, &review(Provider::Github), SHA, 5, Some(2), 1)
        .await
        .is_err());
    assert!(b
        .job_log(&c, &review(Provider::Github), SHA, 5)
        .await
        .is_err());
    assert!(!api
        .seen
        .lock()
        .unwrap()
        .iter()
        .any(|(p, _, _)| p.ends_with("/logs")));
}
#[tokio::test]
async fn durable_ledger_deduplicates_after_restart_without_storing_tokens_or_bodies() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("operations.json");
    let ledger = Ledger { file: file.clone() };
    let request = write(Action::Submit {
        verdict: "approve".into(),
        body: "private draft".into(),
    });
    let (record, new) = ledger.begin(&request, "12").await.unwrap();
    assert!(new);
    assert_eq!(record.receipt.status, "pending");
    let restarted = Ledger { file: file.clone() };
    let (_, new) = restarted.begin(&request, "12").await.unwrap();
    assert!(!new);
    let mut duplicate = request.clone();
    duplicate.operation = "op-2".into();
    let (again, new) = restarted.begin(&duplicate, "12").await.unwrap();
    assert!(!new);
    assert_eq!(again.receipt.operation, "op-1");
    let mut conflict = request.clone();
    conflict.expected_head = OTHER.into();
    assert!(restarted.begin(&conflict, "12").await.is_err());
    let text = std::fs::read_to_string(file).unwrap();
    assert!(!text.contains("private draft"));
    assert!(!text.contains("test-secret"));
    assert!(text.contains("approve"));
    ledger
        .finish(record, "unknown", "Disconnected", None)
        .await
        .unwrap();
    assert_eq!(
        restarted.find("op-1").await.unwrap().receipt.status,
        "unknown"
    );
}
#[tokio::test]
async fn reviews_and_merges_send_provider_side_commit_preconditions_and_markers() {
    let api = api(|_, _| (200, vec![], json!({"id":9,"merged":true}))).await;
    let b = Broker::default();
    let c = conn(&api, Provider::Github);
    b.mutate(
        &c,
        &write(Action::Submit {
            verdict: "requestChanges".into(),
            body: "Please revise".into(),
        }),
    )
    .await
    .unwrap();
    b.mutate(
        &c,
        &write(Action::Merge {
            method: "squash".into(),
        }),
    )
    .await
    .unwrap();
    let seen = api.seen.lock().unwrap();
    assert_eq!(seen[0].2["commit_id"], SHA);
    assert_eq!(seen[0].2["event"], "REQUEST_CHANGES");
    assert!(seen[0].2["body"]
        .as_str()
        .unwrap()
        .contains("mdiff-operation:op-1"));
    assert_eq!(seen[1].2["sha"], SHA);
    assert_eq!(seen[1].2["merge_method"], "squash");
}
#[tokio::test]
async fn gitlab_approves_with_sha_and_does_not_fabricate_request_changes_or_merge_methods() {
    let api=api(|p,_|if p.contains("merge_requests"){(200,vec![],json!({"sha":SHA,"state":"opened","user":{"can_merge":true},"diff_refs":{"base_sha":OTHER}}))}else{(200,vec![],json!({"squash_option":"always"}))}).await;
    let c = conn(&api, Provider::Gitlab);
    let b = Broker::default();
    let r = review(Provider::Gitlab);
    let d = b.details(&c, &r, false).await.unwrap();
    assert_eq!(d["capabilities"]["mergeMethods"], json!(["squash"]));
    assert_eq!(d["capabilities"]["review"], json!(["comment", "approve"]));
    let mut w = write(Action::Submit {
        verdict: "approve".into(),
        body: "kept locally".into(),
    });
    w.review = r;
    b.mutate(&c, &w).await.unwrap();
    assert_eq!(api.seen.lock().unwrap().last().unwrap().2["sha"], SHA);
    w.action = Action::Submit {
        verdict: "requestChanges".into(),
        body: "unsupported".into(),
    };
    let count = api.seen.lock().unwrap().len();
    assert!(b.mutate(&c, &w).await.is_err());
    assert_eq!(api.seen.lock().unwrap().len(), count);
}
#[tokio::test]
async fn uncertain_reviews_reconcile_only_the_original_actor_and_operation_marker() {
    let api=api(|_,_|(200,vec![],json!([{"id":1,"user":{"id":99},"body":"<!-- mdiff-operation:op-1 -->"},{"id":2,"user":{"id":12},"body":"<!-- mdiff-operation:op-1 -->"}]))).await;
    let dir = tempfile::tempdir().unwrap();
    let ledger = Ledger {
        file: dir.path().join("ops"),
    };
    let (record, _) = ledger
        .begin(
            &write(Action::Submit {
                verdict: "comment".into(),
                body: "message".into(),
            }),
            "12",
        )
        .await
        .unwrap();
    let result = Broker::default()
        .reconcile(&conn(&api, Provider::Github), &record)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(result["id"], 2);
}
#[tokio::test]
async fn connection_changes_cancel_inflight_reads_without_waiting_for_the_provider() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = Url::parse(&format!("http://{}", listener.local_addr().unwrap())).unwrap();
    let (started, mut notice) = tokio::sync::mpsc::channel(1);
    let server = tokio::spawn(async move {
        axum::serve(
            listener,
            Router::new().fallback(move || {
                let started = started.clone();
                async move {
                    let _ = started.send(()).await;
                    tokio::time::sleep(std::time::Duration::from_secs(30)).await;
                    StatusCode::OK
                }
            }),
        )
        .await
        .unwrap()
    });
    let b = Arc::new(Broker::default());
    let mut c = Connection::new(review(Provider::Github).site, None).unwrap();
    c.base = base;
    let task = {
        let b = b.clone();
        tokio::spawn(async move { b.user(&c).await })
    };
    notice.recv().await.unwrap();
    let pause = b.pause();
    let result = tokio::time::timeout(std::time::Duration::from_secs(1), task)
        .await
        .unwrap()
        .unwrap();
    assert!(result.is_err());
    drop(pause);
    server.abort();
}
#[test]
fn submission_preflight_rejects_drift_mismatched_bases_closed_reviews_and_forbidden_merge_methods()
{
    let request = write(Action::Merge {
        method: "squash".into(),
    });
    let good =
        json!({"head":SHA,"state":"open","draft":false,"capabilities":{"mergeMethods":["squash"]}});
    assert!(validate_provider(&request, OTHER, SHA, OTHER, &good).is_ok());
    assert!(validate_provider(&request, OTHER, OTHER, OTHER, &good).is_err());
    assert!(validate_provider(&request, OTHER, SHA, SHA, &good).is_err());
    for override_value in [
        json!({"head":OTHER}),
        json!({"state":"closed"}),
        json!({"draft":true}),
        json!({"capabilities":{"mergeMethods":[]}}),
    ] {
        let mut details = good.clone();
        for (k, v) in override_value.as_object().unwrap() {
            details[k] = v.clone();
        }
        assert!(validate_provider(&request, OTHER, SHA, OTHER, &details).is_err());
    }
}
#[tokio::test]
async fn write_dispatch_records_uncertain_results_before_returning_and_never_reposts_after_restart()
{
    let api = api(|_, _| (500, vec![], json!({"error":"private provider details"}))).await;
    let broker = Broker::default();
    let c = conn(&api, Provider::Github);
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("operations");
    let ledger = Ledger { file: file.clone() };
    let request = write(Action::Comment {
        body: "message".into(),
    });
    let mut prepared = false;
    let first = dispatch(&broker, &c, &ledger, &request, "12", &mut prepared)
        .await
        .unwrap();
    assert!(prepared);
    assert_eq!(first.status, "unknown");
    assert!(!first.message.contains("private provider details"));
    let restarted = Ledger { file };
    let mut second = request.clone();
    second.operation = "new-click".into();
    let receipt = dispatch(&broker, &c, &restarted, &second, "12", &mut false)
        .await
        .unwrap();
    assert_eq!(receipt.operation, first.operation);
    assert_eq!(api.seen.lock().unwrap().len(), 1);
}
#[tokio::test]
async fn asynchronous_merge_is_not_reported_as_completed_and_rejection_is_distinct() {
    let api = api(|p, _| {
        if p.ends_with("/merge") {
            (202, vec![], json!({"merged":false}))
        } else {
            (409, vec![], json!({}))
        }
    })
    .await;
    let c = conn(&api, Provider::Github);
    let broker = Broker::default();
    let dir = tempfile::tempdir().unwrap();
    let ledger = Ledger {
        file: dir.path().join("ops"),
    };
    let result = dispatch(
        &broker,
        &c,
        &ledger,
        &write(Action::Merge {
            method: "squash".into(),
        }),
        "12",
        &mut false,
    )
    .await
    .unwrap();
    assert_eq!(result.status, "unknown");
    let mut request = write(Action::Comment {
        body: "message".into(),
    });
    request.operation = "op-2".into();
    let result = dispatch(&broker, &c, &ledger, &request, "12", &mut false)
        .await
        .unwrap();
    assert_eq!(result.status, "rejected");
}
#[tokio::test]
async fn cache_payloads_have_an_aggregate_bound_in_addition_to_per_response_limits() {
    let api = api(|_, _| (200, vec![], json!({"payload":"x".repeat(900000)}))).await;
    let c = conn(&api, Provider::Github);
    let broker = Broker::default();
    for n in 0..14 {
        broker
            .read(&c, Method::GET, &format!("/page/{n}"), &[], None, false)
            .await
            .unwrap();
    }
    let (entries, bytes) = broker.cache_size().await;
    assert!(entries < 14);
    assert!(bytes <= 8 * 1024 * 1024);
}
#[tokio::test]
async fn conditional_not_modified_preserves_cached_body_and_etag() {
    let count = std::sync::atomic::AtomicUsize::new(0);
    let api = api(move |_, _| {
        if count.fetch_add(1, std::sync::atomic::Ordering::SeqCst) > 0 {
            (304, vec![], Value::Null)
        } else {
            (
                200,
                vec![("etag", "\"version\"".into())],
                json!({"ok":true}),
            )
        }
    })
    .await;
    let broker = Broker::default();
    let c = conn(&api, Provider::Github);
    for fresh in [false, true, true] {
        let value = broker
            .read(&c, Method::GET, "/resource", &[], None, fresh)
            .await
            .unwrap();
        assert_eq!(value["ok"], true);
    }
    let seen = api.seen.lock().unwrap();
    for request in &seen[1..] {
        assert_eq!(request.1["if-none-match"], "\"version\"");
    }
}
#[tokio::test]
async fn hosting_cancellation_is_window_scoped_and_handles_early_aborts() {
    let reads = crate::transport::Requests::default();
    let (_guard, mut cancelled) = reads.start(("main".into(), "request".into())).unwrap();
    reads.cancel(("peer".into(), "request".into()));
    assert!(cancelled.try_recv().is_err());
    reads.cancel_owner("main");
    cancelled.await.unwrap();
    assert!(reads.start(("peer".into(), "request".into())).is_err());
}
#[tokio::test]
async fn github_discussion_writes_validate_membership_and_keep_original_positions() {
    let api=api(|_,body|if body["query"].as_str().is_some_and(|q|q.starts_with("query")){(200,vec![],json!({"data":{"repository":{"pullRequest":{"reviewThreads":{"pageInfo":{"hasNextPage":false},"nodes":[{"id":"thread-1","isResolved":true,"viewerCanResolve":false,"viewerCanUnresolve":true,"isOutdated":true,"path":"src/file.ts","line":null,"originalLine":4,"diffSide":"RIGHT","comments":{"pageInfo":{"hasNextPage":false},"nodes":[{"databaseId":"9007199254740993","body":"Old comment","author":{"login":"ada"},"originalCommit":{"oid":OTHER},"commit":{"oid":SHA}}]}}]}}}}}))}else{(200,vec![],json!({"id":12,"data":{"unresolveReviewThread":{"thread":{"id":"thread-1","isResolved":false}}}}))}).await;
    let broker = Broker::default();
    let c = conn(&api, Provider::Github);
    let r = review(Provider::Github);
    let threads = broker.threads(&c, &r, 1, None).await.unwrap();
    assert_eq!(threads["items"][0]["canResolve"], true);
    assert_eq!(threads["items"][0]["originalLine"], 4);
    assert_eq!(threads["items"][0]["comments"][0]["sha"], OTHER);
    let request = write(Action::Reply {
        thread: "thread-1".into(),
        comment: Some("9007199254740993".into()),
        body: "Reply".into(),
    });
    broker.prepare_action(&c, &request).await.unwrap();
    broker.mutate(&c, &request).await.unwrap();
    assert!(api.seen.lock().unwrap().iter().any(|(path, _, body)| path
        .ends_with("/comments/9007199254740993/replies")
        && body["body"].as_str().is_some_and(|b| b.contains(SHA))));
    let invalid = write(Action::Reply {
        thread: "thread-1".into(),
        comment: Some("99".into()),
        body: "Wrong comment".into(),
    });
    assert!(broker.prepare_action(&c, &invalid).await.is_err());
    let request = write(Action::Resolve {
        thread: "thread-1".into(),
        resolved: false,
    });
    broker.prepare_action(&c, &request).await.unwrap();
    broker.mutate(&c, &request).await.unwrap();
    assert!(api
        .seen
        .lock()
        .unwrap()
        .iter()
        .any(|(_, _, body)| body["query"]
            .as_str()
            .is_some_and(|q| q.contains("unresolveReviewThread"))));
}
#[tokio::test]
async fn inaccessible_thread_is_rejected_before_preparing_a_durable_operation() {
    let api = api(|_, _| (200, vec![], json!([]))).await;
    let broker = Broker::default();
    let c = conn(&api, Provider::Gitlab);
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("ops");
    let ledger = Ledger { file: file.clone() };
    let mut request = write(Action::Reply {
        thread: "missing".into(),
        comment: None,
        body: "Reply".into(),
    });
    request.review = review(Provider::Gitlab);
    let mut prepared = false;
    assert!(
        dispatch(&broker, &c, &ledger, &request, "12", &mut prepared)
            .await
            .is_err()
    );
    assert!(!prepared);
    assert!(!file.exists());
    assert_eq!(api.seen.lock().unwrap().len(), 1);
}
#[tokio::test]
async fn completed_merge_receipt_keeps_the_provider_result_revision_separate_from_reviewed_head() {
    let api = api(|_, _| (200, vec![], json!({"merged":true,"sha":OTHER}))).await;
    let broker = Broker::default();
    let dir = tempfile::tempdir().unwrap();
    let ledger = Ledger {
        file: dir.path().join("ops"),
    };
    let receipt = dispatch(
        &broker,
        &conn(&api, Provider::Github),
        &ledger,
        &write(Action::Merge {
            method: "squash".into(),
        }),
        "12",
        &mut false,
    )
    .await
    .unwrap();
    assert_eq!(receipt.status, "succeeded");
    assert_eq!(receipt.reviewed_sha, SHA);
    assert_eq!(receipt.result_sha.as_deref(), Some(OTHER));
}
#[tokio::test]
async fn concurrent_duplicate_submissions_share_the_durable_receipt_without_a_global_write_lock() {
    let api = api(|_, _| (200, vec![], json!({"id":9}))).await;
    let broker = Broker::default();
    let dir = tempfile::tempdir().unwrap();
    let ledger = Ledger {
        file: dir.path().join("ops"),
    };
    let c = conn(&api, Provider::Github);
    let first = write(Action::Comment {
        body: "Same feedback".into(),
    });
    let mut second = first.clone();
    second.operation = "other-window".into();
    let mut a = false;
    let mut b = false;
    let (one, two) = tokio::join!(
        dispatch(&broker, &c, &ledger, &first, "12", &mut a),
        dispatch(&broker, &c, &ledger, &second, "12", &mut b)
    );
    assert_eq!(one.unwrap().operation, two.unwrap().operation);
    assert_eq!(api.seen.lock().unwrap().len(), 1);
}

#[tokio::test]
async fn attention_summaries_are_repository_scoped_bounded_and_do_not_fetch_threads_or_history() {
    let api=api(|path,_|{
        let value=if path.starts_with("/search/issues?"){json!({"items":[{"html_url":"https://provider.test/team/repo/pull/7","title":"Requested"},{"html_url":"https://other.test/team/repo/pull/8","title":"foreign host"},{"html_url":"https://provider.test/other/repo/pull/9","title":"foreign repository"}],"incomplete_results":true})}
        else if path=="/repos/team/repo/pulls/7"{json!({"state":"open","head":{"sha":SHA}})}
        else if path.contains("/check-runs?"){json!({"total_count":21,"check_runs":[{"name":"tests","conclusion":"failure"}]})}
        else if path.contains("/status?"){json!({"total_count":0,"statuses":[]})}
        else {panic!("Unexpected attention endpoint: {path}")};(200,vec![],value)
    }).await;
    let broker = Broker::default();
    let c = conn(&api, Provider::Github);
    let requested = broker
        .attention_requested(&c, "team/repo", None)
        .await
        .unwrap();
    assert_eq!(requested.reviews.len(), 1);
    assert!(!requested.complete);
    let checks = broker
        .attention_checks(&c, &review(Provider::Github))
        .await
        .unwrap();
    assert_eq!(checks.head, SHA);
    assert_eq!(checks.failed, vec!["tests"]);
    assert!(!checks.complete);
    let seen = api.seen.lock().unwrap();
    assert_eq!(seen.len(), 4);
    assert!(seen[0].0.contains("repo%3Ateam%2Frepo"));
    assert!(seen.iter().all(|(p, _, _)| p.starts_with("GET ")));
}
#[tokio::test]
async fn attention_gitlab_only_reports_the_latest_pipeline_for_a_ref() {
    let api=api(|path,_|{let v=if path.ends_with("/merge_requests/7"){json!({"state":"opened","sha":SHA})}else if path.contains("/pipelines?"){json!([{"id":2,"ref":"topic","status":"success"},{"id":1,"ref":"topic","status":"failed"}])}else{panic!("Unexpected endpoint {path}")};(200,vec![],v)}).await;
    let broker = Broker::default();
    let result = broker
        .attention_checks(&conn(&api, Provider::Gitlab), &review(Provider::Gitlab))
        .await
        .unwrap();
    assert!(result.failed.is_empty());
    assert!(result.complete);
}

#[tokio::test]
async fn malformed_attention_responses_are_errors_instead_of_absence_proof() {
    let api = api(|_, _| (200, vec![], json!({"message":"unexpected schema"}))).await;
    let broker = Broker::default();
    assert!(broker
        .attention_requested(&conn(&api, Provider::Github), "team/repo", None)
        .await
        .is_err());
}
