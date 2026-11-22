//! Disk-derived operation recovery and bounded, canonical index-stage editing.
use super::*;
use anyhow::{Context, Result};
use serde::Serialize;
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

const MARKERS: &[&str] = &[
    "MERGE_HEAD",
    "MERGE_MSG",
    "MERGE_MODE",
    "MERGE_AUTOSTASH",
    "CHERRY_PICK_HEAD",
    "REVERT_HEAD",
    "REBASE_HEAD",
    "ORIG_HEAD",
    "AUTO_MERGE",
    "rebase-merge",
    "rebase-apply",
    "sequencer",
    "BISECT_LOG",
];
// Operation metadata is mutable even when HEAD, index and marker names stay the same.
pub async fn metadata_fingerprint(git: &Path) -> Result<String> {
    let mut queue = MARKERS.iter().map(|name| git.join(name)).collect::<Vec<_>>();
    let mut entries = vec![];
    let mut size = 0u64;
    let mut visited = 0;
    while let Some(path) = queue.pop() {
        visited += 1;
        anyhow::ensure!(visited <= 512, "Operation metadata exceeds its entry limit");
        let meta = match tokio::fs::symlink_metadata(&path).await {
            Ok(meta) => meta,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
            Err(e) => return Err(e.into()),
        };
        anyhow::ensure!(!meta.is_symlink(), "Operation metadata is symlinked; recover externally");
        if meta.is_dir() {
            let mut dir = tokio::fs::read_dir(&path).await?;
            while let Some(entry) = dir.next_entry().await? {
                queue.push(entry.path());
                anyhow::ensure!(queue.len() + entries.len() <= 512, "Operation metadata exceeds its entry limit");
            }
        } else {
            anyhow::ensure!(meta.is_file(), "Unsupported operation metadata; recover externally");
            size += meta.len();
            anyhow::ensure!(
                size <= 2 * 1024 * 1024 && entries.len() < 512,
                "Operation metadata exceeds 2 MiB; recover externally"
            );
            entries.push((path.strip_prefix(git)?.to_path_buf(), state::path_fingerprint(&path).await?));
        }
    }
    entries.sort();
    Ok(hash(&serde_json::to_vec(&entries)?))
}
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Stage {
    pub stage: u8,
    pub oid: String,
    pub mode: String,
}
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConflictPath {
    pub path: String,
    pub stages: Vec<Stage>,
}
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Recovery {
    pub operation: Option<String>,
    pub commands: Vec<RecoveryCommand>,
    pub conflicts: Vec<ConflictPath>,
    pub current_commit: Option<String>,
    pub onto: Option<String>,
    pub branch: Option<String>,
    pub explanation: String,
}
async fn small_file(git: &Path, name: &str) -> Result<Option<String>> {
    let path = git.join(name);
    use tokio::io::AsyncReadExt;
    let mut bytes = Vec::new();
    let file = match tokio::fs::File::open(&path).await {
        Ok(file) => file,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e.into()),
    };
    file.take(64 * 1024 + 1).read_to_end(&mut bytes).await?;
    anyhow::ensure!(bytes.len() <= 64 * 1024, "Operation detail exceeds its limit");
    Ok(Some(String::from_utf8(bytes)?.trim_end_matches('\n').to_owned()))
}
async fn stages(root: &Path) -> Result<Vec<ConflictPath>> {
    let bytes = read(root, &["ls-files", "--unmerged", "-z"]).await?;
    let mut paths = BTreeMap::<String, Vec<Stage>>::new();
    for record in bytes.split(|b| *b == 0).filter(|r| !r.is_empty()) {
        let tab = record.iter().position(|b| *b == b'\t').context("Malformed index stages")?;
        let mut fields = std::str::from_utf8(&record[..tab])?.split(' ');
        let mode = fields.next().context("Missing mode")?.into();
        let oid = fields.next().context("Missing object")?.into();
        let stage = fields.next().context("Missing stage")?.parse()?;
        let path = String::from_utf8(record[tab + 1..].to_vec()).context("Non-UTF-8 conflicts require external Git")?;
        paths.entry(path).or_default().push(Stage { stage, oid, mode });
        anyhow::ensure!(paths.len() <= 2000, "More than 2,000 conflicts; recover externally");
    }
    Ok(paths.into_iter().map(|(path, stages)| ConflictPath { path, stages }).collect())
}
pub async fn recovery(root: &Path) -> Result<Recovery> {
    let id = identity(root).await?;
    let git = &id.git;
    metadata_fingerprint(git).await?;
    let conflicts = stages(&id.root).await?;
    let exists = |name: &str| git.join(name).exists();
    let operation = if exists("rebase-merge") || exists("rebase-apply") {
        Some("rebase")
    } else if exists("MERGE_HEAD") {
        Some("merge")
    } else if exists("CHERRY_PICK_HEAD") {
        Some("cherry-pick")
    } else if exists("REVERT_HEAD") {
        Some("revert")
    } else if exists("sequencer") {
        let todo = small_file(git, "sequencer/todo").await?.unwrap_or_default();
        if todo.starts_with("pick ") {
            Some("cherry-pick")
        } else if todo.starts_with("revert ") {
            Some("revert")
        } else {
            Some("unknown")
        }
    } else if exists("BISECT_LOG") {
        Some("bisect")
    } else {
        None
    };
    let mut commands = vec![];
    if matches!(operation, Some("merge" | "cherry-pick" | "revert" | "rebase")) {
        if conflicts.is_empty() {
            commands.push(RecoveryCommand::Continue);
        }
        if operation != Some("merge") {
            commands.push(RecoveryCommand::Skip);
        }
        commands.push(RecoveryCommand::Abort);
    }
    let mut external_todo = false;
    if operation == Some("rebase") && exists("rebase-merge") {
        for name in ["rebase-merge/git-rebase-todo", "rebase-merge/done"] {
            if let Some(todo) = small_file(git, name).await?
                && todo.lines().filter(|line| !line.trim().is_empty() && !line.trim_start().starts_with('#')).any(
                    |line| {
                        let mut fields = line.split_whitespace();
                        !matches!(fields.next(), Some("pick" | "drop" | "fixup" | "squash"))
                            || !fields.next().is_some_and(|oid| {
                                (7..=64).contains(&oid.len()) && oid.bytes().all(|b| b.is_ascii_hexdigit())
                            })
                    },
                )
            {
                external_todo = true;
            }
        }
        if external_todo {
            commands.retain(|command| *command == RecoveryCommand::Abort);
        }
    }
    let prefix = if exists("rebase-merge") { "rebase-merge" } else { "rebase-apply" };
    let branch =
        if operation == Some("rebase") { small_file(git, &format!("{prefix}/head-name")).await? } else { None };
    let onto = if operation == Some("rebase") { small_file(git, &format!("{prefix}/onto")).await? } else { None };
    let marker = match operation {
        Some("rebase") => "REBASE_HEAD",
        Some("merge") => "MERGE_HEAD",
        Some("cherry-pick") => "CHERRY_PICK_HEAD",
        _ => "REVERT_HEAD",
    };
    let current_commit = if operation.is_some() { small_file(git, marker).await? } else { None };
    let mut explanation: String = match operation {
        Some("rebase") => "Rebase: stage 2 is the rebased destination plus commits already replayed; stage 3 is the original commit being replayed.",
        Some("merge") => "Merge: stage 2 is the current checkout; stage 3 is the incoming merge head. Stage 1 can be a synthesized merge base.",
        Some("revert") => "Revert: stage 2 is the current checkout; stage 3 is the parent-side change being restored, not the reverted commit itself.",
        Some("cherry-pick") => "Cherry-pick: stage 2 is the current checkout; stage 3 is the picked commit.",
        Some(_) => "This operation requires external Git recovery. No unsupported continue/skip/abort command is offered.",
        None if !conflicts.is_empty() => "Unmerged index without an ongoing sequencer (for example stash apply). Resolve and stage each path, then inspect; there is no sequencer to continue or abort.",
        None => "No ongoing Git operation detected.",
    }.into();
    if external_todo {
        explanation.push_str(" This rebase contains unsupported todo instructions; continue/skip require external Git. Abort remains available.");
    }
    Ok(Recovery {
        operation: operation.map(str::to_owned),
        commands,
        conflicts,
        current_commit,
        onto,
        branch,
        explanation,
    })
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Conflict {
    pub path: String,
    pub draft_key: String,
    pub expected: String,
    pub supported: bool,
    pub reason: String,
    pub labels: Vec<String>,
    pub stages: Vec<Stage>,
    pub contents: Vec<String>,
    pub result: String,
}
pub fn has_markers(text: &str) -> bool {
    text.lines().any(|line| ["<<<<<<<", "=======", ">>>>>>>", "|||||||"].iter().any(|m| line.starts_with(m)))
}
pub async fn conflict(root: &Path, path: &str) -> Result<Conflict> {
    let snapshot = inspect(root, &Action::Commit { message: String::new(), amend: false }).await?;
    let recovery = recovery(root).await?;
    let row = recovery.conflicts.into_iter().find(|r| r.path == path).context("Path is no longer unmerged")?;
    let file = validate_path(root, path).await?;
    let stamp = state::path_fingerprint(&file).await?;
    let expected = hash(&serde_json::to_vec(&(&snapshot, &row, &stamp))?);
    let side2 = match recovery.operation.as_deref() {
        Some("rebase") => "Rebased destination (stage 2)",
        _ => "Current checkout (stage 2)",
    };
    let side3 = match recovery.operation.as_deref() {
        Some("rebase") => "Original commit being replayed (stage 3)",
        Some("revert") => "Parent-side inverse (stage 3)",
        Some("cherry-pick") => "Picked commit (stage 3)",
        Some("merge") => "Incoming merge head (stage 3)",
        _ => "Incoming index content (stage 3)",
    };
    let mut view = Conflict {
        path: path.into(),
        draft_key: hash(&serde_json::to_vec(&(
            &row.path,
            &row.stages,
            &recovery.operation,
            &recovery.current_commit,
            &recovery.onto,
        ))?),
        expected,
        supported: false,
        reason: String::new(),
        labels: vec!["Index base, possibly synthesized (stage 1)".into(), side2.into(), side3.into()],
        stages: row.stages,
        contents: vec![],
        result: String::new(),
    };
    if recovery.operation.as_deref().is_some_and(|kind| !matches!(kind, "merge" | "cherry-pick" | "revert" | "rebase"))
    {
        view.reason = "This operation requires external conflict recovery.".into();
        return Ok(view);
    }
    if view.stages.len() != 3
        || view.stages.iter().enumerate().any(|(n, s)| {
            s.stage != n as u8 + 1 || s.mode != view.stages[0].mode || !matches!(s.mode.as_str(), "100644" | "100755")
        })
    {
        view.reason = "Missing sides, add/delete, rename, mode, symlink and submodule conflicts require an external tool. Nothing is marked resolved.".into();
        return Ok(view);
    }
    let attributes = read(root, &["check-attr", "-z", "filter", "working-tree-encoding", "merge", "--", path]).await?;
    for record in attributes.split(|b| *b == 0).collect::<Vec<_>>().chunks(3) {
        if record.len() == 3 && !matches!(record[2], b"unspecified" | b"unset") {
            view.reason = "Configured filters, encodings or merge drivers require external conflict tooling.".into();
            return Ok(view);
        }
    }
    for stage in &view.stages {
        let bytes = read(root, &["cat-file", "blob", &stage.oid]).await?;
        if bytes.len() > 64 * 1024 || bytes.contains(&0) {
            view.reason = "Binary or large conflicts require an external tool.".into();
            return Ok(view);
        }
        let Ok(text) = String::from_utf8(bytes) else {
            view.reason = "Non-UTF-8 conflicts require an external tool.".into();
            return Ok(view);
        };
        view.contents.push(text);
    }
    let meta = tokio::fs::symlink_metadata(&file).await?;
    if !meta.is_file() || meta.len() > 64 * 1024 {
        view.reason = "The working file is not a supported regular text file.".into();
        return Ok(view);
    }
    let bytes = tokio::fs::read(file).await?;
    let Ok(result) = String::from_utf8(bytes) else {
        view.reason = "Non-UTF-8 working content requires external recovery.".into();
        return Ok(view);
    };
    if result.contains('\0') {
        view.reason = "Binary working content requires external recovery.".into();
        return Ok(view);
    }
    view.result = result;
    view.supported = true;
    Ok(view)
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RebaseCommit {
    pub oid: String,
    pub subject: String,
}
pub async fn rebase_commits(root: &Path, upstream: &str) -> Result<Vec<RebaseCommit>> {
    let target = super::prepare::commit(root, upstream).await?;
    let data = text(
        read(root, &["log", "--reverse", "--topo-order", "--format=%H%x09%P%x09%s", &format!("{target}..HEAD")])
            .await?,
    )?;
    let mut rows = vec![];
    for line in data.lines() {
        let mut parts = line.splitn(3, '\t');
        let oid = parts.next().context("Missing commit")?.into();
        let parents = parts.next().context("Missing parents")?;
        anyhow::ensure!(
            parents.split_whitespace().count() == 1,
            "Merge/root commits require external rebase; topology is never silently flattened"
        );
        rows.push(RebaseCommit { oid, subject: parts.next().unwrap_or_default().into() });
        anyhow::ensure!(rows.len() <= 100, "More than 100 commits; use external rebase");
    }
    anyhow::ensure!(!rows.is_empty(), "No commits to replay");
    Ok(rows)
}
pub async fn prepare(root: &Path, action: Action) -> Result<Plan> {
    let (before, rows) = state::inspect_parts(root, &action).await?;
    let root = &before.identity.root;
    let state = recovery(root).await?;
    let mut steps = vec![];
    let args = |parts: &[&str]| Step { args: parts.iter().map(|s| s.to_string()).collect(), input: None };
    let preview;
    match &action {
        Action::Recover { operation, command } => {
            anyhow::ensure!(
                state.operation.as_deref() == Some(operation) && state.commands.contains(command),
                "Recovery state changed or this command is unavailable; refresh"
            );
            let flag = match command {
                RecoveryCommand::Continue => "--continue",
                RecoveryCommand::Skip => "--skip",
                RecoveryCommand::Abort => "--abort",
            };
            steps.push(args(&[operation, flag]));
            preview = format!(
                "{} {flag}\nCurrent commit: {}\n{}\nContinue includes the current staged resolution. Skip discards the current replayed change. Abort asks Git to return to its recorded start; external edits/autostash may prevent or complicate recovery. Completed operations cannot be aborted. Installed hooks/signing run without terminal input.",
                operation,
                state.current_commit.as_deref().unwrap_or("none"),
                state.explanation
            );
        }
        Action::ResolveText { path, content, expected } => {
            let view = conflict(root, path).await?;
            anyhow::ensure!(
                view.supported && view.expected == *expected,
                "Conflict stages or working content changed; reload before resolving"
            );
            anyhow::ensure!(
                content.len() <= 64 * 1024 && !content.contains('\0') && !has_markers(content),
                "Resolve every conflict marker and keep the text under 64 KiB before staging"
            );
            steps.push(args(&["add", "--", &format!(":(literal){path}")]));
            preview = format!(
                "Replace the working file {path} with your edited text and stage that exact path. Other index entries are kept. This clears its unmerged stages; continue the operation separately.\n\n{content}"
            );
        }
        _ => {
            anyhow::ensure!(
                state.operation.is_none() && before.operations.is_empty() && !before.conflicts && rows.is_empty(),
                "Start from a clean checkout with no ongoing operation; recover existing work first"
            );
            anyhow::ensure!(
                before.head.is_some() && before.branch.is_some(),
                "Start from an existing checked-out branch"
            );
            let common = ["-c", "rerere.enabled=false", "-c", "merge.autoStash=false", "-c", "rebase.autoStash=false"];
            match &action {
                Action::Merge { revision } => {
                    let target = super::prepare::commit(root, revision).await?;
                    let summary = text(
                        read(root, &["log", "--max-count=100", "--format=%h %s", &format!("HEAD..{target}")]).await?,
                    )?;
                    let mut step = args(&common);
                    step.args.extend(
                        ["merge", "--no-edit", "--no-autostash", "--no-rerere-autoupdate", "--"].map(str::to_string),
                    );
                    step.args.push(target.clone());
                    steps.push(step);
                    preview = format!(
                        "Merge {revision} ({target}) into {}. Git may fast-forward, create a merge commit, or stop with conflicts.\nIncoming commits (up to 100):\n{summary}",
                        before.branch.as_deref().unwrap()
                    );
                }
                Action::CherryPick { revision, mainline } | Action::Revert { revision, mainline } => {
                    let target = super::prepare::commit(root, revision).await?;
                    let parents = text(read(root, &["rev-list", "--parents", "-n", "1", &target]).await?)?
                        .split_whitespace()
                        .count()
                        - 1;
                    anyhow::ensure!(
                        (parents > 1 && mainline.is_some_and(|n| n > 0 && n as usize <= parents))
                            || (parents <= 1 && mainline.is_none()),
                        "Merge commits require an explicit valid mainline parent; ordinary commits require none"
                    );
                    let operation = if matches!(action, Action::Revert { .. }) { "revert" } else { "cherry-pick" };
                    let mut step = args(&common);
                    step.args.extend([operation, "--no-edit", "--no-rerere-autoupdate"].map(str::to_string));
                    if let Some(parent) = mainline {
                        step.args.extend(["--mainline".into(), parent.to_string()]);
                    }
                    step.args.extend(["--".into(), target.clone()]);
                    steps.push(step);
                    let subject = text(read(root, &["show", "--no-patch", "--format=%h %s", &target]).await?)?;
                    preview = format!(
                        "{operation} {subject}\nExact commit: {target}\nMainline parent: {mainline:?}\nThis creates a new commit or stops for recovery; no automatic rollback."
                    );
                }
                Action::Rebase { upstream } | Action::RebaseInteractive { upstream, .. } => {
                    let target = super::prepare::commit(root, upstream).await?;
                    let commits = rebase_commits(root, upstream).await?;
                    let mut step = args(&common);
                    step.args.extend(
                        [
                            "rebase",
                            "--merge",
                            "--no-autostash",
                            "--no-update-refs",
                            "--no-fork-point",
                            "--no-rebase-merges",
                            "--no-autosquash",
                            "--keep-empty",
                            "--empty=stop",
                            "--reapply-cherry-picks",
                            "--no-rerere-autoupdate",
                        ]
                        .map(str::to_string),
                    );
                    let mut sequence = String::new();
                    if let Action::RebaseInteractive { todo, .. } = &action {
                        anyhow::ensure!(
                            todo.len() == commits.len(),
                            "The interactive plan must include each commit exactly once (use drop to omit)"
                        );
                        let mut used = std::collections::HashSet::new();
                        let mut kept = false;
                        for item in todo {
                            anyhow::ensure!(
                                commits.iter().any(|c| c.oid == item.oid) && used.insert(&item.oid),
                                "Interactive commit list changed or contains duplicates"
                            );
                            let command = match item.command {
                                RebaseCommand::Pick => "pick",
                                RebaseCommand::Drop => "drop",
                                RebaseCommand::Fixup => "fixup",
                                RebaseCommand::Squash => "squash",
                            };
                            anyhow::ensure!(
                                kept || !matches!(item.command, RebaseCommand::Fixup | RebaseCommand::Squash),
                                "Fixup/squash needs an earlier kept commit"
                            );
                            if !matches!(item.command, RebaseCommand::Drop) {
                                kept = true;
                            }
                            sequence.push_str(&format!("{command} {}\n", item.oid));
                        }
                        anyhow::ensure!(kept, "Keep at least one commit; use external Git to drop the entire range");
                        step.args.push("--interactive".into());
                    }
                    step.args.extend(["--onto".into(), target.clone(), target.clone()]);
                    steps.push(step);
                    preview = format!(
                        "Rebase {} onto {upstream} ({target}). This rewrites local commit IDs, does not update other branches, and may stop for conflicts/empty commits. No force push is performed.\nOriginal replay range:\n{}\nInteractive order (if selected):\n{sequence}",
                        before.branch.as_deref().unwrap(),
                        commits.iter().map(|c| format!("{} {}", c.oid, c.subject)).collect::<Vec<_>>().join("\n")
                    );
                }
                _ => anyhow::bail!("Unsupported advanced action"),
            }
        }
    }
    Ok(Plan { before, action, steps, preview, patch: None, remote: None, extra: vec![], destination: None })
}
// Git invokes a trusted fixed editor script, never renderer-supplied shell text.
pub fn sequence_editor(action: &Action) -> Result<Option<(tempfile::TempDir, PathBuf)>> {
    let Action::RebaseInteractive { todo, .. } = action else { return Ok(None) };
    let dir = tempfile::tempdir()?;
    let text = todo
        .iter()
        .map(|item| {
            format!(
                "{} {}\n",
                match item.command {
                    RebaseCommand::Pick => "pick",
                    RebaseCommand::Drop => "drop",
                    RebaseCommand::Fixup => "fixup",
                    RebaseCommand::Squash => "squash",
                },
                item.oid
            )
        })
        .collect::<String>();
    std::fs::write(dir.path().join("todo"), text)?;
    let script = dir.path().join("editor");
    std::fs::write(&script, b"#!/bin/sh\nset -eu\ncat -- \"$(dirname -- \"$0\")/todo\" > \"$1\"\n")?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o700))?;
    }
    Ok(Some((dir, script)))
}

