use super::*;
use crate::transport::dispatch;
use std::{path::Path, time::Duration};

fn git(root: &Path, args: &[&str]) -> String {
    let out = std::process::Command::new("git")
        .current_dir(root)
        .args(args)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).unwrap().trim().into()
}
fn fixture() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    git(root, &["init", "-q"]);
    git(root, &["config", "user.name", "Test"]);
    git(root, &["config", "user.email", "test@example.test"]);
    git(root, &["config", "commit.gpgsign", "false"]);
    for i in 1..=3 {
        std::fs::write(root.join("f.py"), format!("print({i})\n")).unwrap();
        git(root, &["add", "."]);
        git(root, &["commit", "-qm", &format!("commit {i}")]);
    }
    dir
}
async fn open(
    engine: &Engine,
    root: &Path,
    request: EnterRequest,
    key: &str,
    owner: &str,
) -> OpenedReview {
    let _opening = engine.opening(key).await;
    if let Some(opened) = engine.duplicate(key, owner).await {
        return opened;
    }
    let slot = engine.reserve().await.unwrap();
    let path = root.join(".git/settings/review.json");
    crate::github::Connections {
        github_enabled: false,
    }
    .save(&path)
    .await
    .unwrap();
    let config = engine.config(path.clone()).await;
    let (runtime, _) = start_with(
        path,
        root.into(),
        request,
        None,
        None,
        None,
        Some(config),
        None,
        None,
    )
    .await
    .unwrap();
    engine
        .insert(owner.into(), key.into(), runtime, slot)
        .await
        .unwrap()
}
fn comparison(root: &Path, old: &str, new: &str) -> EnterRequest {
    EnterRequest::Comparison {
        old: Some(git(root, &["rev-parse", old])),
        new: git(root, &["rev-parse", new]),
        base: diffle::protocol::ComparisonBase::Direct,
    }
}

#[tokio::test]
async fn repositories_and_comparisons_interleave_without_comments_refs_or_owner_cross_talk() {
    let a = fixture();
    let b = fixture();
    let engine = Engine::default();
    let first = open(&engine, a.path(), EnterRequest::Working, "repo-a", "main").await;
    let second = open(&engine, b.path(), EnterRequest::Working, "repo-b", "other").await;
    let (ra, mut ea, pins_a) = {
        let r = engine.runtime(&first.id).await.unwrap();
        (
            r.router.clone(),
            r.hub.subscribe(),
            r.repo.review_refs.clone(),
        )
    };
    let (rb, mut eb, pins_b) = {
        let r = engine.runtime(&second.id).await.unwrap();
        (
            r.router.clone(),
            r.hub.subscribe(),
            r.repo.review_refs.clone(),
        )
    };
    let (one, two) = tokio::join!(
        dispatch(
            ra.clone(),
            "POST",
            "/api/threads",
            Some(serde_json::json!({"path":"f.py","body":"Only A"}).to_string())
        ),
        dispatch(
            rb.clone(),
            "POST",
            "/api/threads",
            Some(serde_json::json!({"path":"f.py","body":"Only B"}).to_string())
        )
    );
    assert_eq!(one.unwrap().status, 201);
    assert_eq!(two.unwrap().status, 201);
    tokio::time::timeout(Duration::from_secs(2), ea.recv())
        .await
        .unwrap()
        .unwrap();
    tokio::time::timeout(Duration::from_secs(2), eb.recv())
        .await
        .unwrap()
        .unwrap();
    let one = dispatch(ra, "GET", "/api/threads", None).await.unwrap();
    let two = dispatch(rb, "GET", "/api/threads", None).await.unwrap();
    assert!(String::from_utf8_lossy(&one.body).contains("Only A"));
    assert!(!String::from_utf8_lossy(&one.body).contains("Only B"));
    assert!(String::from_utf8_lossy(&two.body).contains("Only B"));
    assert_ne!(pins_a, pins_b);
    assert!(engine.owned_runtime(&second.id, "main").await.is_err());
    assert!(engine.close_owned(&second.id, "main").await.is_err());
    engine.close_owned(&first.id, "main").await.unwrap();
    engine.close_owned(&first.id, "main").await.unwrap();
    assert!(engine.runtime(&second.id).await.is_ok());
    engine.close().await;

    let engine = Engine::default();
    let first = open(
        &engine,
        a.path(),
        comparison(a.path(), "HEAD~2", "HEAD~1"),
        "older",
        "main",
    )
    .await;
    let second = open(
        &engine,
        a.path(),
        comparison(a.path(), "HEAD~1", "HEAD"),
        "newer",
        "main",
    )
    .await;
    let (old_a, new_a) = {
        let r = engine.runtime(&first.id).await.unwrap();
        let s = r.session.snapshotter().unwrap().current().await.unwrap();
        (s.old_sha.clone(), s.new_sha.clone())
    };
    let (old_b, new_b, refs_b) = {
        let r = engine.runtime(&second.id).await.unwrap();
        let s = r.session.snapshotter().unwrap().current().await.unwrap();
        (
            s.old_sha.clone(),
            s.new_sha.clone(),
            r.repo.review_refs.clone(),
        )
    };
    assert_ne!(old_a, old_b);
    assert_ne!(new_a, new_b);
    engine.close_owned(&first.id, "main").await.unwrap();
    assert!(!git(a.path(), &["for-each-ref", "--format=%(refname)", &refs_b]).is_empty());
    engine.close().await;
}

