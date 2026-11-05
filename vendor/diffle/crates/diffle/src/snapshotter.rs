//! Turns one review into snapshots: trees, changed files, commits, and their patches.
use std::collections::{BTreeSet, HashMap, HashSet};
use std::sync::{Arc, Mutex};

use futures::FutureExt;
use futures::future::{BoxFuture, Shared};

use crate::error::{Error, Result};
use crate::generated::GeneratedFlags;
use crate::git::{GitRepo, WORKTREE};
use crate::iterations::IterationStore;
use crate::mode::view::{Shown, resolve_shown};
use crate::protocol::{
    ChangeStatus, ChangedFile, Iteration, MAX_RANGE_COMMITS, RangeCommits, RangeEnds, Review, ReviewedCommit, Snapshot,
};

/// Patches computed ahead for the first changed files, which the viewer asks for first.
const PREWARM: usize = 3;

type Task<T> = Shared<BoxFuture<'static, Result<T>>>;

struct State {
    version: i64,
    context: u32,
    snapshot: Option<Task<Arc<Snapshot>>>,
    patches: HashMap<String, Task<String>>,
    all: Option<Task<String>>,
}

/// One review's snapshots, cached until `invalidate`. A failed computation is dropped from the cache,
/// so the next caller retries it.
pub struct Snapshotter {
    repo: Arc<GitRepo>,
    pub review: Review,
    /// Where the range's comments live, whatever slice is shown; a lone commit has none.
    range_key: Option<String>,
    /// Records the range's endpoints as taken; absent for reviews that are not followed.
    iterations: Option<IterationStore>,
    state: Mutex<State>,
    generated: GeneratedFlags,
    cancelled: tokio::sync::watch::Sender<u64>,
    identity: String,
}

impl Snapshotter {
    pub fn new(
        repo: Arc<GitRepo>,
        review: Review,
        range_key: Option<String>,
        version: i64,
        context: u32,
        iterations: Option<IterationStore>,
    ) -> Arc<Self> {
        Arc::new(Self {
            repo,
            review,
            range_key,
            iterations,
            state: Mutex::new(State { version, context, snapshot: None, patches: HashMap::new(), all: None }),
            generated: GeneratedFlags::default(),
            cancelled: tokio::sync::watch::channel(0).0,
            identity: uuid::Uuid::new_v4().to_string(),
        })
    }

    /// Change context lines: drops cached patches; the next snapshot carries the new value.
    pub fn set_context(&self, context: u32, version: i64) {
        let mut state = self.state.lock().unwrap();
        if state.context != context {
            state.context = context;
            drop(state);
            self.invalidate(version);
        }
    }

    pub fn invalidate(&self, version: i64) {
        self.cancel_pending();
        let mut state = self.state.lock().unwrap();
        state.version = version;
        state.snapshot = None;
        state.patches.clear();
        state.all = None;
    }

    pub fn cancel_pending(&self) {
        self.cancelled.send_modify(|generation| *generation += 1);
    }

    /// Cancelling a reader drops its Git futures (kill_on_drop), without cancelling a
    /// serialized session mutation or a request owned by another snapshotter.
    pub async fn read<T>(&self, version: i64, job: impl Future<Output = Result<T>>) -> Result<T> {
        let mut cancelled = self.cancelled.subscribe();
        if self.state.lock().unwrap().version != version {
            return Err(Error::revspec("review changed; retry"));
        }
        tokio::select! {
            biased;
            _ = cancelled.changed() => Err(Error::revspec("review changed; retry")),
            result = job => {
                if self.state.lock().unwrap().version != version { return Err(Error::revspec("review changed; retry")); }
                result
            }
        }
    }

