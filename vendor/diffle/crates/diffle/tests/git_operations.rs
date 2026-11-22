#![cfg(unix)]
mod common;
use common::git_in;
use diffle::git::operations::{self as ops, Action};
use std::path::{Path, PathBuf};
fn repo() -> (tempfile::TempDir, PathBuf) {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("main");
    std::fs::create_dir(&root).unwrap();
    git_in(&root, &["init", "-qb", "main"]);
    git_in(&root, &["config", "user.name", "mdiff fixture"]);
    git_in(&root, &["config", "user.email", "fixture@example.invalid"]);
    git_in(&root, &["config", "commit.gpgsign", "false"]);
    git_in(&root, &["config", "core.hooksPath", root.join(".git/hooks").to_str().unwrap()]);
    std::fs::write(root.join("file"), "base\n").unwrap();
    git_in(&root, &["add", "."]);
    git_in(&root, &["commit", "-qm", "base"]);
    (temp, root)
}
async fn run(root: &Path, action: Action) -> ops::Outcome {
    let plan = ops::prepare(root, action).await.unwrap();
    ops::execute(plan, tokio::sync::watch::channel(false).1).await.unwrap()
}
#[tokio::test]
async fn stale_content_index_refs_and_repository_replacement_are_rejected() {
    let (_temp, root) = repo();
    std::fs::write(root.join("file"), "first\n").unwrap();
    let action = Action::Stage { path: "file".into() };
    let before = ops::prepare(&root, action.clone()).await.unwrap();
    std::fs::write(root.join("file"), "other\n").unwrap();
    assert!(ops::revalidate(&before).await.is_err());
    let before = ops::prepare(&root, action.clone()).await.unwrap();
    git_in(&root, &["add", "file"]);
    assert!(ops::revalidate(&before).await.is_err());
    let before = ops::prepare(&root, action).await.unwrap();
    git_in(&root, &["branch", "external"]);
    assert!(ops::revalidate(&before).await.is_err());
    let before = ops::prepare(&root, Action::TagCreate { name: "v1".into(), start: "HEAD".into() }).await.unwrap();
    let moved = root.with_file_name("original");
    std::fs::rename(&root, &moved).unwrap();
    std::fs::create_dir(&root).unwrap();
    git_in(&root, &["init", "-q"]);
    assert!(ops::revalidate(&before).await.is_err());
}
#[tokio::test]
async fn exact_unusual_paths_and_linked_indexes_remain_independent() {
    let (temp, root) = repo();
    let linked = temp.path().join("linked 'quotes'\n folder");
    git_in(&root, &["worktree", "add", "-qb", "feature", linked.to_str().unwrap()]);
    let name = ":(glob)* 'quotes'\nfile";
    std::fs::write(linked.join(name), b"binary\0\xff").unwrap();
    std::fs::write(linked.join("unrelated"), "kept\n").unwrap();
    let original = std::fs::read(root.join(".git/index")).unwrap();
    assert_eq!(run(&linked, Action::Stage { path: name.into() }).await.status, "completed");
    assert!(git_in(&linked, &["diff", "--cached", "--name-only"]).contains("quotes"));
    assert!(!git_in(&linked, &["diff", "--cached", "--name-only"]).contains("unrelated"));
    assert_eq!(std::fs::read(root.join(".git/index")).unwrap(), original);
    assert_eq!(run(&linked, Action::Unstage { path: name.into() }).await.status, "completed");
    assert!(git_in(&linked, &["diff", "--cached", "--name-only"]).is_empty());
    for path in [".", "../file", ".git/config", "/tmp/file"] {
        assert!(ops::validate_path(&linked, path).await.is_err());
    }
}
#[tokio::test]
async fn index_lock_and_prestart_cancel_never_replace_the_original_index() {
    let (_temp, root) = repo();
    std::fs::write(root.join("file"), "changed\n").unwrap();
    let before = std::fs::read(root.join(".git/index")).unwrap();
    let plan = ops::prepare(&root, Action::Stage { path: "file".into() }).await.unwrap();
    let lock = root.join(".git/index.lock");
    std::fs::write(&lock, "external lock").unwrap();
    assert!(ops::execute(plan, tokio::sync::watch::channel(false).1).await.is_err());
    assert_eq!(std::fs::read_to_string(&lock).unwrap(), "external lock");
    std::fs::remove_file(lock).unwrap();
    let plan = ops::prepare(&root, Action::Stage { path: "file".into() }).await.unwrap();
    assert!(ops::execute(plan, tokio::sync::watch::channel(true).1).await.is_err());
    assert_eq!(std::fs::read(root.join(".git/index")).unwrap(), before);
}
#[tokio::test]
async fn canonical_hunks_preserve_crlf_and_reject_stale_or_binary_selections() {
    let (_temp, root) = repo();
    let original = (0..40).map(|n| format!("line {n}\r\n")).collect::<String>();
    std::fs::write(root.join("lines"), &original).unwrap();
    git_in(&root, &["add", "lines"]);
    git_in(&root, &["commit", "-qm", "lines"]);
    let changed = original.replace("line 2\r\n", "first change\r\n").replace("line 35\r\n", "second change\r\n");
    std::fs::write(root.join("lines"), changed).unwrap();
    let patch = ops::patch(&root, "lines", false).await.unwrap();
    assert_eq!(patch.hunks.len(), 2);
    assert_eq!(run(&root, Action::Hunk { path: "lines".into(), staged: false, hunk: 0 }).await.status, "completed");
    let staged = git_in(&root, &["show", ":lines"]);
    assert!(staged.contains("first change\r"));
    assert!(!staged.contains("second change"));
    assert_eq!(run(&root, Action::Hunk { path: "lines".into(), staged: true, hunk: 0 }).await.status, "completed");
    assert!(git_in(&root, &["diff", "--cached"]).is_empty());
    std::fs::write(root.join("file"), b"binary\0").unwrap();
    assert!(
        !ops::patch(&root, "file", false).await.unwrap().partial
            || ops::patch(&root, "file", false).await.unwrap().hunks.is_empty()
    );
}
#[tokio::test]
async fn hooks_and_signing_failures_preserve_real_state_without_claiming_rollback() {
    use std::os::unix::fs::PermissionsExt;
    let (_temp, root) = repo();
    std::fs::write(root.join("file"), "staged\n").unwrap();
    git_in(&root, &["add", "file"]);
    let head = git_in(&root, &["rev-parse", "HEAD"]);
    let hook = root.join(".git/hooks/pre-commit");
    std::fs::write(&hook, "#!/bin/sh\nprintf 'hook edit\\n' > hook-file\nexit 1\n").unwrap();
    std::fs::set_permissions(&hook, std::fs::Permissions::from_mode(0o755)).unwrap();
    let result = run(&root, Action::Commit { message: "draft remains with caller".into(), amend: false }).await;
    assert_eq!(result.status, "needsInspection");
    assert_eq!(git_in(&root, &["rev-parse", "HEAD"]), head);
    assert!(root.join("hook-file").exists());
    std::fs::remove_file(hook).unwrap();
    let signer = root.join("failed-signer");
    std::fs::write(&signer, "#!/bin/sh\nexit 1\n").unwrap();
    std::fs::set_permissions(&signer, std::fs::Permissions::from_mode(0o755)).unwrap();
    git_in(&root, &["config", "commit.gpgsign", "true"]);
    git_in(&root, &["config", "gpg.program", signer.to_str().unwrap()]);
    git_in(&root, &["config", "gpg.format", "openpgp"]);
    assert_eq!(
        run(&root, Action::Commit { message: "cannot sign".into(), amend: false }).await.status,
        "needsInspection"
    );
    assert_eq!(git_in(&root, &["rev-parse", "HEAD"]), head);
}
#[tokio::test]
async fn conditional_refs_clean_switch_restore_and_exact_discard_work() {
    let (_temp, root) = repo();
    assert_eq!(
        run(&root, Action::BranchCreate { name: "feature".into(), start: "HEAD".into() }).await.status,
        "completed"
    );
    assert_eq!(run(&root, Action::TagCreate { name: "v1".into(), start: "HEAD".into() }).await.status, "completed");
    assert_eq!(run(&root, Action::BranchSwitch { name: "feature".into() }).await.status, "completed");
    assert!(ops::prepare(&root, Action::BranchDelete { name: "main".into() }).await.is_ok());
    assert!(ops::prepare(&root, Action::BranchDelete { name: "feature".into() }).await.is_err());
    std::fs::write(root.join("file"), "discard tracked edit\n").unwrap();
    assert_eq!(run(&root, Action::Restore { path: "file".into() }).await.status, "completed");
    assert_eq!(std::fs::read_to_string(root.join("file")).unwrap(), "base\n");
    std::fs::write(root.join("untracked"), "remove only this file").unwrap();
    assert_eq!(run(&root, Action::Discard { path: "untracked".into() }).await.status, "completed");
    assert!(!root.join("untracked").exists());
    assert_eq!(run(&root, Action::BranchDelete { name: "main".into() }).await.status, "completed");
    assert_eq!(run(&root, Action::TagDelete { name: "v1".into() }).await.status, "completed");
}
#[tokio::test]
async fn worktree_lifecycle_refuses_dirty_ignored_and_locked_targets() {
    let (temp, root) = repo();
    git_in(&root, &["branch", "feature"]);
    let linked = temp.path().join("new worktree");
    let path = linked.to_str().unwrap().to_string();
    assert_eq!(
        run(&root, Action::WorktreeCreate { path: path.clone(), branch: "feature".into() }).await.status,
        "completed"
    );
    assert_eq!(
        run(&root, Action::WorktreeLock { path: path.clone(), reason: "keep".into() }).await.status,
        "completed"
    );
    assert!(ops::prepare(&root, Action::WorktreeRemove { path: path.clone() }).await.is_err());
    assert_eq!(run(&root, Action::WorktreeUnlock { path: path.clone() }).await.status, "completed");
    std::fs::write(linked.join("ignored"), "kept").unwrap();
    std::fs::write(root.join(".git/info/exclude"), "ignored\n").unwrap();
    assert!(ops::prepare(&root, Action::WorktreeRemove { path: path.clone() }).await.is_err());
    std::fs::remove_file(linked.join("ignored")).unwrap();
    assert_eq!(run(&root, Action::WorktreeRemove { path }).await.status, "completed");
    assert!(!linked.exists());
    assert_eq!(run(&root, Action::WorktreePrune).await.status, "completed");
}
#[tokio::test]
async fn stash_pop_conflict_retains_the_original_stash_and_exposes_recovery() {
    let (_temp, root) = repo();
    std::fs::write(root.join("file"), "stash version\n").unwrap();
    assert_eq!(run(&root, Action::StashSave { message: "save".into(), untracked: false }).await.status, "completed");
    let oid = git_in(&root, &["rev-parse", "refs/stash"]).trim().to_owned();
    std::fs::write(root.join("file"), "new committed version\n").unwrap();
    git_in(&root, &["add", "file"]);
    git_in(&root, &["commit", "-qm", "external"]);
    let outcome = run(&root, Action::StashPop { oid: oid.clone() }).await;
    assert_eq!(outcome.status, "needsInspection");
    assert_eq!(git_in(&root, &["rev-parse", "refs/stash"]).trim(), oid);
    assert!(outcome.after.unwrap().conflicts);
}
#[tokio::test]
async fn local_remote_push_uses_recorded_destinations_and_exact_leases() {
    let (temp, root) = repo();
    let remote = temp.path().join("remote.git");
    git_in(temp.path(), &["init", "--bare", "-q", remote.to_str().unwrap()]);
    assert_eq!(
        run(&root, Action::RemoteAdd { name: "origin".into(), url: remote.to_str().unwrap().into() }).await.status,
        "completed"
    );
    assert_eq!(
        run(&root, Action::Push { remote: "origin".into(), branch: "main".into(), force: false }).await.status,
        "completed"
    );
    assert_eq!(git_in(&remote, &["rev-parse", "refs/heads/main"]), git_in(&root, &["rev-parse", "HEAD"]));
    let preview = ops::prepare(&root, Action::Push { remote: "origin".into(), branch: "main".into(), force: true })
        .await
        .unwrap();
    git_in(&remote, &["update-ref", "-d", "refs/heads/main"]);
    assert!(ops::revalidate(&preview).await.is_err());
    assert_eq!(run(&root, Action::Fetch { remote: "origin".into() }).await.status, "completed");
    for url in
        ["https://user:token@example.invalid/repo", "ext::sh -c evil", "https://example.invalid/repo?token=secret"]
    {
        assert!(ops::validate_url(url).is_err());
    }
}
#[tokio::test]
async fn cancellation_reaps_git_hook_group_and_reports_an_inspectable_outcome() {
    use std::os::unix::fs::PermissionsExt;
    let (_temp, root) = repo();
    std::fs::write(root.join("file"), "staged\n").unwrap();
    git_in(&root, &["add", "file"]);
    let hook = root.join(".git/hooks/pre-commit");
    std::fs::write(
        &hook,
        "#!/bin/sh\ntrap '' TERM\nprintf started > hook-started\nsleep 1\nprintf unsafe > hook-finished\nsleep 60\n",
    )
    .unwrap();
    std::fs::set_permissions(hook, std::fs::Permissions::from_mode(0o755)).unwrap();
    let plan = ops::prepare(&root, Action::Commit { message: "cancelled commit".into(), amend: false }).await.unwrap();
    let (cancel, receiver) = tokio::sync::watch::channel(false);
    let job = tokio::spawn(ops::execute(plan, receiver));
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        while !root.join("hook-started").exists() {
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap();
    cancel.send_replace(true);
    let result = tokio::time::timeout(std::time::Duration::from_secs(5), job).await.unwrap().unwrap().unwrap();
    assert_eq!(result.status, "needsInspection");
    assert!(!root.join("hook-finished").exists());
}
#[tokio::test]
async fn losing_the_controller_does_not_cancel_an_owned_write_or_release_its_drain_guard() {
    let (_temp, root) = repo();
    std::fs::write(root.join("file"), "new\n").unwrap();
    let plan = ops::prepare(&root, Action::Stage { path: "file".into() }).await.unwrap();
    let gate = std::sync::Arc::new(tokio::sync::RwLock::new(()));
    let held = gate.clone().read_owned().await;
    let (controller, receiver) = tokio::sync::watch::channel(false);
    drop(controller);
    let job = tokio::spawn(async move {
        let _held = held;
        ops::execute(plan, receiver).await
    });
    let drain = gate.write();
    tokio::pin!(drain);
    let result = job.await.unwrap().unwrap();
    assert_eq!(result.status, "completed");
    assert!(tokio::time::timeout(std::time::Duration::from_secs(1), drain).await.is_ok());
    assert!(git_in(&root, &["diff", "--cached"]).contains("+new"));
}
#[tokio::test]
async fn fast_forward_pull_uses_the_explicit_remote_branch_and_refuses_divergence() {
    let (temp, root) = repo();
    let remote = temp.path().join("remote.git");
    git_in(temp.path(), &["clone", "--bare", "-q", root.to_str().unwrap(), remote.to_str().unwrap()]);
    run(&root, Action::RemoteAdd { name: "origin".into(), url: remote.to_str().unwrap().into() }).await;
    let other = temp.path().join("other");
    git_in(temp.path(), &["clone", "-q", remote.to_str().unwrap(), other.to_str().unwrap()]);
    std::fs::write(other.join("remote-added"), "remote\n").unwrap();
    git_in(&other, &["add", "."]);
    git_in(&other, &["commit", "-qm", "remote update"]);
    git_in(&other, &["push", "-q", "origin", "HEAD:refs/heads/main"]);
    let target = git_in(&other, &["rev-parse", "HEAD"]);
    assert_eq!(run(&root, Action::Pull { remote: "origin".into(), branch: "main".into() }).await.status, "completed");
    assert_eq!(git_in(&root, &["rev-parse", "HEAD"]), target);
    assert!(root.join("remote-added").exists());
    std::fs::write(root.join("local"), "local\n").unwrap();
    git_in(&root, &["add", "."]);
    git_in(&root, &["commit", "-qm", "local"]);
    std::fs::write(other.join("another"), "remote\n").unwrap();
    git_in(&other, &["add", "."]);
    git_in(&other, &["commit", "-qm", "diverged"]);
    git_in(&other, &["push", "-q", "origin", "HEAD:refs/heads/main"]);
    let before = git_in(&root, &["rev-parse", "HEAD"]);
    assert_eq!(
        run(&root, Action::Pull { remote: "origin".into(), branch: "main".into() }).await.status,
        "needsInspection"
    );
    assert_eq!(git_in(&root, &["rev-parse", "HEAD"]), before);
    assert!(!root.join("another").exists());
}
#[tokio::test]
async fn whole_file_staging_covers_modes_symlinks_binary_and_rename_as_delete_add() {
    use std::os::unix::fs::PermissionsExt;
    let (_temp, root) = repo();
    std::fs::set_permissions(root.join("file"), std::fs::Permissions::from_mode(0o755)).unwrap();
    assert_eq!(run(&root, Action::Stage { path: "file".into() }).await.status, "completed");
    assert!(git_in(&root, &["ls-files", "--stage", "file"]).starts_with("100755"));
    std::os::unix::fs::symlink("file", root.join("link")).unwrap();
    assert_eq!(run(&root, Action::Stage { path: "link".into() }).await.status, "completed");
    assert!(git_in(&root, &["ls-files", "--stage", "link"]).starts_with("120000"));
    std::fs::write(root.join("binary"), b"a\0b\xff").unwrap();
    assert_eq!(run(&root, Action::Stage { path: "binary".into() }).await.status, "completed");
    git_in(&root, &["commit", "-qm", "special files"]);
    std::fs::rename(root.join("file"), root.join("renamed")).unwrap();
    assert_eq!(run(&root, Action::Stage { path: "file".into() }).await.status, "completed");
    assert_eq!(run(&root, Action::Stage { path: "renamed".into() }).await.status, "completed");
    let diff = git_in(&root, &["diff", "--cached", "--name-status"]);
    assert!(diff.contains("file") && diff.contains("renamed"));
}
#[tokio::test]
async fn commit_amend_and_stash_success_paths_preserve_expected_content_and_refs() {
    let (_temp, root) = repo();
    std::fs::write(root.join("file"), "commit content\n").unwrap();
    run(&root, Action::Stage { path: "file".into() }).await;
    let old = git_in(&root, &["rev-parse", "HEAD"]);
    assert_eq!(run(&root, Action::Commit { message: "first message".into(), amend: false }).await.status, "completed");
    let first = git_in(&root, &["rev-parse", "HEAD"]);
    assert_ne!(first, old);
    assert_eq!(run(&root, Action::Commit { message: "amended message".into(), amend: true }).await.status, "completed");
    assert!(git_in(&root, &["log", "-1", "--format=%s"]).contains("amended message"));
    assert_eq!(git_in(&root, &["rev-parse", "HEAD^@"]), old);
    std::fs::write(root.join("file"), "stashed\n").unwrap();
    std::fs::write(root.join("untracked"), "kept\n").unwrap();
    assert_eq!(run(&root, Action::StashSave { message: "both".into(), untracked: true }).await.status, "completed");
    assert!(!root.join("untracked").exists());
    let oid = git_in(&root, &["rev-parse", "refs/stash"]).trim().to_owned();
    assert_eq!(run(&root, Action::StashPop { oid }).await.status, "completed");
    assert_eq!(std::fs::read_to_string(root.join("file")).unwrap(), "stashed\n");
    assert!(root.join("untracked").exists());
    assert!(git_in(&root, &["stash", "list"]).is_empty());
}
#[tokio::test]
async fn browsing_and_previews_do_not_run_configured_filters_but_confirmed_file_staging_does() {
    use std::os::unix::fs::PermissionsExt;
    let (_temp, root) = repo();
    let filter = root.join("filter-driver");
    std::fs::write(&filter, "#!/bin/sh\nprintf ran > filter-ran\ncat\n").unwrap();
    std::fs::set_permissions(&filter, std::fs::Permissions::from_mode(0o755)).unwrap();
    std::fs::write(root.join(".gitattributes"), "file filter=fixture\n").unwrap();
    git_in(&root, &["config", "filter.fixture.clean", filter.to_str().unwrap()]);
    git_in(&root, &["config", "filter.fixture.required", "true"]);
    std::fs::write(root.join("file"), "changed\n").unwrap();
    let catalog = ops::catalog(&root).await.unwrap();
    assert!(!catalog["files"].as_array().unwrap().is_empty());
    let patch = ops::patch(&root, "file", false).await.unwrap();
    assert!(!patch.partial);
    assert!(!root.join("filter-ran").exists());
    diffle::git::worktrees::status(&root).await.unwrap();
    assert!(!root.join("filter-ran").exists());
    let plan = ops::prepare(&root, Action::Stage { path: "file".into() }).await.unwrap();
    assert!(!root.join("filter-ran").exists());
    assert_eq!(ops::execute(plan, tokio::sync::watch::channel(false).1).await.unwrap().status, "completed");
    assert!(root.join("filter-ran").exists());
}
#[tokio::test]
async fn cancelling_a_push_after_receive_updates_refs_preserves_unknown_outcome_and_reconciles() {
    use std::os::unix::fs::PermissionsExt;
    let (temp, root) = repo();
    let remote = temp.path().join("remote.git");
    git_in(temp.path(), &["clone", "--bare", "-q", root.to_str().unwrap(), remote.to_str().unwrap()]);
    run(&root, Action::RemoteAdd { name: "origin".into(), url: remote.to_str().unwrap().into() }).await;
    std::fs::write(root.join("file"), "published to disposable remote\n").unwrap();
    git_in(&root, &["add", "file"]);
    git_in(&root, &["commit", "-qm", "new head"]);
    let intended = git_in(&root, &["rev-parse", "HEAD"]).trim().to_owned();
    let hook = remote.join("hooks/post-receive");
    std::fs::write(&hook, "#!/bin/sh\nprintf started > postreceive-started\nsleep 60\n").unwrap();
    std::fs::set_permissions(hook, std::fs::Permissions::from_mode(0o755)).unwrap();
    let plan = ops::prepare(&root, Action::Push { remote: "origin".into(), branch: "main".into(), force: false })
        .await
        .unwrap();
    let proof = plan.remote.clone().unwrap();
    let (cancel, receiver) = tokio::sync::watch::channel(false);
    let job = tokio::spawn(ops::execute(plan, receiver));
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        while !remote.join("postreceive-started").exists() {
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap();
    assert_eq!(git_in(&remote, &["rev-parse", "refs/heads/main"]).trim(), intended);
    cancel.send_replace(true);
    let outcome = tokio::time::timeout(std::time::Duration::from_secs(5), job).await.unwrap().unwrap().unwrap();
    assert_eq!(outcome.status, "needsInspection");
    assert_eq!(ops::reconcile_remote(&root, &proof).await.unwrap().as_deref(), Some(intended.as_str()));
}
#[tokio::test]
async fn filter_normalized_clean_files_do_not_report_an_invented_saved_stash() {
    use std::os::unix::fs::PermissionsExt;
    let (_temp, root) = repo();
    let driver = root.join(".git/hooks/clean-driver");
    std::fs::write(&driver, "#!/bin/sh\ncat >/dev/null\nprintf 'base\\n'\n").unwrap();
    std::fs::set_permissions(&driver, std::fs::Permissions::from_mode(0o755)).unwrap();
    std::fs::write(root.join(".git/info/attributes"), "file filter=fixture\n").unwrap();
    git_in(&root, &["config", "filter.fixture.clean", driver.to_str().unwrap()]);
    std::fs::write(root.join("file"), "smudged content\n").unwrap();
    git_in(&root, &["add", "file"]);
    tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    std::fs::write(root.join("file"), "changed content\n").unwrap();
    let outcome = run(&root, Action::StashSave { message: "nothing eligible".into(), untracked: false }).await;
    assert_eq!(outcome.status, "noChanges");
    assert!(git_in(&root, &["stash", "list"]).is_empty());
}
#[test]
fn existing_ssh_scp_and_local_file_remotes_are_supported_without_accepting_helpers() {
    for url in [
        "git@github.com:team/repo.git",
        "work-git:team/repo.git",
        "ssh://git@host.test:2222/team/repo.git",
        "ssh://git@[::1]/repo",
        "file:///tmp/local%20repo.git",
    ] {
        assert!(ops::validate_url(url).is_ok(), "{url}");
    }
    for url in [
        "ext::sh -c evil",
        "helper::command",
        "-oProxyCommand=bad:repo",
        "ssh://-bad/repo",
        "ssh://-bad@host/repo",
        "https://token@host/repo",
        "file://remote-host/private",
    ] {
        assert!(ops::validate_url(url).is_err(), "{url}");
    }
}
