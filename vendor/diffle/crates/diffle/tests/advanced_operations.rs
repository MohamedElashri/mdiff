#![cfg(unix)]
mod common;
use common::git_in;
use diffle::git::operations::{self as ops, Action, RecoveryCommand, advanced};
use std::path::{Path, PathBuf};
fn repo() -> (tempfile::TempDir, PathBuf) {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("main");
    std::fs::create_dir(&root).unwrap();
    git_in(&root, &["init", "-qb", "main"]);
    git_in(&root, &["config", "user.name", "Fixture"]);
    git_in(&root, &["config", "user.email", "fixture@example.invalid"]);
    git_in(&root, &["config", "commit.gpgsign", "false"]);
    git_in(&root, &["config", "core.hooksPath", root.join(".git/hooks").to_str().unwrap()]);
    commit(&root, "file", "base\n", "base");
    (temp, root)
}
fn commit(root: &Path, path: &str, text: &str, message: &str) {
    std::fs::write(root.join(path), text).unwrap();
    git_in(root, &["add", "--", path]);
    git_in(root, &["commit", "-qm", message]);
}
async fn run(root: &Path, action: Action) -> ops::Outcome {
    let plan = ops::prepare(root, action).await.unwrap();
    ops::execute(plan, tokio::sync::watch::channel(false).1).await.unwrap()
}
fn conflict(root: &Path) {
    git_in(root, &["checkout", "-qb", "incoming"]);
    commit(root, "file", "incoming\n", "incoming");
    git_in(root, &["checkout", "main"]);
    commit(root, "file", "current\n", "current");
    let out = std::process::Command::new("git").args(["merge", "incoming"]).current_dir(root).output().unwrap();
    assert!(!out.status.success());
}
#[tokio::test]
async fn external_merge_restart_state_valid_commands_and_metadata_freshness() {
    let (_tmp, root) = repo();
    conflict(&root);
    for _ in 0..2 {
        let state = advanced::recovery(&root).await.unwrap();
        assert_eq!(state.operation.as_deref(), Some("merge"));
        assert_eq!(state.conflicts[0].stages.len(), 3);
        assert_eq!(state.commands, vec![RecoveryCommand::Abort]);
    }
    let action = Action::Recover { operation: "merge".into(), command: RecoveryCommand::Abort };
    let plan = ops::prepare(&root, action.clone()).await.unwrap();
    std::fs::write(root.join(".git/MERGE_MSG"), "external message edit").unwrap();
    assert!(ops::revalidate(&plan).await.is_err());
    assert_eq!(run(&root, action).await.status, "completed");
    assert!(advanced::recovery(&root).await.unwrap().operation.is_none());
    assert_eq!(std::fs::read_to_string(root.join("file")).unwrap(), "current\n");
    assert!(
        ops::prepare(&root, Action::Recover { operation: "merge".into(), command: RecoveryCommand::Abort })
            .await
            .is_err()
    );
}
#[tokio::test]
async fn stash_conflicts_and_bisect_have_no_invented_sequencer_commands() {
    let (_tmp, root) = repo();
    std::fs::write(root.join("file"), "stash\n").unwrap();
    git_in(&root, &["stash", "push"]);
    commit(&root, "file", "other\n", "other");
    let out = std::process::Command::new("git").args(["stash", "apply"]).current_dir(&root).output().unwrap();
    assert!(!out.status.success());
    let state = advanced::recovery(&root).await.unwrap();
    assert!(state.operation.is_none());
    assert!(state.commands.is_empty());
    assert!(!state.conflicts.is_empty());
    std::fs::write(root.join(".git/BISECT_LOG"), "fixture").unwrap();
    let state = advanced::recovery(&root).await.unwrap();
    assert_eq!(state.operation.as_deref(), Some("bisect"));
    assert!(state.commands.is_empty());
}
#[tokio::test]
async fn linked_worktrees_have_independent_operation_metadata() {
    let (tmp, root) = repo();
    let linked = tmp.path().join("linked 'quotes'\n folder");
    git_in(&root, &["worktree", "add", "-qb", "linked", linked.to_str().unwrap()]);
    conflict(&root);
    assert_eq!(advanced::recovery(&root).await.unwrap().operation.as_deref(), Some("merge"));
    assert!(advanced::recovery(&linked).await.unwrap().operation.is_none());
}