#[tokio::test]
async fn repeated_eviction_plateaus_and_reconstruction_is_pinned_untrusted_and_fresh_for_worktrees()
{
    let dir = fixture();
    let engine = Engine::default();
    let config = dir.path().join(".git/settings/review.json");
    let first = open(
        &engine,
        dir.path(),
        comparison(dir.path(), "HEAD~2", "HEAD~1"),
        "pinned",
        "main",
    )
    .await;
    let head = git(dir.path(), &["rev-parse", "HEAD~1"]);
    assert_eq!(
        open(
            &engine,
            dir.path(),
            comparison(dir.path(), "HEAD~2", "HEAD~1"),
            "pinned",
            "main"
        )
        .await
        .id,
        first.id
    );
    let mut suspended_worktree = None;
    for i in 0..12 {
        let opened = open(
            &engine,
            dir.path(),
            EnterRequest::Working,
            &format!("working-{i}"),
            "main",
        )
        .await;
        if i == 0 {
            suspended_worktree = Some(opened.id.clone());
        }
        let mut live = 0;
        for handle in engine.handles().await {
            if handle.live.lock().await.is_some() {
                live += 1;
            }
        }
        assert_eq!(live, 2);
        assert_eq!(engine.slots.available_permits(), 0);
        assert!(engine.runtime(&opened.id).await.unwrap().lsp.is_none());
    }
    assert!(engine.runtime(&first.id).await.is_err());
    git(dir.path(), &["reflog", "expire", "--expire=now", "--all"]);
    git(dir.path(), &["gc", "--prune=now"]);
    let resumed = engine
        .resume(&first.id, "main", config.clone(), None)
        .await
        .unwrap();
    assert_ne!(first.id, resumed.id);
    let runtime = engine.runtime(&resumed.id).await.unwrap();
    assert!(runtime.lsp.is_none());
    let snap = runtime
        .session
        .snapshotter()
        .unwrap()
        .current()
        .await
        .unwrap();
    assert_eq!(snap.new_sha, head);
    drop(runtime);
    std::fs::write(dir.path().join("f.py"), "print('fresh after suspension')\n").unwrap();
    let resumed_worktree = engine
        .resume(&suspended_worktree.unwrap(), "main", config, None)
        .await
        .unwrap();
    let runtime = engine.runtime(&resumed_worktree.id).await.unwrap();
    assert!(runtime.lsp.is_none());
    let response = dispatch(runtime.router.clone(), "GET", "/api/patch?path=f.py", None)
        .await
        .unwrap();
    assert_eq!(
        response.status,
        200,
        "{}",
        String::from_utf8_lossy(&response.body)
    );
    assert!(String::from_utf8_lossy(&response.body).contains("fresh after suspension"));
    drop(runtime);
    engine.close().await;
    assert_eq!(engine.slots.available_permits(), MAX_LIVE_REVIEWS);
    assert!(engine.handles().await.is_empty());
    assert!(git(
        dir.path(),
        &["for-each-ref", "--format=%(refname)", "refs/diffle"]
    )
    .is_empty());
}

