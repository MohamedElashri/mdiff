mod common;
use common::{TestRepo, revspec, snapshot};
use diffle::protocol::EnterRequest;
use std::collections::HashSet;
use std::sync::Arc;

#[tokio::test]
async fn immutable_batches_seed_reusable_file_cache_including_unusual_names() {
    let t = TestRepo::new();
    let names = ["a", "b", "c", "with spaces.txt", "tab\tname.txt", "line\nname.txt"];
    for name in names {
        t.write(name, "before\n");
    }
    t.commit("base");
    for name in names {
        t.write(name, "after\n");
    }
    t.commit("edit");
    let repo = Arc::new(t.open().await);
    let (s, _) = snapshot(&repo, revspec(&["HEAD~1..HEAD"])).await;
    let paths: Vec<String> = names.iter().map(|s| (*s).into()).collect();
    let batch = s.patch_many(&paths).await.unwrap();
    let hits = repo.patch_cache_stats().1;
    for path in &paths {
        assert!(batch.contains(&s.patch(path).await.unwrap().unwrap()));
    }
    assert!(repo.patch_cache_stats().1 >= hits + paths.len());
    s.invalidate(100);
    assert!(s.patch("with spaces.txt").await.unwrap().unwrap().contains("+after"));
    assert!(repo.patch_cache_stats().1 > hits + paths.len());
    let (other, _) = snapshot(&repo, revspec(&["HEAD~1..HEAD"])).await;
    let hits = repo.patch_cache_stats().1;
    other.patch("line\nname.txt").await.unwrap();
    assert!(repo.patch_cache_stats().1 > hits);
    assert!(repo.patch_cache_stats().0 <= 32 * 1024 * 1024);
}

#[tokio::test]
async fn selective_refresh_matches_full_recomputation_and_falls_back_for_membership_changes() {
    let t = TestRepo::new();
    t.write("a.txt", "a\n");
    t.write("b.txt", "b\n");
    t.commit("base");
    t.write("a.txt", "a\nfirst\n");
    t.write("b.txt", "b\nuntouched\n");
    let repo = Arc::new(t.open().await);
    let (s, _) = snapshot(&repo, EnterRequest::Working).await;
    let untouched = s.patch("b.txt").await.unwrap();
    t.write("a.txt", "a\nsecond\n");
    let dirty = HashSet::from(["a.txt".to_owned()]);
    let incremental = s.refresh_paths(&dirty, 200).await.unwrap().unwrap();
    let (_, full) = snapshot(&repo, EnterRequest::Working).await;
    assert_eq!(serde_json::to_value(&incremental.changed).unwrap(), serde_json::to_value(&full.changed).unwrap());
    assert_eq!(incremental.tree, full.tree);
    assert_eq!(s.patch("b.txt").await.unwrap(), untouched);
    assert!(s.patch("a.txt").await.unwrap().unwrap().contains("+second"));
    t.write("a.txt", "a\n");
    let restored = s.refresh_paths(&dirty, 201).await.unwrap().unwrap();
    assert!(!restored.changed.iter().any(|f| f.path == "a.txt"));
    t.write("untracked.txt", "new\n");
    assert!(s.refresh_paths(&HashSet::from(["untracked.txt".into()]), 202).await.unwrap().is_none());
    t.rm("a.txt");
    assert!(s.refresh_paths(&dirty, 203).await.unwrap().is_none());
}