#[tokio::test]
async fn canonical_editor_checks_staleness_preserves_other_index_entries_and_continues_merge() {
    let (_tmp, root) = repo();
    conflict(&root);
    let view = advanced::conflict(&root, "file").await.unwrap();
    assert!(view.supported);
    assert_eq!(view.contents, vec!["base\n", "current\n", "incoming\n"]);
    assert!(view.labels[1].contains("Current checkout"));
    let action = Action::ResolveText { path: "file".into(), content: "resolved\n".into(), expected: view.expected };
    let plan = ops::prepare(&root, action).await.unwrap();
    std::fs::write(root.join("file"), "external edit\n").unwrap();
    assert!(ops::execute(plan, tokio::sync::watch::channel(false).1).await.is_err());
    std::fs::write(root.join("other"), "preserved\n").unwrap();
    git_in(&root, &["add", "other"]);
    let view = advanced::conflict(&root, "file").await.unwrap();
    assert!(view.supported);
    assert_eq!(
        run(&root, Action::ResolveText { path: "file".into(), content: "resolved\n".into(), expected: view.expected })
            .await
            .status,
        "completed"
    );
    assert!(git_in(&root, &["ls-files", "--unmerged"]).is_empty());
    assert!(git_in(&root, &["diff", "--cached", "--name-only"]).contains("other"));
    let state = advanced::recovery(&root).await.unwrap();
    assert!(state.commands.contains(&RecoveryCommand::Continue));
    assert_eq!(
        run(&root, Action::Recover { operation: "merge".into(), command: RecoveryCommand::Continue }).await.status,
        "completed"
    );
    assert_eq!(git_in(&root, &["rev-list", "--parents", "-n", "1", "HEAD"]).split_whitespace().count(), 3);
}
#[tokio::test]
async fn simple_merge_pick_revert_and_explicit_mainline_are_validated() {
    let (_tmp, root) = repo();
    git_in(&root, &["checkout", "-qb", "topic"]);
    commit(&root, "new", "topic\n", "topic");
    let target = git_in(&root, &["rev-parse", "HEAD"]);
    git_in(&root, &["checkout", "main"]);
    assert_eq!(run(&root, Action::CherryPick { revision: target.clone(), mainline: None }).await.status, "completed");
    assert_eq!(run(&root, Action::Revert { revision: "HEAD".into(), mainline: None }).await.status, "completed");
    assert!(!root.join("new").exists());
    git_in(&root, &["checkout", "-qb", "another"]);
    commit(&root, "another", "another\n", "another");
    git_in(&root, &["checkout", "main"]);
    commit(&root, "ours", "ours\n", "ours");
    assert_eq!(run(&root, Action::Merge { revision: "another".into() }).await.status, "completed");
    let merge = git_in(&root, &["rev-parse", "HEAD"]);
    assert!(ops::prepare(&root, Action::Revert { revision: merge.clone(), mainline: None }).await.is_err());
    assert_eq!(run(&root, Action::Revert { revision: merge, mainline: Some(1) }).await.status, "completed");
    assert!(!root.join("another").exists());
    assert!(root.join("ours").exists());
}
#[tokio::test]
async fn cherry_pick_conflict_offers_skip_and_abort_and_no_premature_continue() {
    for command in [RecoveryCommand::Skip, RecoveryCommand::Abort] {
        let (_tmp, root) = repo();
        git_in(&root, &["checkout", "-qb", "topic"]);
        commit(&root, "file", "picked\n", "picked");
        let target = git_in(&root, &["rev-parse", "HEAD"]);
        git_in(&root, &["checkout", "main"]);
        commit(&root, "file", "ours\n", "ours");
        let head = git_in(&root, &["rev-parse", "HEAD"]);
        assert_eq!(run(&root, Action::CherryPick { revision: target, mainline: None }).await.status, "needsInspection");
        let state = advanced::recovery(&root).await.unwrap();
        assert_eq!(state.operation.as_deref(), Some("cherry-pick"));
        assert!(state.commands.contains(&command));
        assert!(!state.commands.contains(&RecoveryCommand::Continue));
        assert_eq!(run(&root, Action::Recover { operation: "cherry-pick".into(), command }).await.status, "completed");
        assert_eq!(git_in(&root, &["rev-parse", "HEAD"]), head);
        assert_eq!(std::fs::read_to_string(root.join("file")).unwrap(), "ours\n");
    }
}
#[tokio::test]
async fn rebase_conflict_labels_are_inverted_and_abort_restores_original_topology() {
    let (_tmp, root) = repo();
    git_in(&root, &["checkout", "-qb", "topic"]);
    commit(&root, "file", "topic\n", "topic");
    let old = git_in(&root, &["rev-parse", "HEAD"]);
    git_in(&root, &["checkout", "main"]);
    commit(&root, "file", "destination\n", "destination");
    git_in(&root, &["checkout", "topic"]);
    assert_eq!(run(&root, Action::Rebase { upstream: "main".into() }).await.status, "needsInspection");
    let view = advanced::conflict(&root, "file").await.unwrap();
    assert!(view.supported);
    assert_eq!(view.contents[1], "destination\n");
    assert_eq!(view.contents[2], "topic\n");
    assert!(view.labels[1].contains("Rebased destination"));
    assert!(view.labels[2].contains("Original commit"));
    assert_eq!(
        run(&root, Action::Recover { operation: "rebase".into(), command: RecoveryCommand::Abort }).await.status,
        "completed"
    );
    assert_eq!(git_in(&root, &["rev-parse", "HEAD"]), old);
    assert_eq!(git_in(&root, &["branch", "--show-current"]), "topic");
}
#[tokio::test]
async fn ordinary_rebase_preserves_changes_and_only_updates_the_current_branch() {
    let (_tmp, root) = repo();
    git_in(&root, &["checkout", "-qb", "topic"]);
    commit(&root, "topic", "topic\n", "topic");
    let old = git_in(&root, &["rev-parse", "HEAD"]);
    git_in(&root, &["branch", "pinned"]);
    git_in(&root, &["checkout", "main"]);
    commit(&root, "main", "main\n", "main");
    let main = git_in(&root, &["rev-parse", "HEAD"]);
    git_in(&root, &["checkout", "topic"]);
    assert_eq!(run(&root, Action::Rebase { upstream: "main".into() }).await.status, "completed");
    assert_eq!(git_in(&root, &["rev-parse", "HEAD~"]), main);
    assert_eq!(git_in(&root, &["rev-parse", "pinned"]), old);
    assert!(root.join("main").exists());
    assert!(root.join("topic").exists());
}
#[tokio::test]
async fn interactive_rebase_reorders_drops_fixups_and_squashes_without_shell_commands() {
    use ops::{RebaseCommand as C, RebaseItem};
    let (_tmp, root) = repo();
    let base = git_in(&root, &["rev-parse", "HEAD"]);
    git_in(&root, &["checkout", "-qb", "topic"]);
    let mut ids = vec![];
    for name in ["a", "b", "c", "d"] {
        commit(&root, name, &format!("{name}\n"), name);
        ids.push(git_in(&root, &["rev-parse", "HEAD"]));
    }
    let todo = vec![
        RebaseItem { oid: ids[2].clone(), command: C::Pick },
        RebaseItem { oid: ids[0].clone(), command: C::Fixup },
        RebaseItem { oid: ids[1].clone(), command: C::Drop },
        RebaseItem { oid: ids[3].clone(), command: C::Squash },
    ];
    assert_eq!(run(&root, Action::RebaseInteractive { upstream: base.clone(), todo }).await.status, "completed");
    assert_eq!(git_in(&root, &["rev-list", "--count", &format!("{base}..HEAD")]), "1");
    assert!(root.join("a").exists());
    assert!(!root.join("b").exists());
    assert!(root.join("c").exists());
    assert!(root.join("d").exists());
    assert!(git_in(&root, &["show", "-s", "--format=%B", "HEAD"]).contains('d'));
}
#[tokio::test]
async fn unsupported_binary_missing_sides_and_symlinks_do_not_become_resolved() {
    let (_tmp, root) = repo();
    std::fs::write(root.join("file"), b"base\0").unwrap();
    git_in(&root, &["add", "file"]);
    git_in(&root, &["commit", "-qm", "binary"]);
    git_in(&root, &["checkout", "-qb", "topic"]);
    std::fs::write(root.join("file"), b"incoming\0").unwrap();
    git_in(&root, &["add", "file"]);
    git_in(&root, &["commit", "-qm", "incoming"]);
    git_in(&root, &["checkout", "main"]);
    std::fs::write(root.join("file"), b"ours\0").unwrap();
    git_in(&root, &["add", "file"]);
    git_in(&root, &["commit", "-qm", "ours"]);
    let _ = run(&root, Action::Merge { revision: "topic".into() }).await;
    let view = advanced::conflict(&root, "file").await.unwrap();
    assert!(!view.supported);
    assert!(
        ops::prepare(
            &root,
            Action::ResolveText { path: "file".into(), expected: view.expected, content: "fake resolved\n".into() }
        )
        .await
        .is_err()
    );
    assert!(!git_in(&root, &["ls-files", "-u"]).is_empty());
    run(&root, Action::Recover { operation: "merge".into(), command: RecoveryCommand::Abort }).await;
    git_in(&root, &["rm", "file"]);
    git_in(&root, &["commit", "-qm", "delete"]);
    let _ = run(&root, Action::Merge { revision: "topic".into() }).await;
    assert!(!advanced::conflict(&root, "file").await.unwrap().supported);
}
#[tokio::test]
async fn index_lock_and_marker_checks_preserve_the_working_conflict() {
    let (_tmp, root) = repo();
    conflict(&root);
    let view = advanced::conflict(&root, "file").await.unwrap();
    let original = std::fs::read(root.join("file")).unwrap();
    assert!(
        ops::prepare(
            &root,
            Action::ResolveText { path: "file".into(), content: view.result.clone(), expected: view.expected.clone() }
        )
        .await
        .is_err()
    );
    let plan = ops::prepare(
        &root,
        Action::ResolveText { path: "file".into(), content: "resolved\n".into(), expected: view.expected },
    )
    .await
    .unwrap();
    std::fs::write(root.join(".git/index.lock"), "external").unwrap();
    assert!(ops::execute(plan, tokio::sync::watch::channel(false).1).await.is_err());
    assert_eq!(std::fs::read(root.join("file")).unwrap(), original);
    assert_eq!(std::fs::read_to_string(root.join(".git/index.lock")).unwrap(), "external");
}
#[tokio::test]
async fn result_writer_refuses_parent_symlinks_and_outside_hardlinks() {
    use std::os::unix::fs::{MetadataExt, symlink};
    let (tmp, root) = repo();
    let outside = tmp.path().join("outside");
    std::fs::create_dir(&outside).unwrap();
    std::fs::write(outside.join("keep"), "kept").unwrap();
    symlink(&outside, root.join("link")).unwrap();
    let meta = std::fs::metadata(&root).unwrap();
    let id = (meta.dev(), meta.ino());
    assert!(advanced::write_result(&root, "link/keep", b"changed", id).is_err());
    assert!(advanced::write_result(&root, "../outside/keep", b"changed", id).is_err());
    std::fs::hard_link(outside.join("keep"), root.join("hard")).unwrap();
    assert!(advanced::write_result(&root, "hard", b"changed", id).is_err());
    assert_eq!(std::fs::read_to_string(outside.join("keep")).unwrap(), "kept");
}