#[tokio::test]
async fn close_waits_for_its_owned_mutation_and_does_not_lock_another_review() {
    let a = fixture();
    let b = fixture();
    let engine = Arc::new(Engine::default());
    let first = open(&engine, a.path(), EnterRequest::Working, "a", "main").await;
    let second = open(&engine, b.path(), EnterRequest::Working, "b", "main").await;
    let runtime = engine.runtime(&first.id).await.unwrap();
    let router = runtime.router.clone();
    let (release, wait) = tokio::sync::oneshot::channel();
    let operation = tokio::spawn(async move {
        let _owner = runtime;
        wait.await.unwrap();
        dispatch(
            router,
            "POST",
            "/api/threads",
            Some(serde_json::json!({"path":"f.py","body":"Finish before close"}).to_string()),
        )
        .await
        .unwrap()
    });
    let closing = tokio::spawn({
        let engine = engine.clone();
        let id = first.id.clone();
        async move { engine.close_owned(&id, "main").await.unwrap() }
    });
    drop(
        tokio::time::timeout(Duration::from_secs(2), engine.runtime(&second.id))
            .await
            .unwrap()
            .unwrap(),
    );
    assert!(!closing.is_finished());
    release.send(()).unwrap();
    assert_eq!(operation.await.unwrap().status, 201);
    closing.await.unwrap();
    let store = diffle::comments::CommentStore::open(&a.path().join(".git"), "working")
        .await
        .unwrap();
    assert_eq!(
        store.threads(&Default::default()).await.unwrap()[0].messages[0].body,
        "Finish before close"
    );
    engine.close().await;
}

#[tokio::test]
async fn trusted_pool_admission_never_borrows_trust_and_eviction_releases_it() {
    let dir = fixture();
    let engine = Engine::default();
    let first = open(&engine, dir.path(), EnterRequest::Working, "a", "main").await;
    let second = open(&engine, dir.path(), EnterRequest::Working, "b", "main").await;
    assert!(engine.trust(&first.id, "main").await.unwrap().enabled);
    assert!(engine
        .trust(&second.id, "main")
        .await
        .unwrap_err()
        .contains("budget"));
    let handle = engine.handle(&first.id, Some("main")).await.unwrap();
    engine.evict(&handle).await.unwrap();
    assert!(engine.trust(&second.id, "main").await.unwrap().enabled);
    assert_eq!(engine.lsp_slots.available_permits(), 0);
    engine.close().await;
    assert_eq!(engine.lsp_slots.available_permits(), 1);
}