#[tokio::test]
async fn pages_preserve_utf8_reject_mixed_versions_and_have_bounded_payloads() {
    let t = TestRepo::new();
    t.write("large.txt", "base\n");
    t.commit("base");
    t.write("large.txt", "line é 🦀\n".repeat(12000));
    let repo = Arc::new(t.open().await);
    let (s, snap) = snapshot(&repo, EnterRequest::Working).await;
    let expected = s.patch("large.txt").await.unwrap().unwrap();
    let mut offset = 0;
    let mut digest = None;
    let mut combined = String::new();
    loop {
        let page = s.patch_page("large.txt", snap.version, offset, digest.as_deref()).await.unwrap();
        let chunk = page["patch"].as_str().unwrap();
        assert!(chunk.len() <= 64 * 1024);
        combined.push_str(chunk);
        offset = page["next"].as_u64().unwrap() as usize;
        digest = Some(page["digest"].as_str().unwrap().to_owned());
        if page["done"] == true {
            break;
        }
    }
    assert_eq!(combined, expected);
    assert!(s.patch_page("large.txt", snap.version, 1, Some("wrong")).await.is_err());
    s.invalidate(snap.version + 1);
    assert!(s.patch_page("large.txt", snap.version, offset, digest.as_deref()).await.is_err());
    assert!(s.patch_page("../secret", snap.version + 1, 0, None).await.is_err());
}

#[tokio::test]
async fn invalidation_cancels_obsolete_reads_without_waiting_for_the_job() {
    let t = TestRepo::new();
    t.write("a", "a\n");
    t.commit("base");
    let repo = Arc::new(t.open().await);
    let (s, snap) = snapshot(&repo, EnterRequest::Working).await;
    let reader = s.clone();
    let task =
        tokio::spawn(
            async move { reader.read(snap.version, std::future::pending::<diffle::error::Result<()>>()).await },
        );
    tokio::task::yield_now().await;
    s.invalidate(999);
    assert!(tokio::time::timeout(std::time::Duration::from_secs(1), task).await.unwrap().unwrap().is_err());
}

#[tokio::test]
async fn exceptionally_large_single_file_fails_with_an_actionable_preview_limit() {
    let t = TestRepo::new();
    t.write("a", "base\n");
    t.commit("base");
    t.write("huge.txt", "x".repeat(17 * 1024 * 1024));
    let repo = Arc::new(t.open().await);
    let (s, _) = snapshot(&repo, EnterRequest::Working).await;
    let result = tokio::time::timeout(std::time::Duration::from_secs(10), s.patch("huge.txt")).await.unwrap();
    assert!(result.unwrap_err().to_string().contains("preview limit"));
}

#[tokio::test]
async fn one_file_save_hashes_one_file_instead_of_the_entire_review() {
    let t = TestRepo::new();
    for i in 0..500 {
        t.write(&format!("f{i:04}.txt"), "base\n");
    }
    t.commit("base");
    for i in 0..500 {
        t.write(&format!("f{i:04}.txt"), "base\nchanged\n");
    }
    let repo = Arc::new(t.open().await);
    let (s, _) = snapshot(&repo, EnterRequest::Working).await;
    t.write("f0250.txt", "base\nchanged again\n");
    let before = repo.worktree_hashes();
    let start = std::time::Instant::now();
    s.refresh_paths(&HashSet::from(["f0250.txt".into()]), 999).await.unwrap().unwrap();
    let incremental = start.elapsed();
    assert_eq!(repo.worktree_hashes() - before, 1);
    let before = repo.worktree_hashes();
    let start = std::time::Instant::now();
    snapshot(&repo, EnterRequest::Working).await;
    let full = start.elapsed();
    assert_eq!(repo.worktree_hashes() - before, 500);
    eprintln!("500-file review: selective refresh {incremental:?}, full refresh {full:?}; hashed entries 1 versus 500");
}

#[tokio::test]
async fn selective_refresh_treats_magic_looking_names_as_literal_paths() {
    let t = TestRepo::new();
    t.write("[ab].txt", "base\n");
    t.write("a.txt", "base\n");
    t.commit("base");
    t.write("[ab].txt", "changed\n");
    t.write("a.txt", "other\n");
    let repo = Arc::new(t.open().await);
    let (s, _) = snapshot(&repo, EnterRequest::Working).await;
    t.write("[ab].txt", "latest\n");
    let snap = s.refresh_paths(&HashSet::from(["[ab].txt".into()]), 100).await.unwrap().unwrap();
    assert_eq!(snap.changed.len(), 2);
    let patch = s.patch("[ab].txt").await.unwrap().unwrap();
    assert!(patch.contains("+latest"));
    assert!(!patch.contains("+other"));
}