#[tokio::test]
async fn resolved_rebase_continues_and_a_second_conflict_can_be_skipped() {
    let (_tmp, root) = repo();
    git_in(&root, &["checkout", "-qb", "topic"]);
    commit(&root, "file", "first\n", "first");
    commit(&root, "file", "second\n", "second");
    git_in(&root, &["checkout", "main"]);
    commit(&root, "file", "destination\n", "destination");
    let dest = git_in(&root, &["rev-parse", "HEAD"]);
    git_in(&root, &["checkout", "topic"]);
    run(&root, Action::Rebase { upstream: "main".into() }).await;
    let view = advanced::conflict(&root, "file").await.unwrap();
    run(
        &root,
        Action::ResolveText { path: "file".into(), content: "resolved first\n".into(), expected: view.expected },
    )
    .await;
    assert_eq!(
        run(&root, Action::Recover { operation: "rebase".into(), command: RecoveryCommand::Continue }).await.status,
        "needsInspection"
    );
    assert_eq!(advanced::recovery(&root).await.unwrap().conflicts.len(), 1);
    assert_eq!(
        run(&root, Action::Recover { operation: "rebase".into(), command: RecoveryCommand::Skip }).await.status,
        "completed"
    );
    assert_eq!(git_in(&root, &["rev-parse", "HEAD~"]), dest);
    assert_eq!(std::fs::read_to_string(root.join("file")).unwrap(), "resolved first\n");
}
#[tokio::test]
async fn external_exec_todo_is_not_executed_by_recovery_and_metadata_changes_stale_the_preview() {
    let (_tmp, root) = repo();
    git_in(&root, &["checkout", "-qb", "topic"]);
    commit(&root, "file", "topic\n", "topic");
    git_in(&root, &["checkout", "main"]);
    commit(&root, "file", "main\n", "main");
    git_in(&root, &["checkout", "topic"]);
    run(&root, Action::Rebase { upstream: "main".into() }).await;
    let plan = ops::prepare(&root, Action::Recover { operation: "rebase".into(), command: RecoveryCommand::Abort })
        .await
        .unwrap();
    std::fs::write(root.join(".git/rebase-merge/git-rebase-todo"), "exec touch SHOULD_NOT_RUN\n").unwrap();
    assert!(ops::revalidate(&plan).await.is_err());
    let state = advanced::recovery(&root).await.unwrap();
    assert_eq!(state.commands, vec![RecoveryCommand::Abort]);
    assert!(!root.join("SHOULD_NOT_RUN").exists());
    assert!(
        ops::prepare(&root, Action::Recover { operation: "rebase".into(), command: RecoveryCommand::Continue })
            .await
            .is_err()
    );
    run(&root, Action::Recover { operation: "rebase".into(), command: RecoveryCommand::Abort }).await;
    assert!(!root.join("SHOULD_NOT_RUN").exists());
}
#[tokio::test]
async fn malformed_interactive_plans_and_merge_topology_are_refused_before_mutation() {
    use ops::{RebaseCommand as C, RebaseItem};
    let (_tmp, root) = repo();
    let base = git_in(&root, &["rev-parse", "HEAD"]);
    commit(&root, "one", "one\n", "one");
    let head = git_in(&root, &["rev-parse", "HEAD"]);
    for todo in [
        vec![RebaseItem { oid: head.clone(), command: C::Fixup }],
        vec![RebaseItem { oid: head.clone(), command: C::Drop }],
        vec![RebaseItem { oid: "0".repeat(40), command: C::Pick }],
    ] {
        assert!(ops::prepare(&root, Action::RebaseInteractive { upstream: base.clone(), todo }).await.is_err());
    }
    assert_eq!(git_in(&root, &["rev-parse", "HEAD"]), head);
    git_in(&root, &["checkout", "-qb", "side", &base]);
    commit(&root, "side", "side\n", "side");
    git_in(&root, &["checkout", "main"]);
    git_in(&root, &["merge", "--no-edit", "side"]);
    assert!(advanced::rebase_commits(&root, &base).await.is_err());
}
#[tokio::test]
async fn failing_hooks_and_prestart_cancellation_keep_real_operation_state() {
    use std::os::unix::fs::PermissionsExt;
    let (_tmp, root) = repo();
    git_in(&root, &["checkout", "-qb", "topic"]);
    commit(&root, "topic", "topic\n", "topic");
    git_in(&root, &["checkout", "main"]);
    commit(&root, "main", "main\n", "main");
    git_in(&root, &["checkout", "topic"]);
    let head = git_in(&root, &["rev-parse", "HEAD"]);
    let hook = root.join(".git/hooks/pre-rebase");
    std::fs::write(&hook, "#!/bin/sh\nprintf 'hook left edits\\n' > hook-edits\nexit 1\n").unwrap();
    std::fs::set_permissions(hook, std::fs::Permissions::from_mode(0o700)).unwrap();
    assert_eq!(run(&root, Action::Rebase { upstream: "main".into() }).await.status, "needsInspection");
    assert_eq!(git_in(&root, &["rev-parse", "HEAD"]), head);
    assert!(root.join("hook-edits").exists());
    let (_tmp, clean) = repo();
    git_in(&clean, &["branch", "other"]);
    let plan = ops::prepare(&clean, Action::Merge { revision: "other".into() }).await.unwrap();
    assert!(ops::execute(plan, tokio::sync::watch::channel(true).1).await.is_err());
    assert!(advanced::recovery(&clean).await.unwrap().operation.is_none());
}
#[tokio::test]
async fn operation_metadata_is_bounded_even_for_external_sequences() {
    let (_tmp, root) = repo();
    std::fs::create_dir(root.join(".git/sequencer")).unwrap();
    std::fs::write(root.join(".git/sequencer/todo"), vec![b'x'; 2 * 1024 * 1024 + 1]).unwrap();
    assert!(ops::inspect(&root, &Action::Commit { message: String::new(), amend: false }).await.is_err());
    assert!(advanced::recovery(&root).await.is_err());
}

