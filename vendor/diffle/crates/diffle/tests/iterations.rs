mod common;
use common::{TestRepo, git_in};
use diffle::{git::GitRepo, iterations::IterationStore};
use std::sync::Arc;

#[tokio::test]
async fn linked_worktrees_keep_independent_iteration_pins_for_the_same_comment_key() {
    let main = TestRepo::new();
    main.write("f", "first");
    main.commit("first");
    let first = main.git(&["rev-parse", "HEAD"]);
    main.write("f", "second");
    main.commit("second");
    let second = main.git(&["rev-parse", "HEAD"]);
    let dir = tempfile::tempdir().unwrap();
    let linked = dir.path().join("linked");
    main.git(&["worktree", "add", "--detach", linked.to_str().unwrap(), "HEAD"]);
    let a = IterationStore::new(Arc::new(main.open().await), "working".into());
    let b = IterationStore::new(Arc::new(GitRepo::open(&linked).await.unwrap()), "working".into());
    a.record(&first, &second).await.unwrap();
    b.record(&second, &first).await.unwrap();
    let before = main.git(&["for-each-ref", "--format=%(refname) %(objectname)", "refs/diffle/iterations/"]);
    assert_eq!(before.lines().count(), 4);
    a.clear().await.unwrap();
    assert_eq!(b.list().await.unwrap()[0].new_sha, first);
    let remaining = git_in(&linked, &["for-each-ref", "--format=%(objectname)", "refs/diffle/iterations/"]);
    assert_eq!(remaining.lines().count(), 2);
    assert!(remaining.lines().any(|sha| sha == first));
    assert!(remaining.lines().any(|sha| sha == second));
}
