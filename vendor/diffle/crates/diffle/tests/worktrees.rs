mod common;
use common::git_in;
use diffle::git::{GitRepo, worktrees};
use std::path::Path;
fn repo(path: &Path) {
    std::fs::create_dir_all(path).unwrap();
    git_in(path, &["init", "-q", "-b", "main"]);
    std::fs::write(path.join("file"), "base\n").unwrap();
    git_in(path, &["add", "."]);
    git_in(path, &["commit", "-qm", "base"]);
}
#[tokio::test]
async fn linked_detached_locked_and_prunable_paths_round_trip_without_mutating_registrations() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("main repository");
    repo(&root);
    let linked = dir.path().join("linked 'quotes' ; $() — \n folder");
    let detached = dir.path().join("detached");
    git_in(&root, &["worktree", "add", "-q", "-b", "feature", linked.to_str().unwrap()]);
    git_in(&root, &["worktree", "add", "-q", "--detach", detached.to_str().unwrap()]);
    git_in(&root, &["worktree", "lock", "--reason", "keep\nexternal editor", linked.to_str().unwrap()]);
    std::fs::remove_dir_all(&detached).unwrap();
    let before = git_in(&root, &["show-ref"]);
    let rows = worktrees::list(&root).await.unwrap();
    assert_eq!(rows.len(), 3);
    let row = rows.iter().find(|row| row.path == std::fs::canonicalize(&linked).unwrap()).unwrap();
    assert_eq!(row.branch.as_deref(), Some("feature"));
    assert_eq!(row.locked.as_deref(), Some("keep\nexternal editor"));
    let unavailable = rows.iter().find(|row| row.path.file_name() == Some(std::ffi::OsStr::new("detached"))).unwrap();
    assert!(unavailable.detached);
    assert!(unavailable.prunable.is_some());
    assert_eq!(git_in(&root, &["show-ref"]), before);
    assert_eq!(git_in(&root, &["symbolic-ref", "HEAD"]), "refs/heads/main");
    let linked_repo = GitRepo::open(&linked).await.unwrap();
    let main_repo = GitRepo::open(&root).await.unwrap();
    assert_eq!(linked_repo.common_dir, main_repo.common_dir);
    assert_ne!(linked_repo.git_dir, main_repo.git_dir);
    let history = diffle::history::Repository::open(&linked).await.unwrap();
    assert_eq!(history.info().await.unwrap().root, std::fs::canonicalize(&linked).unwrap().to_str().unwrap());
    history.close().await;
}
#[tokio::test]
async fn selected_status_uses_each_index_local_upstream_and_preserves_index_bytes() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("main");
    repo(&root);
    git_in(&root, &["branch", "upstream"]);
    git_in(&root, &["branch", "--set-upstream-to=upstream", "main"]);
    std::fs::write(root.join("new-commit"), "new\n").unwrap();
    git_in(&root, &["add", "."]);
    git_in(&root, &["commit", "-qm", "ahead"]);
    let linked = dir.path().join("linked");
    git_in(&root, &["worktree", "add", "-q", "-b", "other", linked.to_str().unwrap()]);
    std::fs::write(root.join("file"), "staged\n").unwrap();
    git_in(&root, &["add", "file"]);
    std::fs::write(root.join("file"), "unstaged\n").unwrap();
    std::fs::create_dir(root.join("untracked")).unwrap();
    std::fs::write(root.join("untracked/a"), "a").unwrap();
    std::fs::write(root.join("untracked/b"), "b").unwrap();
    let index = root.join(".git/index");
    let bytes = std::fs::read(&index).unwrap();
    let modified = std::fs::metadata(&index).unwrap().modified().unwrap();
    let value = worktrees::status(&root).await.unwrap();
    assert_eq!((value.staged, value.unstaged, value.untracked, value.conflicts), (1, 1, 1, 0));
    assert_eq!((value.ahead, value.behind), (Some(1), Some(0)));
    let other = worktrees::status(&linked).await.unwrap();
    assert_eq!((other.staged, other.unstaged, other.untracked), (0, 0, 0));
    assert_eq!(other.branch.as_deref(), Some("other"));
    assert_eq!(std::fs::read(&index).unwrap(), bytes);
    assert_eq!(std::fs::metadata(index).unwrap().modified().unwrap(), modified);
}
#[tokio::test]
async fn repository_paths_handle_newlines_carriage_returns_and_subdirectories() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("root\nwith\rcontrol 'chars' —");
    repo(&root);
    let sub = root.join("nested");
    std::fs::create_dir(&sub).unwrap();
    let opened = GitRepo::open(&sub).await.unwrap();
    let expected = std::fs::canonicalize(&root).unwrap();
    assert_eq!(opened.root, expected);
    assert_eq!(opened.common_dir, expected.join(".git"));
    let info = diffle::history::Repository::open(&sub).await.unwrap().info().await.unwrap();
    assert_eq!(info.root, expected.to_str().unwrap());
    let value = worktrees::status(&root).await.unwrap();
    assert_eq!(value.branch.as_deref(), Some("main"));
}
#[tokio::test]
async fn bare_main_inventory_and_unborn_checkout_have_explicit_states() {
    let dir = tempfile::tempdir().unwrap();
    let bare = dir.path().join("objects.git");
    std::fs::create_dir(&bare).unwrap();
    git_in(&bare, &["init", "--bare", "-q"]);
    let rows = worktrees::list(&bare).await.unwrap();
    assert_eq!(rows.len(), 1);
    assert!(rows[0].bare);
    assert!(rows[0].head.is_none());
    let unborn = dir.path().join("unborn");
    std::fs::create_dir(&unborn).unwrap();
    git_in(&unborn, &["init", "-q", "-b", "new"]);
    let row = worktrees::list(&unborn).await.unwrap().remove(0);
    assert_eq!(row.branch.as_deref(), Some("new"));
    assert!(row.head.is_none());
    let status = worktrees::status(&unborn).await.unwrap();
    assert!(status.head.is_none());
    assert_eq!(status.branch.as_deref(), Some("new"));
}
#[test]
fn nul_status_parsing_skips_rename_source_paths_and_counts_conflicts_without_treating_names_as_records() {
    let value = worktrees::parse_status(
        b"# branch.head main\0# branch.ab +12 -3\x002 R. a b c d e f renamed\0? source\0u UU a b c\0? untracked\0",
    )
    .unwrap();
    assert_eq!((value.staged, value.unstaged, value.conflicts, value.untracked), (1, 0, 1, 1));
    assert_eq!((value.ahead, value.behind), (Some(12), Some(3)));
    assert!(worktrees::parse(b"worktree /tmp/missing-terminator\0").is_err());
    assert!(worktrees::parse_status(b"2 R. x\0").is_err());
}
#[test]
fn inventory_has_a_record_bound_and_preserves_unquoted_path_bytes() {
    let mut bytes = vec![];
    for i in 0..257 {
        bytes.extend_from_slice(format!("worktree /tmp/{i}\0bare\0\0").as_bytes());
    }
    assert!(worktrees::parse(&bytes).is_err());
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt;
        let rows = worktrees::parse(b"worktree /tmp/non-utf8-\xff\0bare\0\0").unwrap();
        assert_eq!(rows[0].path.as_os_str().as_bytes(), b"/tmp/non-utf8-\xff");
    }
}