#[tokio::test]
async fn multiple_merge_bases_use_the_actual_synthesized_index_base() {
    let (_tmp, root) = repo();
    let base = git_in(&root, &["rev-parse", "HEAD"]);
    commit(&root, "file", "left\n", "left");
    let left = git_in(&root, &["rev-parse", "HEAD"]);
    let left_tree = git_in(&root, &["rev-parse", "HEAD^{tree}"]);
    git_in(&root, &["checkout", "-qb", "right", &base]);
    commit(&root, "file", "right\n", "right");
    let right = git_in(&root, &["rev-parse", "HEAD"]);
    let right_tree = git_in(&root, &["rev-parse", "HEAD^{tree}"]);
    let m1 = git_in(&root, &["commit-tree", &left_tree, "-p", &left, "-p", &right, "-m", "left merge"]);
    let m2 = git_in(&root, &["commit-tree", &right_tree, "-p", &right, "-p", &left, "-m", "right merge"]);
    git_in(&root, &["checkout", "main"]);
    git_in(&root, &["reset", "--hard", &m1]);
    git_in(&root, &["branch", "incoming-criss-cross", &m2]);
    assert_eq!(git_in(&root, &["merge-base", "--all", "HEAD", "incoming-criss-cross"]).lines().count(), 2);
    let result = run(&root, Action::Merge { revision: "incoming-criss-cross".into() }).await;
    assert_eq!(result.status, "needsInspection");
    let view = advanced::conflict(&root, "file").await.unwrap();
    assert!(view.supported);
    let actual = ops::identity(&root).await.unwrap();
    assert_eq!(view.contents[0].trim_end(), git_in(&actual.root, &["show", ":1:file"]));
    assert!(view.labels[0].contains("possibly synthesized"));
}
#[tokio::test]
async fn nonregular_index_modes_have_explicit_external_fallbacks() {
    use std::io::Write;
    for mode in ["120000", "160000"] {
        let (_tmp, root) = repo();
        let blob = git_in(&root, &["rev-parse", "HEAD:file"]);
        let object = if mode == "160000" { git_in(&root, &["rev-parse", "HEAD"]) } else { blob.clone() };
        let mut child = std::process::Command::new("git")
            .args(["update-index", "--index-info"])
            .current_dir(&root)
            .stdin(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        let records = format!(
            "0 {}\tfile\n100644 {blob} 1\tfile\n{mode} {object} 2\tfile\n100644 {blob} 3\tfile\n",
            "0".repeat(40)
        );
        child.stdin.take().unwrap().write_all(records.as_bytes()).unwrap();
        assert!(child.wait().unwrap().success());
        let view = advanced::conflict(&root, "file").await.unwrap();
        assert!(!view.supported);
        assert!(view.reason.contains("submodule"));
        assert!(!git_in(&root, &["ls-files", "-u"]).is_empty());
    }
}
