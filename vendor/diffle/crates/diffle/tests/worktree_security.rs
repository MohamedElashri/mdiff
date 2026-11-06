#![cfg(unix)]
mod common;

use common::{TestRepo, session};
use diffle::protocol::{EnterRequest, FollowRefs, Side};
use std::os::unix::fs::symlink;
use std::sync::Arc;

#[tokio::test]
async fn snapshot_reads_reject_parent_symlinks_after_directory_replacement() {
    let t = TestRepo::new();
    t.write("tracked/secret.txt", "public\n");
    t.commit("base");
    let outside = tempfile::tempdir().unwrap();
    std::fs::write(outside.path().join("secret.txt"), "private\n").unwrap();
    let repo = Arc::new(t.open().await);
    let (session, _) = session(&repo, false, 3, FollowRefs::Auto);
    let review = session.resolve(EnterRequest::Working).await.unwrap();
    session.start(review).await.unwrap();
    let snap = session.snapshotter().unwrap().current().await.unwrap();
    assert!(snap.tree.contains(&"tracked/secret.txt".into()));
    std::fs::rename(t.path.join("tracked"), t.path.join("saved")).unwrap();
    symlink(outside.path(), t.path.join("tracked")).unwrap();
    assert!(session.read_side(&snap, "tracked/secret.txt", Side::New).await.unwrap().is_none());
    assert!(repo.head("worktree", "tracked/secret.txt", 100).await.unwrap().is_none());
    // The symlink's own target is still reviewable as Git stores it.
    assert_eq!(repo.read_worktree("tracked").await.unwrap().unwrap(), outside.path().as_os_str().as_encoded_bytes());
    session.close().await;
}

#[tokio::test]
async fn concurrent_directory_swaps_never_return_outside_contents() {
    let t = TestRepo::new();
    t.write("tracked/value", "public");
    let outside = tempfile::tempdir().unwrap();
    std::fs::write(outside.path().join("value"), "private").unwrap();
    let repo = t.open().await;
    let root = t.path.clone();
    let target = outside.path().to_owned();
    let swapping = std::thread::spawn(move || {
        for _ in 0..500 {
            std::fs::rename(root.join("tracked"), root.join("saved")).unwrap();
            symlink(&target, root.join("tracked")).unwrap();
            std::thread::yield_now();
            std::fs::remove_file(root.join("tracked")).unwrap();
            std::fs::rename(root.join("saved"), root.join("tracked")).unwrap();
        }
    });
    for _ in 0..500 {
        if let Some(bytes) = repo.read_worktree("tracked/value").await.unwrap() {
            assert_eq!(bytes, b"public");
        }
        if let Some(bytes) = repo.head("worktree", "tracked/value", 7).await.unwrap() {
            assert_eq!(bytes, b"public");
        }
    }
    swapping.join().unwrap();
}

#[tokio::test]
async fn rejects_traversal_and_keeps_leaf_symlinks_as_links() {
    let t = TestRepo::new();
    t.write("file", "public contents");
    symlink("/etc/passwd", t.path.join("link")).unwrap();
    let repo = t.open().await;
    assert_eq!(repo.read_worktree("link").await.unwrap().unwrap(), b"/etc/passwd");
    assert!(repo.head("worktree", "link", 100).await.unwrap().is_none());
    assert!(repo.read_worktree("../outside").await.unwrap().is_none());
    assert_eq!(repo.head("worktree", "file", 6).await.unwrap().unwrap(), b"public");
}