    /// Fast path for content edits to known tracked regular files. All ambiguous changes
    /// return None, asking the session to perform its normal full recomputation.
    pub async fn refresh_paths(
        self: &Arc<Self>,
        paths: &HashSet<String>,
        version: i64,
    ) -> Result<Option<Arc<Snapshot>>> {
        let previous = self.current().await?;
        if previous.new_sha != WORKTREE
            || previous.old_sha == WORKTREE
            || paths.is_empty()
            || previous.changed.iter().any(|f| f.old_path.is_some() || f.submodule.is_some())
            || self.repo.resolve("HEAD").await.unwrap_or_default() != previous.head_sha
        {
            return Ok(None);
        }
        let tracked: HashSet<_> = self.repo.ls_files().await?.into_iter().collect();
        for path in paths {
            if !tracked.contains(path)
                || !previous.tree.contains(path)
                || !tokio::fs::symlink_metadata(self.repo.root.join(path)).await.is_ok_and(|m| m.is_file())
            {
                return Ok(None);
            }
        }
        let selected: Vec<String> = paths.iter().cloned().collect();
        let mut changed = self.repo.numstat_paths(&previous.old_sha, &previous.new_sha, Some(&selected)).await?;
        if changed.iter().any(|f| f.status != ChangeStatus::M || f.old_path.is_some()) {
            return Ok(None);
        }
        self.generated.fill(&self.repo, &mut changed, WORKTREE).await;
        let mut snap = (*previous).clone();
        snap.version = version;
        snap.changed.retain(|f| !paths.contains(&f.path));
        snap.changed.extend(changed);
        snap.changed.sort_by(|a, b| a.path.cmp(&b.path));
        let snap = Arc::new(snap);
        self.cancel_pending();
        let mut state = self.state.lock().unwrap();
        state.version = version;
        state.patches.retain(|path, task| !paths.contains(path) && task.peek().is_some_and(Result::is_ok));
        state.all = None;
        let ready = snap.clone();
        state.snapshot = Some(async move { Ok(ready) }.boxed().shared());
        Ok(Some(snap))
    }

    pub async fn current(self: &Arc<Self>) -> Result<Arc<Snapshot>> {
        let (task, version) = {
            let mut state = self.state.lock().unwrap();
            let version = state.version;
            let this = self.clone();
            let task =
                state.snapshot.get_or_insert_with(|| async move { this.compute(version).await }.boxed().shared());
            (task.clone(), version)
        };
        let result = task.await;
        if result.is_err() {
            self.forget(|state| state.version == version, |state| state.snapshot = None);
        }
        result
    }

    /// Clears what `clear` names, if `still` holds: a failure must not evict a newer computation.
    fn forget(&self, still: impl FnOnce(&State) -> bool, clear: impl FnOnce(&mut State)) {
        let mut state = self.state.lock().unwrap();
        if still(&state) {
            clear(&mut state);
        }
    }

    pub async fn patch(self: &Arc<Self>, path: &str) -> Result<Option<String>> {
        let snap = self.current().await?;
        let Some(file) = snap.changed.iter().find(|f| f.path == path) else {
            return Ok(None);
        };
        self.patch_for(&snap, file).await.map(Some)
    }

    /// Parsed hunks are shared by graph passes and bounded by the same repository budget.
    pub async fn changes(self: &Arc<Self>, path: &str) -> Result<crate::comments::hunks::NewSideChanges> {
        use sha1::{Digest, Sha1};
        let patch = self.patch(path).await?.unwrap_or_default();
        let key = format!("hunks:{}", hex::encode(Sha1::digest(patch.as_bytes())));
        if let Some(cached) = self.repo.patches.get(&key)
            && let Ok(changes) = serde_json::from_str(&cached)
        {
            return Ok(changes);
        }
        let changes = crate::comments::hunks::new_side_changes(&patch);
        self.repo.patches.insert(key, serde_json::to_string(&changes).expect("hunks"));
        Ok(changes)
    }

    /// Search filters share a content-addressed hunk index across repeated queries.
    pub async fn search_ranges(
        self: &Arc<Self>,
        path: &str,
        side: crate::protocol::Side,
        context: bool,
    ) -> Result<Vec<(i64, i64)>> {
        use sha1::{Digest, Sha1};
        let patch = self.patch(path).await?.unwrap_or_default();
        let key = format!("search:{side:?}:{context}:{}", hex::encode(Sha1::digest(patch.as_bytes())));
        if let Some(cached) = self.repo.patches.get(&key)
            && let Ok(ranges) = serde_json::from_str(&cached)
        {
            return Ok(ranges);
        }
        let ranges = if context {
            crate::comments::hunks::shown_ranges(&patch, side)
        } else {
            crate::comments::hunks::changed_ranges(&patch, side)
        };
        self.repo.patches.insert(key, serde_json::to_string(&ranges).expect("search ranges"));
        Ok(ranges)
    }