/// Replace through pinned directory descriptors. No symlink/hard-link target is edited,
/// and a cancelled later index install leaves the file visible for external recovery.
#[cfg(unix)]
pub fn write_result(root: &Path, path: &str, bytes: &[u8], expected: (u64, u64)) -> Result<()> {
    use std::{
        ffi::CString,
        io::Write,
        os::{
            fd::{AsRawFd, FromRawFd},
            unix::fs::{MetadataExt, OpenOptionsExt},
        },
    };
    anyhow::ensure!(
        !path.is_empty()
            && Path::new(path)
                .components()
                .all(|part| matches!(part,std::path::Component::Normal(name) if !name.eq_ignore_ascii_case(".git"))),
        "Resolution requires an exact relative repository file"
    );
    let mut folder = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(root)?;
    let meta = folder.metadata()?;
    anyhow::ensure!((meta.dev(), meta.ino()) == expected, "Checkout identity changed");
    let components = Path::new(path).components().collect::<Vec<_>>();
    let (leaf, parents) = components.split_last().context("Missing file name")?;
    for part in parents {
        let part = CString::new(part.as_os_str().as_encoded_bytes())?;
        let fd = unsafe {
            libc::openat(
                folder.as_raw_fd(),
                part.as_ptr(),
                libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
            )
        };
        if fd < 0 {
            return Err(std::io::Error::last_os_error()).context("A conflict parent changed");
        }
        folder = unsafe { std::fs::File::from_raw_fd(fd) };
    }
    let leaf = CString::new(leaf.as_os_str().as_encoded_bytes())?;
    let fd =
        unsafe { libc::openat(folder.as_raw_fd(), leaf.as_ptr(), libc::O_RDONLY | libc::O_NOFOLLOW | libc::O_CLOEXEC) };
    if fd < 0 {
        return Err(std::io::Error::last_os_error()).context("Conflict file changed");
    }
    let old = unsafe { std::fs::File::from_raw_fd(fd) };
    let meta = old.metadata()?;
    anyhow::ensure!(meta.is_file() && meta.nlink() == 1, "Conflict target must be a regular file without hard links");
    let name = CString::new(format!(".mdiff-resolution-{}", uuid::Uuid::new_v4()))?;
    let fd = unsafe {
        libc::openat(
            folder.as_raw_fd(),
            name.as_ptr(),
            libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL | libc::O_NOFOLLOW | libc::O_CLOEXEC,
            0o600,
        )
    };
    if fd < 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    let result = (|| -> Result<()> {
        let mut file = unsafe { std::fs::File::from_raw_fd(fd) };
        file.write_all(bytes)?;
        file.sync_all()?;
        anyhow::ensure!(
            unsafe { libc::fchmod(file.as_raw_fd(), (meta.mode() & 0o777) as libc::mode_t) } == 0,
            "Could not preserve file mode"
        );
        anyhow::ensure!(
            unsafe { libc::renameat(folder.as_raw_fd(), name.as_ptr(), folder.as_raw_fd(), leaf.as_ptr()) } == 0,
            "Could not replace conflict result"
        );
        Ok(())
    })();
    if result.is_err() {
        unsafe {
            libc::unlinkat(folder.as_raw_fd(), name.as_ptr(), 0);
        }
    }
    result
}
#[cfg(not(unix))]
pub fn write_result(_root: &Path, _path: &str, _bytes: &[u8], _expected: (u64, u64)) -> Result<()> {
    anyhow::bail!("Conflict writes support macOS/Linux")
}
