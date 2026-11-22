mod common;
use common::{TestRepo, snapshot};
use diffle::{
    history::{Query, Repository},
    protocol::{ComparisonBase, EnterRequest},
};
use std::{collections::HashSet, sync::Arc};
fn query(root: &str) -> Query {
    Query {
        root: root.into(),
        generation: String::new(),
        search: String::new(),
        author: String::new(),
        since: None,
        until: None,
    }
}

#[tokio::test]
async fn host_history_admission_refuses_excess_walks_and_releases_completed_walks() {
    let t = TestRepo::new();
    t.write("f", "a");
    t.commit("first");
    let limit = Arc::new(tokio::sync::Semaphore::new(1));
    let occupied = limit.clone().acquire_owned().await.unwrap();
    let repo = Repository::open(&t.path).await.unwrap();
    repo.set_admission(limit.clone());
    assert!(repo.begin(query("HEAD")).await.unwrap_err().to_string().contains("budget"));
    drop(occupied);
    assert_eq!(repo.begin(query("HEAD")).await.unwrap().rows.len(), 1);
    assert_eq!(limit.available_permits(), 1);
    repo.close().await;
}

#[tokio::test]
async fn pages_are_pinned_replayable_complete_and_exclude_owned_refs() {
    let t = TestRepo::new();
    t.write("file", "root\n");
    t.commit("root");
    for i in 0..215 {
        t.git(&["commit", "--allow-empty", "-qm", &format!("commit {i}")]);
    }
    let expected: Vec<_> = t.git(&["log", "--topo-order", "--format=%H", "HEAD"]).lines().map(str::to_owned).collect();
    let tree = t.git(&["rev-parse", "HEAD^{tree}"]);
    let orphan = t.git(&["commit-tree", tree.trim(), "-m", "owned-only"]);
    t.git(&["update-ref", "refs/diffle/test", orphan.trim()]);
    let repo = Repository::open(&t.path).await.unwrap();
    let first = repo.begin(query("all")).await.unwrap();
    let _: diffle::protocol::HistoryPage = serde_json::from_value(serde_json::to_value(&first).unwrap()).unwrap();
    let _: diffle::protocol::HistoryInfo =
        serde_json::from_value(serde_json::to_value(repo.info().await.unwrap()).unwrap()).unwrap();
    assert_eq!(first.rows.len(), 100);
    t.git(&["commit", "--allow-empty", "-qm", "after pin"]);
    assert_eq!(
        serde_json::to_value(repo.page(&first.generation, 0).await.unwrap()).unwrap(),
        serde_json::to_value(&first).unwrap()
    );
    let mut all = first.rows.clone();
    let mut page = first;
    while let Some(next) = page.next {
        page = repo.page(&page.generation, next).await.unwrap();
        all.extend(page.rows.clone());
    }
    assert_eq!(all.iter().map(|r| r.sha.clone()).collect::<Vec<_>>(), expected);
    assert_eq!(all.iter().map(|r| &r.sha).collect::<HashSet<_>>().len(), 216);
    assert!(!all.iter().any(|r| r.sha == orphan.trim()));
    assert!(all.iter().all(|r| r.lane.is_some_and(|lane| lane < 32)));
    assert!(all[100].incoming);
    repo.close().await;
    assert!(repo.page(&page.generation, 0).await.is_err());
}
#[tokio::test]
async fn generation_close_is_scoped_filters_have_no_false_edges_and_unborn_is_empty() {
    let t = TestRepo::new();
    let repo = Repository::open(&t.path).await.unwrap();
    let empty = repo.begin(query("HEAD")).await.unwrap();
    assert!(empty.rows.is_empty());
    assert!(empty.next.is_none());
    t.write("a", "a");
    t.commit("first");
    t.git(&["commit", "--allow-empty", "-qm", "needle"]);
    let mut q = query("HEAD");
    q.search = "needle".into();
    let current = repo.begin(q).await.unwrap();
    repo.close_generation(&empty.generation).await;
    assert_eq!(current.rows.len(), 1);
    assert!(current.rows[0].edges.is_empty());
    assert_eq!(repo.page(&current.generation, 0).await.unwrap().rows.len(), 1);
    assert!(repo.page(&empty.generation, 0).await.is_err());
    repo.close().await;
}
#[tokio::test]
async fn details_trees_and_root_comparisons_remain_lazy_and_reviewable() {
    let t = TestRepo::new();
    t.write("folder/file.txt", "one\n");
    t.commit("root");
    let sha = t.git(&["rev-parse", "HEAD"]).trim().to_owned();
    let context = Repository::open(&t.path).await.unwrap();
    let detail = context.detail(&sha).await.unwrap();
    let _: diffle::protocol::HistoryDetail = serde_json::from_value(serde_json::to_value(&detail).unwrap()).unwrap();
    assert!(detail.parents.is_empty());
    assert_eq!(detail.message, "root");
    let tree = context.tree(&detail.tree).await.unwrap();
    let _: diffle::protocol::HistoryTree = serde_json::from_value(serde_json::to_value(&tree).unwrap()).unwrap();
    assert_eq!(tree.entries[0].kind, "tree");
    assert_eq!(context.tree(&tree.entries[0].oid).await.unwrap().entries[0].name, "file.txt");
    let repo = Arc::new(t.open().await);
    let (_, root) =
        snapshot(&repo, EnterRequest::Comparison { old: None, new: sha, base: ComparisonBase::Direct }).await;
    assert_eq!(root.changed.len(), 1);
    assert_eq!(root.commits.total, 1);
    assert!(!root.old_sha.is_empty());
}
#[tokio::test]
async fn shallow_details_retain_real_parent_ids_instead_of_claiming_a_root() {
    let t = TestRepo::new();
    t.write("a", "a");
    t.commit("root");
    t.git(&["commit", "--allow-empty", "-qm", "tip"]);
    let clone = tempfile::tempdir().unwrap();
    let status = std::process::Command::new("git")
        .args(["clone", "--quiet", "--depth=1", &format!("file://{}", t.path.display())])
        .arg(clone.path())
        .status()
        .unwrap();
    assert!(status.success());
    let repo = Repository::open(clone.path()).await.unwrap();
    let info = repo.info().await.unwrap();
    assert!(info.shallow);
    assert_eq!(repo.detail(info.head.as_deref().unwrap()).await.unwrap().parents.len(), 1);
    let page = repo.begin(query("HEAD")).await.unwrap();
    assert!(page.rows[0].boundary);
    repo.close().await;
}