    /// Frozen, bounded chunks. A digest prevents continuation after cache eviction from
    /// accidentally combining two worktree contents before the watcher has refreshed.
    pub async fn patch_page(
        self: &Arc<Self>,
        path: &str,
        version: i64,
        offset: usize,
        digest: Option<&str>,
    ) -> Result<serde_json::Value> {
        use sha1::{Digest, Sha1};
        let snap = self.current().await?;
        if snap.version != version {
            return Err(Error::revspec("review changed; reload the patch"));
        }
        let file = snap.changed.iter().find(|f| f.path == path).ok_or_else(|| Error::revspec("not a changed file"))?;
        let key = format!("page:{}:{version}:{path}", self.identity);
        let hash_key = format!("digest:{key}");
        let cached = self.repo.patches.get(&key);
        let cached_hash = cached.as_ref().and_then(|_| self.repo.patches.get(&hash_key));
        let patch: Arc<str> = match cached {
            Some(patch) => patch,
            None => {
                let patch = self.patch_for(&snap, file).await?;
                self.repo.patches.insert(key, patch.clone());
                Arc::from(patch)
            }
        };
        let hash = cached_hash.map(|hash| hash.to_string()).unwrap_or_else(|| {
            let hash = hex::encode(Sha1::digest(patch.as_bytes()));
            self.repo.patches.insert(hash_key, hash.clone());
            hash
        });
        if offset > patch.len() || !patch.is_char_boundary(offset) || (offset > 0 && digest != Some(hash.as_str())) {
            return Err(Error::revspec("patch changed; reload the file"));
        }
        let mut end = (offset + 64 * 1024).min(patch.len());
        while !patch.is_char_boundary(end) {
            end -= 1;
        }
        if self.state.lock().unwrap().version != version {
            return Err(Error::revspec("review changed; reload the patch"));
        }
        Ok(
            serde_json::json!({ "version": version, "patch": &patch[offset..end], "next": end, "total": patch.len(), "digest": hash, "done": end == patch.len() }),
        )
    }

    /// Every changed file's patch, concatenated. One git call plus untracked files.
    pub async fn patch_all(self: &Arc<Self>) -> Result<String> {
        let snap = self.current().await?;
        let task = {
            let mut state = self.state.lock().unwrap();
            if state.version != snap.version {
                return Err(Error::revspec("review changed; retry"));
            }
            let (repo, context, snap) = (self.repo.clone(), snap.context as u32, snap.clone());
            let mut cancelled = self.cancelled.subscribe();
            let all = async move {
                tokio::select! {
                    biased;
                    _ = cancelled.changed() => Err(Error::revspec("review changed; retry")),
                    result = repo.patch_all(&snap.old_sha, &snap.new_sha, &snap.changed, context) => Ok(result?),
                }
            };
            state.all.get_or_insert_with(|| all.boxed().shared()).clone()
        };
        let result = task.clone().await;
        if result.is_err() {
            self.forget(|state| state.all.as_ref().is_some_and(|t| t.ptr_eq(&task)), |state| state.all = None);
        }
        result
    }

    /// Cached files are reused; uncached files still share one Git process.
    pub async fn patch_many(self: &Arc<Self>, paths: &[String]) -> Result<String> {
        let snap = self.current().await?;
        let by_path: HashMap<&str, &ChangedFile> = snap.changed.iter().map(|f| (f.path.as_str(), f)).collect();
        let mut seen = BTreeSet::new();
        let mut cached = Vec::new();
        let mut rest = Vec::new();
        for path in paths.iter().filter(|p| seen.insert(p.as_str())) {
            let Some(file) = by_path.get(path.as_str()) else { continue };
            if let Some(value) = patch_key(&snap, file).and_then(|key| self.repo.patches.get(&key)) {
                cached.push(value.to_string());
            } else {
                rest.push((*file).clone());
            }
        }
        let batch = match rest.as_slice() {
            [] => String::new(),
            [file] => self.patch_for(&snap, file).await?,
            _ => {
                let patch = self
                    .read(snap.version, async {
                        Ok(self.repo.patch_many(&snap.old_sha, &snap.new_sha, &rest, snap.context as u32).await?)
                    })
                    .await?;
                // Match full headers against known paths, never split a filename on spaces.
                let headers: HashMap<String, &ChangedFile> =
                    rest.iter().map(|file| (patch_header(file), file)).collect();
                for part in patch_parts(&patch) {
                    if let Some(file) = part.lines().next().and_then(|header| headers.get(header))
                        && let Some(key) = patch_key(&snap, file)
                    {
                        self.repo.patches.insert(key, part.to_owned());
                    }
                }
                patch
            }
        };
        if self.state.lock().unwrap().version != snap.version {
            return Err(Error::revspec("review changed; retry"));
        }
        Ok(cached.concat() + &batch)
    }

