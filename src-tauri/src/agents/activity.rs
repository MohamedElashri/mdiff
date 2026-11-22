use super::*;

pub(super) fn identity(pid: i32, start: &str, root: &std::path::Path) -> String {
    crate::storage::digest(&format!("{pid}:{start}:{}", root.display()))
}

impl Data {
    pub(super) fn reconcile(
        &mut self,
        watches: &[Watch],
        found: &[process::Process],
        current: &HashMap<String, Option<process::Process>>,
        tools: &[(String, PathBuf)],
        at: u64,
    ) {
        self.rows.retain_mut(|a| {
            if !watches.iter().any(|w| w.root == a.root) {
                return false;
            }
            let valid = current
                .get(&a.id)
                .and_then(|p| p.as_ref())
                .is_some_and(|p| p.start == a.start && p.cwd.starts_with(&a.root));
            if !valid {
                // Process-only observations have no useful history. Keep brief hook/launch
                // history separately; never let a vanished PID consume a current row.
                if a.provenance == "detected" {
                    return false;
                }
                a.state = "unavailable".into();
                a.commit = None;
                return at.saturating_sub(a.seen) < 60;
            }
            if a.state == "ended" {
                return at.saturating_sub(a.seen) < 60;
            }
            a.seen = at;
            if a.state == "unavailable" {
                a.state = "unknown".into();
            }
            if a.provenance == "hook" && at.saturating_sub(a.reported.unwrap_or(0)) > 60 {
                a.state = "unknown".into();
            }
            true
        });
        let mut observed = found.to_vec();
        for process in current.values().flatten() {
            if !observed
                .iter()
                .any(|p| p.pid == process.pid && p.start == process.start)
            {
                observed.push(process.clone());
            }
        }
        let candidates = observed
            .iter()
            .filter_map(|p| {
                let tool = process::tool(p, tools)?;
                let watch = watches
                    .iter()
                    .filter(|w| p.cwd.starts_with(&w.root))
                    .max_by_key(|w| w.root.components().count())?;
                Some((p, tool, watch))
            })
            .collect::<Vec<_>>();
        let by_pid = observed
            .iter()
            .map(|p| (p.pid, p))
            .collect::<HashMap<_, _>>();
        let recognized = candidates
            .iter()
            .map(|(p, tool, watch)| (p.pid, (tool, &watch.root)))
            .collect::<HashMap<_, _>>();
        for (p, tool, watch) in &candidates {
            // Wrapper/helper descendants are one process activity, unless hooks explicitly
            // report independent sessions/subagents. Do not guess task identity from argv.
            let mut parent = p.parent;
            let mut helper = false;
            for _ in 0..12 {
                if recognized
                    .get(&parent)
                    .is_some_and(|(name, root)| *name == tool && **root == watch.root)
                {
                    helper = true;
                    break;
                }
                let Some(ancestor) = by_pid.get(&parent) else {
                    break;
                };
                if ancestor.parent <= 1 || ancestor.parent == parent {
                    break;
                }
                parent = ancestor.parent;
            }
            if helper {
                let id = identity(p.pid, &p.start, &watch.root);
                self.rows
                    .retain(|a| a.id != id || a.provenance != "detected");
                continue;
            }
            let id = identity(p.pid, &p.start, &watch.root);
            if let Some(row) = self.rows.iter_mut().find(|a| a.id == id) {
                row.context = watch.context.clone();
                if row.state != "ended" {
                    row.seen = at;
                }
                continue;
            }
            if self.rows.len() >= 256 {
                continue;
            }
            self.rows.push(Activity {
                id,
                tool: tool.clone(),
                session: None,
                parent: None,
                root: watch.root.clone(),
                context: watch.context.clone(),
                state: "detected".into(),
                provenance: "detected".into(),
                started: at,
                seen: at,
                reported: None,
                sequence: 0,
                pid: p.pid,
                start: p.start.clone(),
                branch: None,
                commit: None,
                terminal: None,
            });
        }
        // Bound historical rows independently so ended sessions cannot crowd out live ones.
        let mut recent = 0;
        self.rows.sort_by_key(|a| std::cmp::Reverse(a.seen));
        self.rows.retain(|a| {
            if ["unavailable", "ended"].contains(&a.state.as_str()) {
                recent += 1;
                recent <= 16
            } else {
                true
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn proc(pid: i32, parent: i32) -> process::Process {
        process::Process {
            pid,
            parent,
            start: format!("start-{pid}"),
            executable: "/tools/codex".into(),
            cwd: "/repo".into(),
        }
    }
    fn watch(context: &str) -> Watch {
        Watch {
            owner: "main".into(),
            context: context.into(),
            root: "/repo".into(),
        }
    }
    #[test]
    fn revisiting_a_repository_reuses_process_identity_and_dead_observations_disappear() {
        let mut d = Data::default();
        let p = proc(42, 1);
        d.reconcile(
            &[watch("old-runtime")],
            std::slice::from_ref(&p),
            &HashMap::new(),
            &[],
            1,
        );
        assert_eq!(d.rows.len(), 1);
        let id = d.rows[0].id.clone();
        d.reconcile(
            &[watch("new-runtime")],
            std::slice::from_ref(&p),
            &HashMap::from([(id.clone(), Some(p.clone()))]),
            &[],
            2,
        );
        assert_eq!(d.rows.len(), 1);
        assert_eq!(d.rows[0].id, id);
        assert_eq!(d.rows[0].context, "new-runtime");
        d.reconcile(
            &[watch("new-runtime")],
            &[],
            &HashMap::from([(id, None)]),
            &[],
            3,
        );
        assert!(d.rows.is_empty());
    }
    #[test]
    fn helper_processes_do_not_duplicate_activity_and_pid_reuse_does_not_reuse_a_session() {
        let mut d = Data::default();
        d.reconcile(
            &[watch("review")],
            &[proc(1, 0), proc(2, 1), proc(3, 2)],
            &HashMap::new(),
            &[],
            1,
        );
        assert_eq!(d.rows.len(), 1);
        let mut reused = proc(1, 0);
        reused.start = "new-start".into();
        let old = d.rows[0].id.clone();
        d.reconcile(
            &[watch("review")],
            &[reused.clone()],
            &HashMap::from([(old.clone(), Some(reused))]),
            &[],
            2,
        );
        assert_eq!(d.rows.len(), 1);
        assert_ne!(d.rows[0].id, old);
        assert!(d.rows[0].session.is_none());
    }
}
