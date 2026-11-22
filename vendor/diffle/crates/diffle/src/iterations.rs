//! The iterations of a reviewed range: each distinct pair of endpoints a snapshot saw.
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use sha1::{Digest, Sha1};

use crate::git::GitRepo;
use crate::persist::{with_file_lock, write_file_atomic};
use crate::protocol::Iteration;

/// Iterations kept per range; the oldest beyond this are forgotten and unpinned.
pub const MAX_ITERATIONS: usize = 50;

#[derive(Serialize, Deserialize)]
struct File {
    version: u8,
    ranges: BTreeMap<String, Vec<Iteration>>,
}

/// The iterations of one range, under its comment key in `<gitDir>/diffle/iterations.json`. Nothing
/// is cached: every mutation is a read-modify-write of fresh disk state under the file lock, so two
/// servers on one repository keep each other's records. Each iteration's commits are pinned under
/// `refs/diffle/iterations/<key hash>/<n>/{old,new}`.
pub struct IterationStore {
    repo: Arc<GitRepo>,
    pub key: String,
    file: PathBuf,
    refs: String,
    scoped: bool,
    checked: tokio::sync::Mutex<Vec<(i64, String, String)>>,
}

impl IterationStore {
    pub fn new(repo: Arc<GitRepo>, key: String) -> Self {
        let file = repo.data_git_dir.join("diffle").join("iterations.json");
        let scoped = repo.data_git_dir != repo.common_dir;
        let identity = if scoped { format!("{}\0{key}", repo.data_git_dir.display()) } else { key.clone() };
        let hash = hex::encode(Sha1::digest(identity.as_bytes()));
        Self {
            refs: format!("refs/diffle/iterations/{}", &hash[..16]),
            repo,
            key,
            file,
            scoped,
            checked: tokio::sync::Mutex::new(Vec::new()),
        }
    }

    /// Import the legacy per-revision ranges of a single managed MR into its stable
    /// logical range. Publish all required object pins before replacing metadata.
    pub async fn migrate_ranges(&self) -> anyhow::Result<()> {
        with_file_lock(&self.file, || async {
            let mut all = self.read().await?;
            if all.ranges.contains_key(&self.key) {
                return Ok(());
            }
            let mut list: Vec<_> =
                all.ranges.iter().filter(|(key, _)| key.starts_with("range:")).flat_map(|(_, v)| v.clone()).collect();
            list.sort_by_key(|it| it.recorded_at);
            list.dedup_by(|a, b| a.old_sha == b.old_sha && a.new_sha == b.new_sha);
            let start = list.len().saturating_sub(MAX_ITERATIONS);
            list.drain(..start);
            for (i, it) in list.iter_mut().enumerate() {
                it.n = i as i64 + 1;
            }
            let pins: Vec<_> = list
                .iter()
                .flat_map(|it| {
                    [
                        (self.pin(it.n, "old"), Some(it.old_sha.clone())),
                        (self.pin(it.n, "new"), Some(it.new_sha.clone())),
                    ]
                })
                .collect();
            self.repo.pin(&pins).await?;
            all.ranges.insert(self.key.clone(), list);
            self.write(&all).await
        })
        .await
    }

    /// Oldest first.
    pub async fn list(&self) -> anyhow::Result<Vec<Iteration>> {
        with_file_lock(&self.file, || async {
            let list = self.read().await?.ranges.remove(&self.key).unwrap_or_default();
            self.ensure_refs(&list).await?;
            Ok(list)
        })
        .await
    }

    pub async fn get(&self, n: i64) -> anyhow::Result<Option<Iteration>> {
        Ok(self.list().await?.into_iter().find(|it| it.n == n))
    }

