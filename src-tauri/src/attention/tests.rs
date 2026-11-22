use super::*;
fn item(key: &str, time: u64) -> Item {
    Item::observed(
        "repo",
        "conflicts",
        key,
        "Conflicts".into(),
        "test checkout".into(),
        Target {
            kind: "conflicts".into(),
            tab: Some("a".repeat(64)),
            ..Default::default()
        },
        time,
    )
}
#[test]
fn observations_deduplicate_preserve_read_dismiss_and_only_reopen_after_proven_resolution() {
    let mut doc = Document::default();
    doc.preferences.enabled = true;
    assert_eq!(
        doc.reconcile("repo", vec![item("same", 1)], true, 1).len(),
        1
    );
    let id = doc.items[0].id.clone();
    doc.items[0].read = true;
    doc.items[0].dismissed = true;
    assert!(doc
        .reconcile("repo", vec![item("same", 2)], true, 2)
        .is_empty());
    assert_eq!(doc.items.len(), 1);
    assert_eq!(doc.items[0].id, id);
    assert!(doc.items[0].dismissed && doc.items[0].read);
    doc.reconcile("repo", vec![], false, 3);
    assert!(doc.items[0].stale);
    assert!(doc.items[0].resolved.is_none());
    doc.reconcile("another-repo", vec![], true, 4);
    assert!(doc.items[0].resolved.is_none());
    doc.reconcile("repo", vec![], true, 5);
    assert_eq!(doc.items[0].resolved, Some(5));
    assert_eq!(
        doc.reconcile("repo", vec![item("same", 6)], true, 6),
        vec![id]
    );
    assert!(!doc.items[0].dismissed && !doc.items[0].read);
    assert_eq!(doc.unread(), 1);
}
#[test]
fn bounded_history_prunes_finished_items_before_active_requests() {
    let mut doc = Document::default();
    doc.reconcile(
        "old",
        (0..200)
            .map(|i| {
                let mut x = item(&i.to_string(), 1);
                x.scope = "old".into();
                x
            })
            .collect(),
        true,
        1,
    );
    doc.reconcile("old", vec![], true, 2);
    doc.reconcile("repo", vec![item("active", 3)], true, 3);
    assert_eq!(doc.items.len(), 200);
    assert!(doc
        .items
        .iter()
        .any(|i| i.resolved.is_none() && i.id == item("active", 3).id));
}
#[tokio::test]
async fn restart_marks_unconfirmed_and_never_restores_live_agent_authority_or_overwrites_corruption(
) {
    let tmp = tempfile::tempdir().unwrap();
    let file = tmp.path().join("attention.json");
    let mut doc = Document::default();
    let mut runtime = item("runtime", 1);
    runtime.source = "agents".into();
    runtime.target.owner = Some("old-window".into());
    runtime.target.terminal = Some("old-pty".into());
    doc.items = vec![item("durable", 1), runtime];
    model::save(&file, &doc).await.unwrap();
    let restored = model::load(&file).await.unwrap();
    assert_eq!(restored.items.len(), 1);
    assert!(restored.items[0].stale);
    assert!(restored.items[0].target.owner.is_none());
    tokio::fs::write(&file, b"BROKEN ORIGINAL").await.unwrap();
    assert!(model::load(&file).await.is_err());
    assert_eq!(tokio::fs::read(&file).await.unwrap(), b"BROKEN ORIGINAL");
}
#[test]
fn thirty_two_repositories_have_one_due_job_per_tick_fairness_and_idle_backoff() {
    let jobs: Vec<_> = (0..32).map(|i| (format!("repo-{i}"), false)).collect();
    let mut schedule = Schedule::default();
    let mut seen = HashSet::new();
    for now in (0..480).step_by(15) {
        assert!(seen.insert(schedule.next(&jobs, now).unwrap()));
    }
    assert_eq!(seen.len(), 32);
    assert!(schedule.next(&jobs, 480).is_none());
    assert_eq!(schedule.next(&jobs, 600), Some("repo-0".into()));
    assert!(schedule.next(&[], 1000).is_none());
    assert!(!Preferences::default().source("checks"));
}
#[test]
fn preferences_reject_unbounded_targets_unsafe_hosts_and_implicit_hidden_lifetime() {
    let mut p = Preferences {
        keep_in_tray: true,
        ..Default::default()
    };
    assert!(p.validate().is_err());
    p.keep_in_tray = false;
    p.repositories.push(Remote {
        site: crate::hosting::types::Site {
            provider: crate::hosting::types::Provider::Github,
            host: "user:secret@provider.test".into(),
        },
        repository: "team/repo".into(),
    });
    assert!(p.validate().is_err());
    p.repositories.clear();
    p.local_tabs = vec!["a".repeat(64); 33];
    assert!(p.validate().is_err());
}

#[test]
fn source_event_bursts_cannot_amplify_git_or_network_polling() {
    let mut budget = CycleBudget::default();
    assert!(budget.admit(0));
    for t in 0..15 {
        assert!(!budget.admit(t));
    }
    assert!(budget.admit(15));
    assert!(!budget.admit(16));
    assert!(budget.admit(30));
}
#[test]
fn removing_a_repository_pauses_its_old_items_without_claiming_resolution() {
    let mut doc = Document::default();
    doc.preferences.enabled = true;
    let event = item("repo", 1);
    doc.items.push(event);
    doc.pause_unconfigured(&HashMap::new());
    assert!(doc.items[0].paused);
    assert!(doc.items[0].resolved.is_none());
    assert_eq!(doc.unread(), 0);
    doc.preferences.local_tabs.push("a".repeat(64));
    doc.pause_unconfigured(&HashMap::from([("a".repeat(64), String::new())]));
    assert!(!doc.items[0].paused);
    assert_eq!(doc.unread(), 1);
}