    fn patch_for(
        self: &Arc<Self>,
        snap: &Arc<Snapshot>,
        file: &ChangedFile,
    ) -> impl Future<Output = Result<String>> + use<> {
        let (this, snap, file) = (self.clone(), snap.clone(), file.clone());
        async move {
            // Immutable comparisons can safely outlive the snapshot and survive navigation.
            // Live files remain generation-scoped until their contents have been verified.
            let key = patch_key(&snap, &file);
            if let Some(value) = key.as_ref().and_then(|key| this.repo.patches.get(key)) {
                return Ok(value.to_string());
            }
            let task = {
                let mut state = this.state.lock().unwrap();
                if state.version != snap.version {
                    return Err(Error::revspec("review changed; reload the patch"));
                }
                let (repo, snap, file) = (this.repo.clone(), snap.clone(), file.clone());
                let mut cancelled = this.cancelled.subscribe();
                state.patches.entry(file.path.clone()).or_insert_with(|| async move {
                    tokio::select! {
                        biased;
                        _ = cancelled.changed() => Err(Error::revspec("review changed; retry")),
                        result = repo.patch(&snap.old_sha, &snap.new_sha, &file, snap.context as u32, None) => Ok(result?),
                    }
                }.boxed().shared()).clone()
            };
            let result = task.clone().await;
            if let (Some(key), Ok(value)) = (key, &result) {
                this.repo.patches.insert(key, value.clone());
            }
            // Completed immutable patches live only in the bounded repository cache. Failed
            // jobs are always retryable; live results are bounded to a small working set.
            this.forget(
                |state| state.patches.get(&file.path).is_some_and(|t| t.ptr_eq(&task)),
                |state| {
                    if result.is_err()
                        || snap.new_sha != WORKTREE && snap.old_sha != WORKTREE
                        || state.patches.len() > 64
                        || result.as_ref().is_ok_and(|p| p.len() > 512 * 1024)
                    {
                        state.patches.remove(&file.path);
                    }
                },
            );
            if this.state.lock().unwrap().version != snap.version {
                return Err(Error::revspec("review changed; retry"));
            }
            result
        }
    }

    async fn compute(self: Arc<Self>, version: i64) -> Result<Arc<Snapshot>> {
        let repo = &self.repo;
        let shown = resolve_shown(repo, &self.review, self.iterations.as_ref(), self.range_key.as_deref());
        let (shown, head_sha) = tokio::join!(shown, repo.resolve("HEAD"));
        let (shown, head_sha) = (shown?, head_sha.unwrap_or_default());
        let Shown { old_sha, new_sha, follows, range, interdiff, commit, comment_key } = shown;
        // The tree belongs to the selected new side: a commit's own listing, or the index plus
        // untracked files (added below via `changed`) for the worktree.
        let tracked = async {
            if new_sha == WORKTREE {
                let (files, untracked) = tokio::try_join!(repo.ls_files(), repo.untracked())?;
                Ok::<_, Error>(files.into_iter().chain(untracked).filter(|p| !p.ends_with('/')).collect::<Vec<_>>())
            } else {
                Ok(repo.ls_tree(&new_sha).await?)
            }
        };
        let commit = async {
            match &commit {
                Some(sha) => self.reviewed_commit(sha).await,
                None => Ok(None),
            }
        };
        let (tracked, mut changed, commit, commits) =
            tokio::try_join!(tracked, repo.numstat(&old_sha, &new_sha), commit, self.range_commits(&range, &head_sha))?;
        let (iterations, ()) =
            tokio::join!(self.record_iteration(&commits), self.generated.fill(repo, &mut changed, &new_sha));
        let mut tree: BTreeSet<String> = tracked.into_iter().collect();
        for f in &changed {
            if f.status == ChangeStatus::D {
                tree.remove(&f.path);
            } else {
                tree.insert(f.path.clone());
            }
            if let Some(old) = &f.old_path {
                tree.remove(old);
            }
        }
        let context = self.state.lock().unwrap().context;
        let snap = Arc::new(Snapshot {
            root: repo.root.to_string_lossy().into_owned(),
            review: self.review.clone(),
            version,
            old_sha,
            new_sha,
            head_sha,
            follows,
            comment_key,
            interdiff,
            context: i64::from(context),
            changed,
            tree: tree.into_iter().collect(),
            commit,
            commits,
            iterations: iterations?,
        });
        for f in snap.changed.iter().take(PREWARM) {
            tokio::spawn(crate::scheduler::background(self.patch_for(&snap, f)));
        }
        Ok(snap)
    }