    /// Records the endpoints as the next iteration, unless the latest already has them, and returns
    /// the list afterwards. Pins the commits first, so a recorded iteration is always reachable.
    pub async fn record(&self, old_sha: &str, new_sha: &str) -> anyhow::Result<Vec<Iteration>> {
        with_file_lock(&self.file, || async {
            let mut all = self.read().await?;
            let mut list = all.ranges.remove(&self.key).unwrap_or_default();
            self.ensure_refs(&list).await?;
            if list.last().is_some_and(|last| last.old_sha == old_sha && last.new_sha == new_sha) {
                return Ok(list);
            }
            let n = list.last().map_or(0, |last| last.n) + 1;
            list.push(Iteration { n, old_sha: old_sha.to_owned(), new_sha: new_sha.to_owned(), recorded_at: now_ms() });
            let dropped: Vec<Iteration> = list.drain(..list.len().saturating_sub(MAX_ITERATIONS)).collect();
            let mut pins =
                vec![(self.pin(n, "old"), Some(old_sha.to_owned())), (self.pin(n, "new"), Some(new_sha.to_owned()))];
            pins.extend(self.unpins(&dropped));
            self.repo.pin(&pins).await?;
            all.ranges.insert(self.key.clone(), list.clone());
            self.write(&all).await?;
            Ok(list)
        })
        .await
    }

    /// Forgets the iteration numbered `n`, if recorded, and unpins its commits. The rest keep their numbers.
    pub async fn remove(&self, n: i64) -> anyhow::Result<Vec<Iteration>> {
        with_file_lock(&self.file, || async {
            let mut all = self.read().await?;
            let list = all.ranges.remove(&self.key).unwrap_or_default();
            let (gone, kept): (Vec<_>, Vec<_>) = list.into_iter().partition(|it| it.n == n);
            if gone.is_empty() {
                return Ok(kept);
            }
            self.repo.pin(&self.unpins(&gone)).await?;
            if !kept.is_empty() {
                all.ranges.insert(self.key.clone(), kept.clone());
            }
            self.write(&all).await?;
            Ok(kept)
        })
        .await
    }

    /// Forgets every iteration of this range and unpins their commits.
    pub async fn clear(&self) -> anyhow::Result<()> {
        with_file_lock(&self.file, || async {
            let mut all = self.read().await?;
            let list = all.ranges.remove(&self.key).unwrap_or_default();
            self.repo.pin(&self.unpins(&list)).await?;
            self.write(&all).await
        })
        .await
    }

    async fn ensure_refs(&self, list: &[Iteration]) -> anyhow::Result<()> {
        if !self.scoped {
            return Ok(());
        }
        let desired: Vec<_> = list.iter().map(|it| (it.n, it.old_sha.clone(), it.new_sha.clone())).collect();
        let mut checked = self.checked.lock().await;
        if *checked == desired {
            return Ok(());
        }
        let pins: Vec<_> = list
            .iter()
            .flat_map(|it| {
                [(self.pin(it.n, "old"), Some(it.old_sha.clone())), (self.pin(it.n, "new"), Some(it.new_sha.clone()))]
            })
            .collect();
        self.repo.pin(&pins).await?;
        *checked = desired;
        Ok(())
    }

    fn pin(&self, n: i64, side: &str) -> String {
        format!("{}/{n}/{side}", self.refs)
    }

    fn unpins(&self, iterations: &[Iteration]) -> Vec<(String, Option<String>)> {
        iterations.iter().flat_map(|it| [(self.pin(it.n, "old"), None), (self.pin(it.n, "new"), None)]).collect()
    }

    async fn read(&self) -> anyhow::Result<File> {
        match tokio::fs::read_to_string(&self.file).await {
            Ok(text) => Ok(serde_json::from_str(&text)?),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(File { version: 1, ranges: BTreeMap::new() }),
            Err(e) => Err(e.into()),
        }
    }

    async fn write(&self, all: &File) -> anyhow::Result<()> {
        write_file_atomic(&self.file, &(serde_json::to_string_pretty(all)? + "\n")).await
    }
}

pub fn now_ms() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_millis() as i64)
}