#[test]
fn unknown_or_missing_agent_reports_do_not_resolve_waiting_requests() {
    let mut a = crate::agents::Activity {
        id: "session".into(),
        tool: "codex".into(),
        session: Some("s".into()),
        parent: None,
        root: "/repo".into(),
        context: "review".into(),
        state: "waiting".into(),
        provenance: "hook".into(),
        started: 1,
        seen: 1,
        reported: Some(1),
        sequence: 1,
        pid: 42,
        start: "start".into(),
        branch: None,
        commit: None,
        terminal: Some("owned".into()),
    };
    let (waiting, _) = sources::agent_batch(vec![(a.clone(), "main".into())], &[]);
    assert_eq!(waiting.items.len(), 1);
    let previous = waiting.items;
    a.state = "unknown".into();
    let (unknown, _) = sources::agent_batch(vec![(a.clone(), "main".into())], &previous);
    assert!(unknown.items[0].stale);
    assert!(unknown.items[0].resolved.is_none());
    let (missing, _) = sources::agent_batch(vec![], &previous);
    assert!(missing.items[0].stale);
    a.state = "working".into();
    let (working, running) = sources::agent_batch(vec![(a, "main".into())], &previous);
    assert!(working.items.is_empty());
    assert!(working.complete);
    assert_eq!(running.len(), 1);
}

#[tokio::test]
async fn display_eviction_does_not_realert_or_forget_decisions_for_active_observations() {
    let mut doc = Document::default();
    let observations: Vec<_> = (0..320).map(|n| item(&n.to_string(), 1)).collect();
    assert_eq!(
        doc.reconcile("repo", observations.clone(), true, 1).len(),
        320
    );
    assert_eq!(doc.items.len(), 200);
    assert!(doc.limited());
    doc.mark_all_read();
    assert!(doc
        .reconcile("repo", observations.clone(), true, 2)
        .is_empty());
    assert!(doc.items.iter().all(|i| i.read));
    let temp = tempfile::tempdir().unwrap();
    let file = temp.path().join("attention.json");
    model::save(&file, &doc).await.unwrap();
    let mut restored = model::load(&file).await.unwrap();
    assert!(restored
        .reconcile("repo", observations.clone(), true, 3)
        .is_empty());
    assert!(restored.items.iter().all(|i| i.read));
    restored.reconcile("repo", vec![], true, 4);
    assert_eq!(restored.reconcile("repo", observations, true, 5).len(), 320);
    assert!(restored.items.iter().all(|i| !i.read));
}
#[tokio::test]
async fn long_path_metadata_stays_inside_the_persistence_and_identity_budgets() {
    let mut doc = Document::default();
    let found = (0..500)
        .map(|n| {
            let mut i = item(&n.to_string(), 1);
            i.target.root = Some(format!("/{}", "a".repeat(4095)));
            i
        })
        .collect();
    doc.reconcile("repo", found, true, 1);
    let temp = tempfile::tempdir().unwrap();
    let file = temp.path().join("bounded.json");
    model::save(&file, &doc).await.unwrap();
    assert!(std::fs::metadata(file).unwrap().len() <= 512 * 1024);
    assert!(doc.items.len() < 200);
    assert!(doc.marks.len() <= 768);
}

#[test]
fn unchanged_running_agent_metadata_does_not_produce_timestamp_only_deltas() {
    let mut a = crate::agents::Activity {
        id: "stable".into(),
        tool: "codex".into(),
        session: None,
        parent: None,
        root: "/repo".into(),
        context: "review".into(),
        state: "working".into(),
        provenance: "hook".into(),
        started: 1,
        seen: 1,
        reported: Some(1),
        sequence: 1,
        pid: 42,
        start: "start".into(),
        branch: None,
        commit: None,
        terminal: None,
    };
    let (_, first) = sources::agent_batch(vec![(a.clone(), "main".into())], &[]);
    a.seen = 50;
    a.reported = Some(50);
    let (_, second) = sources::agent_batch(vec![(a, "main".into())], &[]);
    assert_eq!(first, second);
}

#[tokio::test]
async fn large_legacy_full_records_gain_markers_before_display_trimming() {
    let doc = Document {
        items: (0..90)
            .map(|n| {
                let mut i = item(&n.to_string(), 1);
                i.read = true;
                i.target.root = Some(format!("/{}", "a".repeat(4095)));
                i
            })
            .collect(),
        ..Default::default()
    };
    let temp = tempfile::tempdir().unwrap();
    let file = temp.path().join("legacy.json");
    let text = serde_json::to_string(&doc).unwrap();
    assert!(text.len() < 512 * 1024);
    tokio::fs::write(&file, text).await.unwrap();
    let restored = model::load(&file).await.unwrap();
    assert!(restored.items.len() < 90);
    assert_eq!(restored.marks.len(), 90);
    assert!(restored.items.iter().all(|i| i.read));
    model::save(&file, &restored).await.unwrap();
}

#[test]
fn expedited_current_repositories_are_prioritized_without_starving_inactive_repositories() {
    let mut jobs = vec![("current-a".into(), true), ("current-b".into(), true)];
    jobs.extend((0..30).map(|i| (format!("background-{i}"), false)));
    let mut queue = Schedule::default();
    let mut seen = HashSet::new();
    let mut current = [0, 0];
    for tick in 0..60 {
        queue.expedite(["current-a".into(), "current-b".into()].into_iter());
        let key = queue.next(&jobs, tick * 15).unwrap();
        if key == "current-a" {
            current[0] += 1;
        } else if key == "current-b" {
            current[1] += 1;
        } else {
            seen.insert(key);
        }
    }
    assert_eq!(seen.len(), 30);
    assert_eq!(current, [15, 15]);
}
