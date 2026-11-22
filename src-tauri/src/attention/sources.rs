use super::model::*;
use crate::hosting::{
    types::{Provider, ReviewRef},
    Connection,
};
use anyhow::Result;
use serde::Serialize;
use std::collections::{HashMap, HashSet};
use tauri::Manager;
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Repository {
    pub workspace: String,
    pub workspace_name: String,
    pub tab: String,
    pub label: String,
    pub root: String,
    pub active: bool,
    #[serde(skip)]
    pub review: Option<ReviewRef>,
}
pub async fn inventory(app: &tauri::AppHandle) -> Result<Vec<Repository>> {
    let Some(doc) = crate::workspaces::attention_document(app).await? else {
        return Ok(vec![]);
    };
    let visible: HashSet<_> = app
        .state::<crate::windows::Windows>()
        .list("")
        .into_iter()
        .filter(|w| {
            app.get_webview_window(&w.label)
                .is_some_and(|w| w.is_visible().unwrap_or(false))
        })
        .filter_map(|w| w.workspace)
        .collect();
    Ok(doc
        .workspaces
        .into_iter()
        .flat_map(|w| {
            let active = visible.contains(&w.id);
            w.tabs.into_iter().map(move |t| {
                let provider = if t.source.request["kind"] == "gitlab" {
                    Provider::Gitlab
                } else {
                    Provider::Github
                };
                let input = if provider == Provider::Gitlab {
                    t.source.request["url"].as_str()
                } else {
                    t.source.request["pr"].as_str()
                };
                let mut review = input.and_then(|u| ReviewRef::parse(u, provider).ok());
                if let Some(r) = &mut review {
                    if r.site.provider == Provider::Github {
                        r.repository.make_ascii_lowercase();
                    }
                }
                let root = t
                    .saved
                    .as_ref()
                    .map(|s| s.root.to_string_lossy().into_owned())
                    .unwrap_or(t.source.path);
                let root = std::path::Path::new(&root)
                    .canonicalize()
                    .map(|p| p.to_string_lossy().into_owned())
                    .unwrap_or(root);
                Repository {
                    workspace: w.id.clone(),
                    workspace_name: w.name.clone(),
                    root,
                    tab: t.id.clone(),
                    label: t.source.label,
                    active: active && w.active.as_ref() == Some(&t.id),
                    review,
                }
            })
        })
        .take(512)
        .collect())
}
pub struct Batch {
    pub scope: String,
    pub source: &'static str,
    pub items: Vec<Item>,
    pub complete: bool,
    pub warning: Option<String>,
}
impl Batch {
    pub fn error(scope: String, source: &'static str) -> Self {
        Self{warning:Some(format!("{} unavailable ({scope}); previous observations remain unconfirmed. Check the repository or Connections, then refresh.",source)),scope,source,items:vec![],complete:false}
    }
}
pub async fn conflicts(repo: &Repository) -> Result<Batch> {
    let root = std::path::Path::new(&repo.root).canonicalize()?;
    let scope = format!("conflict:{}", crate::storage::digest(&repo.root));
    let recovery = tokio::time::timeout(
        std::time::Duration::from_secs(10),
        diffle::git::operations::advanced::recovery(&root),
    )
    .await??;
    let items = if recovery.conflicts.is_empty() {
        vec![]
    } else {
        vec![Item::observed(
            &scope,
            "conflicts",
            "unmerged",
            format!(
                "{} unresolved conflict{}",
                recovery.conflicts.len(),
                if recovery.conflicts.len() == 1 {
                    ""
                } else {
                    "s"
                }
            ),
            repo.label.clone(),
            Target {
                kind: "conflicts".into(),
                workspace: Some(repo.workspace.clone()),
                tab: Some(repo.tab.clone()),
                root: Some(repo.root.clone()),
                ..Default::default()
            },
            super::now(),
        )]
    };
    Ok(Batch {
        scope,
        source: "conflicts",
        items,
        complete: true,
        warning: None,
    })
}
pub fn agents(app: &tauri::AppHandle, previous: &[Item]) -> (Batch, Vec<Item>) {
    agent_batch(app.state::<crate::agents::Agents>().attention(), previous)
}
pub(super) fn agent_batch(
    rows: Vec<(crate::agents::Activity, String)>,
    previous: &[Item],
) -> (Batch, Vec<Item>) {
    let mut waiting = vec![];
    let mut running = vec![];
    let mut answered = HashSet::new();
    for (a, owner) in rows {
        let mut item = Item::observed(
            "agents",
            "agents",
            &a.id,
            format!(
                "{}{}: {}",
                a.tool,
                if a.parent.is_some() { " subagent" } else { "" },
                a.state
            ),
            a.root.to_string_lossy().into_owned(),
            Target {
                kind: "agents".into(),
                owner: Some(owner),
                context: Some(a.context),
                terminal: a.terminal,
                head: a.commit,
                ..Default::default()
            },
            super::now(),
        );
        item.created = a.started;
        item.updated = a.started;
        if a.provenance == "hook"
            && ["started", "working", "turn completed", "ended"].contains(&a.state.as_str())
        {
            answered.insert(item.id.clone());
        }
        if ["unavailable", "ended"].contains(&a.state.as_str()) {
            continue;
        }
        if a.provenance == "hook" && a.state == "waiting" {
            waiting.push(item)
        } else {
            running.push(item)
        }
    }
    for old in previous
        .iter()
        .filter(|i| i.source == "agents" && i.resolved.is_none())
    {
        if !answered.contains(&old.id) && !waiting.iter().any(|i| i.id == old.id) {
            let mut item = old.clone();
            item.stale = true;
            waiting.push(item);
        }
    }
    (
        Batch {
            scope: "agents".into(),
            source: "agents",
            items: waiting,
            complete: true,
            warning: None,
        },
        running,
    )
}
pub struct Connected {
    pub connection: Connection,
    pub actor: Option<u64>,
}
#[derive(Default)]
pub struct RemoteState {
    connections: HashMap<String, Option<Connected>>,
    pub schedule: Schedule,
}
impl RemoteState {
    pub fn clear(&mut self) {
        self.connections.clear();
        self.schedule = Schedule::default();
    }
    async fn connection(&mut self, app: &tauri::AppHandle, r: &Remote) -> Result<&Connected> {
        let key = format!("{:?}:{}", r.site.provider, r.site.host);
        if !self.connections.contains_key(&key) {
            let value = match crate::hosting::connection(app, r.site.clone()).await {
                Ok(connection) if connection.token.is_some() => {
                    let actor = if r.site.provider == Provider::Gitlab {
                        app.state::<crate::hosting::Hosting>()
                            .broker
                            .user(&connection)
                            .await
                            .ok()
                            .and_then(|v| v["id"].as_u64())
                    } else {
                        None
                    };
                    Some(Connected { connection, actor })
                }
                _ => None,
            };
            self.connections.insert(key.clone(), value);
        }
        self.connections
            .get(&key)
            .and_then(Option::as_ref)
            .ok_or_else(|| anyhow::anyhow!("Connect this host, then refresh the inbox"))
    }
    pub async fn poll(
        &mut self,
        app: &tauri::AppHandle,
        p: &Preferences,
        doc: &Document,
        repos: &[Repository],
        seconds: u64,
    ) -> Option<Batch> {
        let mut jobs = vec![];
        let mut remote_refs = HashMap::new();
        // Only explicitly configured remote repositories can generate remote jobs.
        for r in &p.repositories {
            if p.requested {
                jobs.push((
                    format!("requested:{}", r.key()),
                    repos.iter().any(|x| {
                        x.active && x.review.as_ref().is_some_and(|review| r.matches(review))
                    }),
                ));
            }
        }
        let mut refs = vec![];
        if p.checks {
            for i in &doc.items {
                if i.resolved.is_none() {
                    if let Some(r) = &i.target.review {
                        if !refs.contains(r) {
                            refs.push(r.clone());
                        }
                    }
                }
            }
            if let Ok(Some(workspaces)) = crate::workspaces::attention_document(app).await {
                for w in workspaces.workspaces {
                    for t in w.tabs {
                        let url = if t.source.request["kind"] == "gitlab" {
                            t.source.request["url"].as_str()
                        } else {
                            t.source.request["pr"].as_str()
                        };
                        let provider = if t.source.request["kind"] == "gitlab" {
                            Provider::Gitlab
                        } else {
                            Provider::Github
                        };
                        if let Some(r) = url.and_then(|u| ReviewRef::parse(u, provider).ok()) {
                            if !refs.contains(&r) {
                                refs.push(r);
                            }
                        }
                    }
                }
            }
            for r in refs
                .into_iter()
                .map(|mut r| {
                    if r.site.provider == Provider::Github {
                        r.repository.make_ascii_lowercase();
                    }
                    r
                })
                .filter(|r| p.repositories.iter().any(|repo| repo.matches(r)))
                .take(64)
            {
                let key = format!("checks:{}", r.url());
                if remote_refs.contains_key(&key) {
                    continue;
                }
                jobs.push((
                    key.clone(),
                    repos
                        .iter()
                        .any(|x| x.active && x.review.as_ref() == Some(&r)),
                ));
                remote_refs.insert(key, r);
            }
        }
        jobs.sort_by_key(|(_, active)| !*active);
        let key = self.schedule.next(&jobs, seconds)?;
        let (source, remote) = if let Some(r) = remote_refs.get(&key) {
            (
                "checks",
                Remote {
                    site: r.site.clone(),
                    repository: r.repository.clone(),
                },
            )
        } else {
            (
                "requested",
                p.repositories
                    .iter()
                    .find(|r| key == format!("requested:{}", r.key()))?
                    .clone(),
            )
        };
        let result=tokio::time::timeout(std::time::Duration::from_secs(20),async {
            let connected=self.connection(app,&remote).await?;
            let broker=&app.state::<crate::hosting::Hosting>().broker;
            if source=="requested" {
                let result=broker.attention_requested(&connected.connection,&remote.repository,connected.actor).await?;
                let items=result.reviews.into_iter().map(|(r,title)|Item::observed(&key,source,&r.url(),title,format!("Review requested · {}",r.repository),Target{kind:"review".into(),review:Some(r),..Default::default()},super::now())).collect();
                Ok::<_,anyhow::Error>(Batch{scope:key.clone(),source,items,complete:result.complete,warning:(!result.complete).then(||"More requested reviews exist; this bounded page cannot resolve unseen items. Open Hosting for pagination.".into())})
            } else {
                let r=&remote_refs[&key];let result=broker.attention_checks(&connected.connection,r).await?;
                let items=if result.failed.is_empty(){vec![]}else{vec![Item::observed(&key,source,&result.head,format!("Checks failed · {} #{}",r.repository,r.number),result.failed.join(", "),Target{kind:"checks".into(),review:Some(r.clone()),head:Some(result.head.clone()),url:Some(r.url()),..Default::default()},super::now())]};
                Ok(Batch{scope:key.clone(),source,items,complete:result.complete || !result.open,warning:(!result.complete).then(||"Check summaries are truncated; unseen failures remain unconfirmed.".into())})
            }
        }).await.unwrap_or_else(|_|Err(anyhow::anyhow!("Hosting summary deadline exceeded")));
        Some(match result {
            Ok(batch) => batch,
            Err(_) => {
                self.schedule.failed(key.clone(), seconds);
                Batch::error(key, source)
            }
        })
    }
}