#[tokio::test]
async fn settings_reach_warm_handles_and_changed_commands_revoke_the_existing_pool() {
    let dir = fixture();
    let engine = Engine::default();
    let first = open(&engine, dir.path(), EnterRequest::Working, "a", "main").await;
    let second = open(&engine, dir.path(), EnterRequest::Working, "b", "main").await;
    let config = {
        let r = engine.runtime(&first.id).await.unwrap();
        r.config.clone()
    };
    assert!(Arc::ptr_eq(
        &config,
        &engine.runtime(&second.id).await.unwrap().config
    ));
    config
        .set(diffle::user_config::UserConfigPatch {
            context_lines: Some(2),
            ..Default::default()
        })
        .await
        .unwrap();
    tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            let one = engine.runtime(&first.id).await.unwrap().session.context();
            let two = engine.runtime(&second.id).await.unwrap().session.context();
            if one == 2 && two == 2 {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    assert!(engine.trust(&first.id, "main").await.unwrap().enabled);
    config
        .set_desktop_commands(
            std::collections::HashMap::from([(diffle::protocol::LanguageId::Python, "".into())]),
            Default::default(),
        )
        .await
        .unwrap();
    engine.settings_changed(false).await;
    assert!(engine.runtime(&first.id).await.unwrap().lsp.is_none());
    assert_eq!(engine.lsp_slots.available_permits(), 1);
    engine.close().await;
}

#[tokio::test]
async fn borrowed_managed_checkout_survives_closing_its_original_review() {
    let dir = fixture();
    let engine = Engine::default();
    let source = GitRepo::open(dir.path()).await.unwrap();
    let (repo, lease) = crate::storage::adopt(&dir.path().join(".git/durable"), "managed", &source)
        .await
        .unwrap();
    let root = repo.root.clone();
    drop(repo);
    let config = dir.path().join(".git/settings/review.json");
    crate::github::Connections {
        github_enabled: false,
    }
    .save(&config)
    .await
    .unwrap();
    let shared = engine.config(config.clone()).await;
    let (runtime, _) = start_with(
        config.clone(),
        root.clone(),
        EnterRequest::Working,
        None,
        None,
        None,
        Some(shared.clone()),
        Some((PathBuf::new(), String::new())),
        Some(lease),
    )
    .await
    .unwrap();
    let first = engine
        .insert(
            "main".into(),
            "source".into(),
            runtime,
            engine.reserve().await.unwrap(),
        )
        .await
        .unwrap();
    let borrowed = engine.checkout_for(&root, "main").await.unwrap();
    let (runtime, _) = start_with(
        config,
        root.clone(),
        comparison(&root, "HEAD~1", "HEAD"),
        None,
        None,
        None,
        Some(shared),
        Some((PathBuf::new(), String::new())),
        Some(borrowed),
    )
    .await
    .unwrap();
    let second = engine
        .insert(
            "main".into(),
            "comparison".into(),
            runtime,
            engine.reserve().await.unwrap(),
        )
        .await
        .unwrap();
    engine.close_owned(&first.id, "main").await.unwrap();
    assert!(root.exists());
    assert_eq!(
        dispatch(
            engine.runtime(&second.id).await.unwrap().router.clone(),
            "GET",
            "/api/snapshot",
            None
        )
        .await
        .unwrap()
        .status,
        200
    );
    engine.close().await;
}

#[tokio::test]
async fn workspace_restart_restores_pins_and_follow_intent_without_trust_or_runtime_ids() {
    let root = fixture();
    let tab = crate::storage::digest("workspace-tab");
    let key = format!("workspace/{tab}/local");
    git(root.path(), &["branch", "base", "HEAD~2"]);
    git(root.path(), &["checkout", "-qb", "feature", "HEAD~2"]);
    std::fs::write(root.path().join("unique.txt"), "workspace-pinned only\n").unwrap();
    git(root.path(), &["add", "unique.txt"]);
    git(root.path(), &["commit", "-qm", "divergent feature"]);
    git(root.path(), &["checkout", "-q", "main"]);
    let first = Engine::default();
    let opened = open(
        &first,
        root.path(),
        EnterRequest::Revspec {
            args: vec!["base..feature".into()],
        },
        &key,
        "main",
    )
    .await;
    first
        .pin_workspace_review(&opened.id, "main")
        .await
        .unwrap();
    {
        let runtime = first.runtime(&opened.id).await.unwrap();
        let mut events = runtime.hub.subscribe();
        let version = runtime
            .session
            .snapshotter()
            .unwrap()
            .current()
            .await
            .unwrap()
            .version;
        assert_eq!(runtime.session.pin().await.unwrap().version, version);
        assert!(
            events.try_recv().is_err(),
            "No-op pin must not create an autosave/event loop"
        );
    }
    let saved = first
        .save_workspace_review(&opened.id, "main", &tab, None)
        .await
        .unwrap();
    let pinned = saved.new.clone();
    first.close().await;
    git(root.path(), &["branch", "-f", "feature", "HEAD"]);
    // Leave only the durable workspace checkpoint's refs retaining the divergent commit.
    for reference in git(
        root.path(),
        &["for-each-ref", "--format=%(refname)", "refs/diffle"],
    )
    .lines()
    {
        if !reference.starts_with(&format!("refs/diffle/iterations/workspaces/{tab}/")) {
            git(root.path(), &["update-ref", "-d", reference]);
        }
    }
    git(root.path(), &["reflog", "expire", "--expire=now", "--all"]);
    git(root.path(), &["gc", "--prune=now"]);
    let config = root.path().join(".git/settings/review.json");
    let next = Engine::default();
    let restored = next
        .restore_workspace_review(&saved, true, "main", config.clone(), None)
        .await
        .unwrap();
    assert_ne!(restored.id, opened.id);
    let runtime = next.runtime(&restored.id).await.unwrap();
    assert!(runtime.lsp.is_none());
    assert!(!runtime.github.enabled.load(Ordering::SeqCst));
    assert_eq!(
        runtime
            .session
            .snapshotter()
            .unwrap()
            .current()
            .await
            .unwrap()
            .new_sha,
        pinned
    );
    drop(runtime);
    next.close().await;
    let follow = Engine::default();
    let reopened = follow
        .restore_workspace_review(&saved, false, "main", config, None)
        .await
        .unwrap();
    let runtime = follow.runtime(&reopened.id).await.unwrap();
    assert_eq!(
        runtime
            .session
            .snapshotter()
            .unwrap()
            .current()
            .await
            .unwrap()
            .new_sha,
        git(root.path(), &["rev-parse", "feature"])
    );
    drop(runtime);
    follow.close().await;
}

#[tokio::test]
async fn pinned_named_commit_can_follow_again_and_new_modes_get_new_comment_keys() {
    let root = fixture();
    let engine = Engine::default();
    let tab = crate::storage::digest("named-request");
    let opened = open(
        &engine,
        root.path(),
        EnterRequest::Revspec {
            args: vec!["HEAD^!".into()],
        },
        "named",
        "main",
    )
    .await;
    let saved = engine
        .save_workspace_review(&opened.id, "main", &tab, None)
        .await
        .unwrap();
    engine.close().await;
    std::fs::write(root.path().join("f.py"), "print(4)\n").unwrap();
    git(root.path(), &["add", "."]);
    git(root.path(), &["commit", "-qm", "commit 4"]);
    let next = Engine::default();
    let restored = next
        .restore_workspace_review(
            &saved,
            false,
            "main",
            root.path().join(".git/settings/review.json"),
            None,
        )
        .await
        .unwrap();
    let runtime = next.runtime(&restored.id).await.unwrap();
    assert_eq!(
        runtime
            .session
            .snapshotter()
            .unwrap()
            .current()
            .await
            .unwrap()
            .new_sha,
        git(root.path(), &["rev-parse", "HEAD"])
    );
    runtime.session.enter(EnterRequest::Working).await.unwrap();
    assert_eq!(
        runtime
            .session
            .snapshotter()
            .unwrap()
            .current()
            .await
            .unwrap()
            .comment_key,
        "working"
    );
    drop(runtime);
    next.close().await;
}

#[tokio::test]
async fn twenty_workspace_activations_keep_two_runtimes_and_reap_cold_native_descriptors() {
    let root = fixture();
    let engine = Engine::default();
    let mut saved = vec![];
    let mut current: Option<OpenedReview> = None;
    for i in 0..20 {
        if let Some(previous) = current.take() {
            engine.suspend_owned(&previous.id, "main").await.unwrap();
        }
        let tab = crate::storage::digest(&format!("tab-{i}"));
        let opened = open(
            &engine,
            root.path(),
            EnterRequest::Working,
            &format!("workspace/{tab}/working"),
            "main",
        )
        .await;
        saved.push(
            engine
                .save_workspace_review(&opened.id, "main", &tab, None)
                .await
                .unwrap(),
        );
        engine.reap_workspace_cold("main").await;
        let handles = engine.handles().await;
        assert!(handles.len() <= 2);
        for handle in handles {
            assert!(handle.live.lock().await.is_some());
        }
        current = Some(opened);
    }
    assert_eq!(saved.len(), 20);
    assert_eq!(engine.slots.available_permits(), 0);
    engine.close().await;
    assert_eq!(engine.slots.available_permits(), 2);
}

#[tokio::test]
async fn duplicate_window_checkpoints_own_their_pins_and_closing_one_owner_keeps_the_other_live() {
    let root = fixture();
    let engine = Engine::default();
    let a = crate::storage::digest("window-a");
    let b = crate::storage::digest("window-b");
    let first = open(
        &engine,
        root.path(),
        comparison(root.path(), "HEAD~2", "HEAD~1"),
        &format!("workspace/{a}/local"),
        "main",
    )
    .await;
    let saved = engine
        .save_workspace_review(&first.id, "main", &a, None)
        .await
        .unwrap();
    let copy = crate::workspaces::duplicate_checkpoint(&saved, &b)
        .await
        .unwrap();
    assert!(copy
        .pins
        .iter()
        .all(|p| p.starts_with(&format!("refs/diffle/iterations/workspaces/{b}/"))));
    for pin in &saved.pins {
        git(root.path(), &["update-ref", "-d", pin]);
    }
    let second = engine
        .restore_workspace_review(
            &copy,
            true,
            "workspace-b",
            root.path().join(".git/settings/review.json"),
            None,
        )
        .await
        .unwrap();
    assert!(engine.owned_runtime(&second.id, "main").await.is_err());
    engine.close_owner("main").await;
    assert!(engine.runtime(&first.id).await.is_err());
    let runtime = engine
        .owned_runtime(&second.id, "workspace-b")
        .await
        .unwrap();
    assert_eq!(
        runtime
            .session
            .snapshotter()
            .unwrap()
            .current()
            .await
            .unwrap()
            .new_sha,
        saved.new
    );
    assert!(runtime.lsp.is_none());
    for pin in &copy.pins {
        assert!(!git(root.path(), &["rev-parse", pin]).is_empty());
    }
    drop(runtime);
    engine.close().await;
}

#[tokio::test]
async fn git_write_refresh_keeps_review_threads_and_marks_removed_quotes_stale() {
    let dir = fixture();
    std::fs::write(
        dir.path().join("f.py"),
        "print('reviewed working change')\n",
    )
    .unwrap();
    let engine = Engine::default();
    let opened = open(
        &engine,
        dir.path(),
        EnterRequest::Working,
        "git-refresh",
        "main",
    )
    .await;
    let (router, session, common) = {
        let runtime = engine.runtime(&opened.id).await.unwrap();
        (
            runtime.router.clone(),
            runtime.session.clone(),
            runtime.repo.common_dir.clone(),
        )
    };
    let added = dispatch(
        router.clone(),
        "POST",
        "/api/threads",
        Some(
            serde_json::json!({"path":"f.py","startLine":1,"body":"Keep this review discussion"})
                .to_string(),
        ),
    )
    .await
    .unwrap();
    assert_eq!(
        added.status,
        201,
        "{}",
        String::from_utf8_lossy(&added.body)
    );
    std::fs::write(
        dir.path().join("f.py"),
        "print(\"rewritten by local operation\")\n",
    )
    .unwrap();
    git(dir.path(), &["add", "f.py"]);
    git(dir.path(), &["commit", "-qm", "complete local write"]);
    engine.refresh_git(&common).await;
    tokio::time::timeout(Duration::from_secs(15), async {
        loop {
            let threads = dispatch(router.clone(), "GET", "/api/threads", None)
                .await
                .unwrap();
            let text = String::from_utf8_lossy(&threads.body);
            if text.contains("\"stale\":true") {
                assert!(text.contains("Keep this review discussion"));
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap();
    assert!(engine.runtime(&opened.id).await.unwrap().lsp.is_none());
    drop(session);
    engine.close().await;
}
