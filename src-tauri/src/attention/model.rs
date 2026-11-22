//! Bounded metadata ledger. Missing/incomplete observations are never resolution proof.
use crate::hosting::types::{ReviewRef, Site};
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
pub const MAX_ITEMS: usize = 200;
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Remote {
    pub site: Site,
    pub repository: String,
}
impl Remote {
    pub fn matches(&self, r: &ReviewRef) -> bool {
        self.site == r.site
            && if r.site.provider == crate::hosting::types::Provider::Github {
                self.repository.eq_ignore_ascii_case(&r.repository)
            } else {
                self.repository == r.repository
            }
    }
    pub fn validate(&self) -> Result<()> {
        ReviewRef {
            site: self.site.clone(),
            repository: self.repository.clone(),
            number: 1,
        }
        .validate()
    }
    pub fn key(&self) -> String {
        format!(
            "{:?}:{}:{}",
            self.site.provider,
            self.site.host,
            if self.site.provider == crate::hosting::types::Provider::Github {
                self.repository.to_ascii_lowercase()
            } else {
                self.repository.clone()
            }
        )
    }
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Preferences {
    pub revision: u64,
    pub enabled: bool,
    pub requested: bool,
    pub checks: bool,
    pub conflicts: bool,
    pub agents: bool,
    pub notifications: bool,
    pub notify_requested: bool,
    pub notify_checks: bool,
    pub notify_conflicts: bool,
    pub notify_agents: bool,
    pub tray: bool,
    pub keep_in_tray: bool,
    pub local_tabs: Vec<String>,
    pub repositories: Vec<Remote>,
}
impl Default for Preferences {
    fn default() -> Self {
        Self {
            revision: 0,
            enabled: false,
            requested: true,
            checks: true,
            conflicts: true,
            agents: true,
            notifications: false,
            notify_requested: true,
            notify_checks: true,
            notify_conflicts: true,
            notify_agents: true,
            tray: false,
            keep_in_tray: false,
            local_tabs: vec![],
            repositories: vec![],
        }
    }
}
impl Preferences {
    pub fn validate(&self) -> Result<()> {
        anyhow::ensure!(
            self.local_tabs.len() <= 32 && self.repositories.len() <= 16,
            "Monitor at most 32 local tabs and 16 remote repositories"
        );
        anyhow::ensure!(
            !self.keep_in_tray || self.tray,
            "Enable the menu bar/tray before keeping windows there"
        );
        let mut keys = HashSet::new();
        for tab in &self.local_tabs {
            anyhow::ensure!(
                tab.len() == 64 && tab.bytes().all(|b| b.is_ascii_hexdigit()) && keys.insert(tab),
                "Invalid or duplicate local tab"
            );
        }
        let mut keys = HashSet::new();
        for r in &self.repositories {
            r.validate()?;
            anyhow::ensure!(keys.insert(r.key()), "Duplicate repository");
        }
        Ok(())
    }
    pub fn source(&self, source: &str) -> bool {
        self.enabled
            && match source {
                "requested" => self.requested,
                "checks" => self.checks,
                "conflicts" => self.conflicts,
                "agents" => self.agents,
                _ => false,
            }
    }
    pub fn notify(&self, source: &str) -> bool {
        self.notifications
            && self.source(source)
            && match source {
                "requested" => self.notify_requested,
                "checks" => self.notify_checks,
                "conflicts" => self.notify_conflicts,
                "agents" => self.notify_agents,
                _ => false,
            }
    }
}
fn discard_runtime<'de, D: serde::Deserializer<'de>>(
    d: D,
) -> std::result::Result<Option<String>, D::Error> {
    let _ = serde_json::Value::deserialize(d)?;
    Ok(None)
}
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Target {
    pub kind: String,
    pub workspace: Option<String>,
    pub tab: Option<String>,
    pub review: Option<ReviewRef>,
    pub head: Option<String>,
    pub url: Option<String>,
    #[serde(default)]
    pub root: Option<String>,
    // Live process identities are never restored from the ledger.
    #[serde(default, deserialize_with = "discard_runtime")]
    pub owner: Option<String>,
    #[serde(default, deserialize_with = "discard_runtime")]
    pub context: Option<String>,
    #[serde(default, deserialize_with = "discard_runtime")]
    pub terminal: Option<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Item {
    pub id: String,
    pub scope: String,
    pub source: String,
    pub title: String,
    pub detail: String,
    pub created: u64,
    pub updated: u64,
    pub resolved: Option<u64>,
    pub read: bool,
    pub dismissed: bool,
    pub stale: bool,
    #[serde(default)]
    pub paused: bool,
    pub target: Target,
}
impl Item {
    pub fn observed(
        scope: &str,
        source: &str,
        key: &str,
        title: String,
        detail: String,
        target: Target,
        now: u64,
    ) -> Self {
        Self {
            id: crate::storage::digest(&format!("{source}:{scope}:{key}")),
            scope: scope.into(),
            source: source.into(),
            title: title.chars().take(200).collect(),
            detail: detail.chars().take(300).collect(),
            created: now,
            updated: now,
            resolved: None,
            read: false,
            dismissed: false,
            stale: false,
            paused: false,
            target,
        }
    }
}
fn valid_digest(s: &str) -> bool {
    s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit())
}
const MAX_MARKS: usize = 768;
const MAX_ITEM_BYTES: usize = 256 * 1024;
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Mark {
    #[serde(rename = "s")]
    scope: String,
    #[serde(rename = "p")]
    source: String,
    #[serde(rename = "c")]
    created: u64,
    #[serde(rename = "u")]
    updated: u64,
    #[serde(rename = "x")]
    resolved: Option<u64>,
    #[serde(rename = "r")]
    read: bool,
    #[serde(rename = "d")]
    dismissed: bool,
}
#[derive(Default, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Document {
    pub preferences: Preferences,
    pub items: Vec<Item>,
    #[serde(default)]
    pub marks: HashMap<String, Mark>,
}
impl Document {
    pub fn remember(&mut self) {
        for i in &self.items {
            self.marks.insert(
                i.id.clone(),
                Mark {
                    scope: crate::storage::digest(&i.scope),
                    source: i.source.clone(),
                    created: i.created,
                    updated: i.updated,
                    resolved: i.resolved,
                    read: i.read,
                    dismissed: i.dismissed,
                },
            );
        }
        if self.marks.len() > MAX_MARKS {
            let mut order: Vec<_> = self
                .marks
                .iter()
                .map(|(id, m)| (m.resolved.is_none(), m.updated, id.clone()))
                .collect();
            order.sort();
            for (_, _, id) in order.into_iter().take(self.marks.len() - MAX_MARKS) {
                self.marks.remove(&id);
            }
        }
    }
    pub fn mark_all_read(&mut self) {
        for i in &mut self.items {
            i.read = true;
        }
        for m in self.marks.values_mut() {
            m.read = true;
        }
        self.remember();
    }
    pub fn limited(&self) -> bool {
        self.marks
            .iter()
            .any(|(id, m)| m.resolved.is_none() && !self.items.iter().any(|i| &i.id == id))
    }
    pub fn pause_unconfigured(&mut self, live_tabs: &HashMap<String, String>) {
        for item in &mut self.items {
            item.paused = !self.preferences.source(&item.source)
                || match item.source.as_str() {
                    "requested" | "checks" => item.target.review.as_ref().is_none_or(|r| {
                        !self
                            .preferences
                            .repositories
                            .iter()
                            .any(|repo| repo.matches(r))
                    }),
                    "conflicts" => item.target.tab.as_ref().is_none_or(|tab| {
                        !self.preferences.local_tabs.contains(tab)
                            || !live_tabs.contains_key(tab)
                            || item
                                .target
                                .root
                                .as_ref()
                                .is_some_and(|root| live_tabs.get(tab) != Some(root))
                    }),
                    _ => false,
                };
        }
    }
    pub fn validate(&self) -> Result<()> {
        self.preferences.validate()?;
        anyhow::ensure!(
            self.items.len() <= MAX_ITEMS,
            "Attention history exceeds limit"
        );
        anyhow::ensure!(
            self.marks.len() <= MAX_MARKS
                && self.marks.iter().all(|(id, m)| valid_digest(id)
                    && valid_digest(&m.scope)
                    && ["requested", "checks", "conflicts"].contains(&m.source.as_str())),
            "Invalid bounded attention markers"
        );
        let mut ids = HashSet::new();
        for i in &self.items {
            anyhow::ensure!(
                valid_digest(&i.id)
                    && ids.insert(&i.id)
                    && i.scope.len() <= 2048
                    && i.title.len() <= 800
                    && i.detail.len() <= 1200,
                "Invalid attention metadata"
            );
            anyhow::ensure!(
                ["requested", "checks", "conflicts"].contains(&i.source.as_str()),
                "Runtime agent metadata cannot be restored"
            );
            if let Some(r) = &i.target.review {
                r.validate()?;
            }
            if let Some(url) = &i.target.url {
                let u = url::Url::parse(url)?;
                anyhow::ensure!(
                    u.scheme() == "https"
                        && u.username().is_empty()
                        && u.password().is_none()
                        && i.target
                            .review
                            .as_ref()
                            .is_some_and(|r| u.authority() == r.site.host),
                    "Invalid attention source link"
                );
            }
        }
        Ok(())
    }
    pub fn reconcile(
        &mut self,
        scope: &str,
        found: Vec<Item>,
        complete: bool,
        now: u64,
    ) -> Vec<String> {
        self.remember();
        let present: HashSet<_> = found.iter().map(|i| i.id.clone()).collect();
        let mut notify = vec![];
        for mut next in found {
            if let Some(old) = self.items.iter_mut().find(|i| i.id == next.id) {
                let recurring = old.resolved.is_some();
                next.created = old.created;
                next.read = if recurring { false } else { old.read };
                next.dismissed = if recurring { false } else { old.dismissed };
                next.updated = old.updated;
                if next != *old {
                    next.updated = now;
                }
                if recurring {
                    notify.push(next.id.clone());
                }
                *old = next;
            } else {
                if let Some(mark) = self.marks.get(&next.id) {
                    next.created = mark.created;
                    if mark.resolved.is_none() {
                        next.read = mark.read;
                        next.dismissed = mark.dismissed;
                    } else {
                        notify.push(next.id.clone());
                    }
                } else {
                    notify.push(next.id.clone());
                }
                self.items.push(next);
            }
        }
        for i in self
            .items
            .iter_mut()
            .filter(|i| i.scope == scope && i.resolved.is_none() && !present.contains(&i.id))
        {
            if complete {
                i.resolved = Some(now);
                i.updated = now;
                i.stale = false;
            } else {
                i.stale = true;
            }
        }
        let scope_id = crate::storage::digest(scope);
        if complete {
            for (id, m) in &mut self.marks {
                if m.scope == scope_id && !present.contains(id) && m.resolved.is_none() {
                    m.resolved = Some(now);
                    m.updated = now;
                }
            }
        }
        self.remember();
        self.trim_items();
        notify
    }
    fn trim_items(&mut self) {
        self.items
            .sort_by_key(|i| (i.resolved.is_none() && !i.dismissed && !i.paused, i.updated));
        let excess = self.items.len().saturating_sub(MAX_ITEMS);
        self.items.drain(..excess);
        let mut bytes: usize = self
            .items
            .iter()
            .map(|i| {
                serde_json::to_vec(i)
                    .map(|v| v.len() + 1)
                    .unwrap_or(MAX_ITEM_BYTES)
            })
            .sum();
        while bytes > MAX_ITEM_BYTES && !self.items.is_empty() {
            let old = self.items.remove(0);
            bytes = bytes.saturating_sub(
                serde_json::to_vec(&old)
                    .map(|v| v.len() + 1)
                    .unwrap_or(MAX_ITEM_BYTES),
            );
        }
    }
    pub fn unread(&self) -> usize {
        self.items
            .iter()
            .filter(|i| {
                self.preferences.source(&i.source)
                    && i.resolved.is_none()
                    && !i.dismissed
                    && !i.paused
                    && !i.read
            })
            .count()
    }
}
pub async fn load(file: &std::path::Path) -> Result<Document> {
    if !file.exists() {
        return Ok(Document::default());
    }
    anyhow::ensure!(
        tokio::fs::metadata(file).await?.len() <= 512 * 1024,
        "Attention file exceeds limit; original preserved"
    );
    let mut doc: Document = serde_json::from_slice(&tokio::fs::read(file).await?)
        .context("Attention metadata is invalid; original preserved")?;
    doc.validate()?;
    doc.remember();
    doc.trim_items();
    for i in &mut doc.items {
        if i.resolved.is_none() {
            i.stale = true;
        }
    }
    Ok(doc)
}
pub async fn save(file: &std::path::Path, doc: &Document) -> Result<()> {
    let mut durable = doc.clone();
    durable.remember();
    durable.items.retain(|i| i.source != "agents");
    durable.marks.retain(|_, m| m.source != "agents");
    durable.validate()?;
    let text = serde_json::to_string(&durable)?;
    anyhow::ensure!(text.len() <= 512 * 1024, "Attention metadata exceeds limit");
    diffle::persist::write_file_atomic(file, &text).await
}
/// A due queue shared by all windows. One job per tick; current work gets a shorter interval.
#[derive(Default)]
pub struct Schedule {
    due: HashMap<String, u64>,
    cursor: usize,
    priority_cursor: usize,
    last_priority: bool,
}
impl Schedule {
    pub fn expedite(&mut self, keys: impl Iterator<Item = String>) {
        for key in keys {
            self.due.remove(&key);
        }
    }
    pub fn next(&mut self, jobs: &[(String, bool)], now: u64) -> Option<String> {
        self.due
            .retain(|key, _| jobs.iter().any(|(id, _)| id == key));
        let due = |key: &String| self.due.get(key).is_none_or(|deadline| *deadline <= now);
        let active = jobs
            .iter()
            .enumerate()
            .cycle()
            .skip(self.priority_cursor % jobs.len().max(1))
            .take(jobs.len())
            .find(|(_, (key, active))| *active && due(key))
            .map(|(i, _)| i);
        let background = jobs
            .iter()
            .enumerate()
            .cycle()
            .skip(self.cursor % jobs.len().max(1))
            .take(jobs.len())
            .find(|(_, (key, active))| !*active && due(key))
            .map(|(i, _)| i);
        let priority = active.filter(|_| !self.last_priority || background.is_none());
        let index = if let Some(i) = priority {
            self.last_priority = true;
            self.priority_cursor = i + 1;
            i
        } else {
            let i = background?;
            self.last_priority = false;
            self.cursor = i + 1;
            i
        };
        let (key, active) = &jobs[index];
        self.due
            .insert(key.clone(), now + if *active { 120 } else { 600 });
        Some(key.clone())
    }
    pub fn failed(&mut self, key: String, now: u64) {
        self.due.insert(key, now + 600);
    }
}

#[derive(Default)]
pub struct CycleBudget {
    next: u64,
}
impl CycleBudget {
    pub fn admit(&mut self, seconds: u64) -> bool {
        if seconds < self.next {
            return false;
        }
        self.next = seconds + 15;
        true
    }
}