    /// The snapshot taken is the range's current iteration; the worktree has no commit to record.
    async fn record_iteration(&self, commits: &RangeCommits) -> Result<Vec<Iteration>> {
        match &self.iterations {
            Some(store) if commits.old_sha != WORKTREE && commits.new_sha != WORKTREE => {
                Ok(store.record(&commits.old_sha, &commits.new_sha).await?)
            }
            _ => Ok(Vec::new()),
        }
    }

    async fn reviewed_commit(&self, sha: &str) -> Result<Option<ReviewedCommit>> {
        let (info, neighbours) = tokio::try_join!(self.repo.commit_info(sha), self.repo.commit_neighbours(sha))?;
        Ok(info.map(|info| ReviewedCommit {
            sha: info.sha,
            short: info.short,
            message: info.message,
            parent: neighbours.0,
            child: neighbours.1,
        }))
    }

    /// The commits the review's range spans; a lone commit lists itself.
    async fn range_commits(&self, range: &RangeEnds, head_sha: &str) -> Result<RangeCommits> {
        if let Review::Commit { sha } = &self.review {
            let list = self.repo.range_commits(&format!("{sha}^!"), MAX_RANGE_COMMITS).await?;
            return Ok(RangeCommits {
                list: list.list,
                total: list.total,
                old_sha: range.old_sha.clone(),
                new_sha: range.new_sha.clone(),
            });
        }
        self.spanned(&range.old_sha, &range.new_sha, head_sha).await
    }

    async fn spanned(&self, old_sha: &str, new_sha: &str, head_sha: &str) -> Result<RangeCommits> {
        let side = |sha: &str| {
            if sha == WORKTREE { head_sha.to_owned() } else { sha.to_owned() }
        };
        let (from, to) = (side(old_sha), side(new_sha));
        // An unborn branch has no commits; its old side is then the empty tree, which no range can name.
        let (list, total) = if from.is_empty() || to.is_empty() || from == to {
            (Vec::new(), 0)
        } else {
            let commits = self.repo.range_commits(&format!("{from}..{to}"), MAX_RANGE_COMMITS).await?;
            (commits.list, commits.total)
        };
        Ok(RangeCommits { list, total, old_sha: old_sha.into(), new_sha: new_sha.into() })
    }
}

fn patch_key(snap: &Snapshot, file: &ChangedFile) -> Option<String> {
    (snap.old_sha != WORKTREE && snap.new_sha != WORKTREE)
        .then(|| serde_json::to_string(&(&snap.old_sha, &snap.new_sha, snap.context, file)).expect("patch key"))
}

fn patch_header(file: &ChangedFile) -> String {
    fn quote(path: String) -> String {
        if !path.bytes().any(|b| b < 32 || b == 127 || b == b'"' || b == b'\\') {
            return path;
        }
        let mut result = String::from("\"");
        for c in path.chars() {
            match c {
                '\n' => result.push_str("\\n"),
                '\t' => result.push_str("\\t"),
                '\r' => result.push_str("\\r"),
                '"' => result.push_str("\\\""),
                '\\' => result.push_str("\\\\"),
                c if c.is_ascii_control() => result.push_str(&format!("\\{:03o}", c as u8)),
                c => result.push(c),
            }
        }
        result.push('"');
        result
    }
    format!(
        "diff --git {} {}",
        quote(format!("a/{}", file.old_path.as_deref().unwrap_or(&file.path))),
        quote(format!("b/{}", file.path))
    )
}

fn patch_parts(patch: &str) -> Vec<&str> {
    let mut starts: Vec<usize> = patch.match_indices("\ndiff --git ").map(|(at, _)| at + 1).collect();
    if patch.starts_with("diff --git ") {
        starts.insert(0, 0);
    }
    starts.push(patch.len());
    starts.windows(2).map(|w| &patch[w[0]..w[1]]).collect()
}
