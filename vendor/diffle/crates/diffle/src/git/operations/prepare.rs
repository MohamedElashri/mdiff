use super::*;
use anyhow::{Context, Result};
use std::path::{Path, PathBuf};
pub fn validate_url(value: &str) -> Result<()> {
    anyhow::ensure!(value.len() <= 4096 && !value.contains(['\0', '\n', '\r']), "Invalid remote URL");
    if Path::new(value).is_absolute() {
        return Ok(());
    }
    if !value.contains("://") {
        anyhow::ensure!(!value.contains("::"), "Remote-helper command URLs are not supported");
        let (authority, path) =
            value.split_once(':').context("Use an HTTPS/SSH URL, SCP-style SSH address or absolute local path")?;
        let (user, host) = authority.rsplit_once('@').map_or((None, authority), |(user, host)| (Some(user), host));
        let safe = |part: &str| {
            !part.is_empty()
                && !part.starts_with('-')
                && part.bytes().all(|c| c.is_ascii_alphanumeric() || b"._-".contains(&c))
        };
        anyhow::ensure!(
            safe(host) && user.is_none_or(safe) && !path.is_empty() && !path.starts_with('-'),
            "Invalid SCP-style SSH address; use ssh:// for IPv6 or custom ports"
        );
        return Ok(());
    }
    let url = url::Url::parse(value).context("Invalid remote URL")?;
    anyhow::ensure!(
        url.password().is_none() && url.query().is_none() && url.fragment().is_none(),
        "Remote URLs must not contain passwords, tokens or queries"
    );
    if url.scheme() == "file" {
        anyhow::ensure!(
            url.username().is_empty()
                && url.host_str().is_none_or(|host| host == "localhost")
                && url.to_file_path().is_ok_and(|path| path.is_absolute()),
            "Use a local file URL without credentials"
        );
        return Ok(());
    }
    anyhow::ensure!(
        matches!(url.scheme(), "https" | "ssh")
            && url.host_str().is_some_and(|host| !host.starts_with('-'))
            && (url.scheme() == "ssh" || url.username().is_empty()),
        "Use an HTTPS/SSH URL without embedded credentials or remote-helper commands"
    );
    if url.scheme() == "ssh" {
        anyhow::ensure!(
            !url.username().starts_with('-')
                && url.username().bytes().all(|c| c.is_ascii_alphanumeric() || b"._-".contains(&c)),
            "Invalid SSH username"
        );
    }
    Ok(())
}

async fn refname(root: &Path, name: &str, prefix: &str) -> Result<String> {
    anyhow::ensure!(
        !name.is_empty() && name.len() <= 255 && !name.starts_with('-') && !name.contains(['\0', '\n', '\r']),
        "Invalid reference name"
    );
    let reference = format!("refs/{prefix}/{name}");
    read(root, &["check-ref-format", &reference]).await?;
    Ok(reference)
}
pub(super) async fn commit(root: &Path, revision: &str) -> Result<String> {
    anyhow::ensure!(
        !revision.is_empty() && revision.len() <= 1024 && !revision.contains(['\0', '\n', '\r']),
        "Invalid revision"
    );
    text(read(root, &["rev-parse", "--verify", "--end-of-options", &format!("{revision}^{{commit}}")]).await?)
}
async fn oid(root: &Path, reference: &str) -> Result<String> {
    text(read(root, &["rev-parse", "--verify", "--end-of-options", reference]).await?)
}
fn field(value: &str) -> Result<()> {
    anyhow::ensure!(
        !value.is_empty()
            && value.len() <= 255
            && !value.starts_with('-')
            && value.bytes().all(|c| c.is_ascii_alphanumeric() || b"-_.".contains(&c)),
        "Use a remote name containing letters, digits, dot, dash or underscore"
    );
    Ok(())
}
async fn destination(path: &str, extra: &mut Vec<(PathBuf, String)>) -> Result<PathBuf> {
    let path = PathBuf::from(path);
    anyhow::ensure!(
        path.is_absolute() && path.as_os_str().len() <= 4096 && !path.as_os_str().as_encoded_bytes().contains(&0),
        "Choose an absolute destination folder"
    );
    let name = path.file_name().context("Choose a new folder name")?;
    anyhow::ensure!(name != ".git", "Reserved folder name");
    let parent = tokio::fs::canonicalize(path.parent().context("Missing destination parent")?).await?;
    let path = parent.join(name);
    anyhow::ensure!(!path.try_exists()?, "Destination already exists; choose a new folder");
    extra.push((parent.clone(), state::path_fingerprint(&parent).await?));
    extra.push((path.clone(), "missing".into()));
    Ok(path)
}
pub async fn prepare(root: &Path, action: Action) -> Result<Plan> {
    if action.advanced() {
        return super::advanced::prepare(root, action).await;
    }
    let (before, rows) = state::inspect_parts(root, &action).await?;
    let root = before.identity.root.clone();
    let root = root.as_path();
    anyhow::ensure!(
        before.operations.is_empty() && !before.conflicts,
        "A merge/rebase/conflict operation is active. Recover in an external Git client before starting a new operation."
    );
    let clean = rows.is_empty();
    let mut steps = vec![];
    let mut preview;
    let mut patch = None;
    let mut remote = None;
    let mut extra = vec![];
    let mut reserved_destination = None;
    let step = |args: Vec<String>| Step { args, input: None };
    match &action {
        Action::Merge { .. }
        | Action::CherryPick { .. }
        | Action::Revert { .. }
        | Action::Rebase { .. }
        | Action::RebaseInteractive { .. }
        | Action::Recover { .. }
        | Action::ResolveText { .. } => unreachable!(),
        Action::Init { .. } | Action::Clone { .. } => {
            anyhow::bail!("New repositories require native destination ownership")
        }
        Action::BranchCreate { name, start } | Action::TagCreate { name, start } => {
            let prefix = if matches!(action, Action::TagCreate { .. }) { "tags" } else { "heads" };
            let reference = refname(root, name, prefix).await?;
            let target = commit(root, start).await?;
            steps.push(step(vec![
                "update-ref".into(),
                "--no-deref".into(),
                reference.clone(),
                target.clone(),
                "0".repeat(target.len()),
            ]));
            preview = format!("Create {reference} at {target}. Existing refs are never overwritten.");
        }
        Action::BranchDelete { name } | Action::TagDelete { name } => {
            let branch = matches!(action, Action::BranchDelete { .. });
            let reference = refname(root, name, if branch { "heads" } else { "tags" }).await?;
            let old = oid(root, &reference).await?;
            if branch {
                anyhow::ensure!(
                    !super::super::worktrees::list(root).await?.iter().any(|row| row.branch.as_deref() == Some(name)),
                    "Branch is checked out in a worktree"
                );
                read(root, &["merge-base", "--is-ancestor", &old, "HEAD"])
                    .await
                    .context("Branch contains unmerged commits; deletion is refused")?;
            }
            steps.push(step(vec![
                "update-ref".into(),
                "--no-deref".into(),
                "-d".into(),
                reference.clone(),
                old.clone(),
            ]));
            preview = format!("Delete {reference} only if it still points at {old}. Branch configuration is retained.");
        }
        Action::BranchSwitch { name } => {
            let reference = refname(root, name, "heads").await?;
            let target = commit(root, &reference).await?;
            anyhow::ensure!(clean, "Switch requires a clean checkout; commit or stash changes first");
            steps.push(step(vec!["switch".into(), "--no-guess".into(), "--".into(), name.clone()]));
            preview =
                format!("Switch this clean checkout to {reference} ({target}). Configured checkout hooks may run.");
        }
        Action::RemoteAdd { name, url } => {
            field(name)?;
            validate_url(url)?;
            steps.push(step(vec!["remote".into(), "add".into(), name.clone(), url.clone()]));
            preview = format!("Add remote {name}: {url}. No network access is started by this action.");
        }
        Action::RemoteRemove { name } => {
            field(name)?;
            let (url, _) = state::remote_url(root, name).await?;
            steps.push(step(vec!["remote".into(), "remove".into(), name.clone()]));
            preview = format!("Remove remote {name} ({url}) and its remote-tracking refs. Local branches are kept.");
        }
        Action::Stage { path }
        | Action::Unstage { path }
        | Action::Hunk { path, .. }
        | Action::Restore { path }
        | Action::Discard { path } => {
            let file = validate_path(root, path).await?;
            let spec = format!(":(literal){path}");
            let row = rows.iter().find(|row| row.path == *path).context("File is no longer changed")?;
            anyhow::ensure!(
                row.old_path.is_none(),
                "Rename pairs require staging their add/delete paths separately with an external Git client"
            );
            match &action {
                Action::Stage { .. } => steps.push(step(vec!["add".into(), "-A".into(), "--".into(), spec.clone()])),
                Action::Unstage { .. } => {
                    steps.push(step(if before.head.is_some() {
                        vec!["restore".into(), "--staged".into(), "--source=HEAD".into(), "--".into(), spec.clone()]
                    } else {
                        vec!["rm".into(), "--cached".into(), "--ignore-unmatch".into(), "--".into(), spec.clone()]
                    }));
                }
                Action::Hunk { staged, hunk, .. } => {
                    let raw = super::patch(root, path, *staged).await?;
                    anyhow::ensure!(
                        raw.partial,
                        "Binary, mode, symlink, add/delete and non-UTF-8 patches require whole-file staging"
                    );
                    let selected = raw.hunks.get(*hunk).context("Hunk changed; select it again")?.as_bytes().to_vec();
                    let mut args = vec!["apply".into(), "--cached".into(), "--whitespace=nowarn".into()];
                    if *staged {
                        args.push("--reverse".into());
                    }
                    steps.push(Step { args, input: Some(selected) });
                    patch = Some(raw);
                }
                Action::Restore { .. } => {
                    anyhow::ensure!(!row.untracked, "Use Delete untracked file for an untracked path");
                    steps.push(step(vec!["restore".into(), "--worktree".into(), "--".into(), spec.clone()]));
                }
                Action::Discard { .. } => {
                    anyhow::ensure!(
                        row.untracked && tokio::fs::symlink_metadata(&file).await?.is_file(),
                        "Only an exact untracked regular file can be deleted"
                    );
                }
                _ => unreachable!(),
            }
            preview = format!(
                "{}: {path}\nOnly this exact file is targeted. Staging runs configured clean filters. Restore uses the index and never changes staged content. Deleting an untracked file is permanent.",
                action.label()
            );
            if let Action::Hunk { .. } = &action {
                if let Some(bytes) = steps.first().and_then(|step| step.input.as_ref()) {
                    preview.push_str("\n\nCanonical selected patch:\n");
                    preview.push_str(std::str::from_utf8(bytes)?);
                }
            } else if !row.untracked {
                let current = super::patch(root, path, matches!(action, Action::Unstage { .. })).await?;
                preview.push_str(
                    "\n\nCurrent diff (whole-file actions include all changes, including any truncated display):\n",
                );
                preview.push_str(&current.text);
                patch = Some(current);
            } else {
                preview.push_str(
                    "\nUntracked content is staged as a whole file. Inspect it in an editor before confirming.",
                );
            }
        }
        Action::Commit { message, amend } => {
            anyhow::ensure!(
                !message.trim().is_empty() && message.len() <= 64 * 1024 && !message.contains('\0'),
                "Enter a commit message up to 64 KiB"
            );
            if *amend {
                anyhow::ensure!(before.head.is_some(), "Cannot amend an unborn branch");
            } else {
                anyhow::ensure!(
                    rows.iter().any(|row| row.index != ' ' && row.index != '?'),
                    "No staged changes to commit"
                );
            }
            let mut args = vec!["commit".into(), "--file=-".into()];
            if *amend {
                args.push("--amend".into());
            }
            steps.push(Step { args, input: Some(message.as_bytes().to_vec()) });
            preview = format!(
                "{} on {}. All staged changes are included. Configured hooks and signing run; stdin prompts are unavailable. Failed/cancelled hooks/signers may leave changes; inspect the resulting state before retrying.\n\n{message}",
                action.label(),
                before.branch.as_deref().unwrap_or("detached HEAD")
            );
            preview.push_str("\n\nCurrent staged paths:\n");
            for row in rows.iter().filter(|row| row.index != ' ' && !row.untracked) {
                preview.push_str(&format!("{:?}\n", row.path));
            }
        }
        Action::Fetch { remote: name } | Action::Pull { remote: name, .. } | Action::Push { remote: name, .. } => {
            field(name)?;
            let (url, url_hash) = state::remote_url(root, name).await?;
            let push_urls = text(read(root, &["remote", "get-url", "--push", "--all", name]).await?)?;
            anyhow::ensure!(
                push_urls == url,
                "Configure one identical fetch/push URL before using desktop network operations"
            );
            let branch = match &action {
                Action::Pull { branch, .. } | Action::Push { branch, .. } => branch.clone(),
                _ => String::new(),
            };
            let expected = if branch.is_empty() {
                None
            } else {
                refname(root, &branch, "heads").await?;
                remote_head(root, name, &branch).await?
            };
            match &action {
                Action::Fetch { .. } => steps.push(step(vec![
                    "fetch".into(),
                    "--no-all".into(),
                    "--no-tags".into(),
                    "--no-prune".into(),
                    "--no-prune-tags".into(),
                    "--recurse-submodules=no".into(),
                    "--refmap=".into(),
                    "--".into(),
                    name.clone(),
                    format!("+refs/heads/*:refs/remotes/{name}/*"),
                ])),
                Action::Pull { .. } => {
                    anyhow::ensure!(
                        clean && before.branch.is_some(),
                        "Fast-forward pull requires a clean checkout on a local branch"
                    );
                    let target = expected.clone().context("Remote branch does not exist")?;
                    steps.push(step(vec![
                        "fetch".into(),
                        "--no-all".into(),
                        "--no-prune".into(),
                        "--no-prune-tags".into(),
                        "--recurse-submodules=no".into(),
                        "--refmap=".into(),
                        "--no-tags".into(),
                        "--".into(),
                        name.clone(),
                        format!("refs/heads/{branch}:refs/remotes/{name}/{branch}"),
                    ]));
                    steps.push(step(vec!["merge".into(), "--ff-only".into(), "--no-edit".into(), target]));
                }
                Action::Push { force, .. } => {
                    let head = before.head.clone().context("No commit to push")?;
                    if !*force && let Some(old) = &expected {
                        read(root,&["merge-base","--is-ancestor",old,&head]).await.context("Normal push requires fetched remote history and a fast-forward; fetch or explicitly preview a leased push")?;
                    }
                    steps.push(step(vec![
                        "-c".into(),
                        format!("remote.{name}.mirror=false"),
                        "push".into(),
                        "--no-follow-tags".into(),
                        "--recurse-submodules=no".into(),
                        "--porcelain".into(),
                        format!("--force-with-lease=refs/heads/{branch}:{}", expected.as_deref().unwrap_or("")),
                        "--".into(),
                        name.clone(),
                        format!("{head}:refs/heads/{branch}"),
                    ]));
                }
                _ => unreachable!(),
            }
            preview = format!(
                "{} via {name}: {url}\nRemote branch: {branch}\nRecorded remote head: {}\nUses installed Git credentials/helpers and hooks, without persisting application tokens. No terminal stdin is available. Fetch writes only refs/remotes/{name}/* without pruning; push targets only this branch, without automatic tag/submodule pushes or mirroring. Network interruption can leave an unknown outcome.",
                action.label(),
                expected.as_deref().unwrap_or("absent / not applicable")
            );
            remote =
                Some(RemoteProof { remote: name.clone(), branch, url_hash, expected, intended: before.head.clone() });
        }
        Action::WorktreeCreate { path, branch } => {
            refname(root, branch, "heads").await?;
            commit(root, &format!("refs/heads/{branch}")).await?;
            let destination = destination(path, &mut extra).await?;
            anyhow::ensure!(
                !destination.starts_with(&before.identity.git) && !destination.starts_with(&before.identity.common),
                "Choose a worktree destination outside Git metadata"
            );
            reserved_destination = Some(destination.clone());
            let path = destination.to_str().context("Worktree paths require UTF-8")?;
            steps.push(step(vec!["worktree".into(), "add".into(), "--".into(), path.into(), branch.clone()]));
            preview = format!(
                "Create {path} from existing branch {branch}. A new destination folder is reserved exclusively. Failed/partial checkouts are kept for inspection; no automatic deletion."
            );
        }
        Action::WorktreeLock { path, .. } | Action::WorktreeUnlock { path } | Action::WorktreeRemove { path } => {
            anyhow::ensure!(Path::new(path).is_absolute(), "Choose an absolute registered worktree path");
            let path = tokio::fs::canonicalize(path).await?;
            let row = super::super::worktrees::list(root)
                .await?
                .into_iter()
                .find(|row| row.path == path)
                .context("Worktree is no longer registered")?;
            anyhow::ensure!(
                path != root && !row.bare && row.prunable.is_none(),
                "Select another available linked checkout"
            );
            let selected_identity = state::identity(&path).await?;
            anyhow::ensure!(
                selected_identity.common == before.identity.common,
                "Selected worktree now belongs to another repository"
            );
            for metadata in [path.join(".git"), selected_identity.git.join("index"), selected_identity.git.join("HEAD")]
            {
                extra.push((metadata.clone(), state::path_fingerprint(&metadata).await?));
            }
            extra.push((path.clone(), state::path_fingerprint(&path).await?));
            let target = path.to_str().context("Worktree paths require UTF-8")?.to_owned();
            let args = match &action {
                Action::WorktreeLock { reason, .. } => {
                    anyhow::ensure!(reason.len() <= 1024 && !reason.contains('\0'), "Lock reason exceeds its limit");
                    vec![
                        "worktree".into(),
                        "lock".into(),
                        "--reason".into(),
                        reason.clone(),
                        "--".into(),
                        target.clone(),
                    ]
                }
                Action::WorktreeUnlock { .. } => vec!["worktree".into(), "unlock".into(), "--".into(), target.clone()],
                Action::WorktreeRemove { .. } => {
                    anyhow::ensure!(
                        read(&path, &["ls-files", "--others", "--ignored", "--exclude-standard", "-z"])
                            .await?
                            .is_empty(),
                        "Refusing to remove a checkout containing ignored files"
                    );
                    let entries = text(read(&path, &["ls-files", "--stage"]).await?)?;
                    anyhow::ensure!(
                        !entries.lines().any(|line| line.starts_with("160000 ")),
                        "Worktrees with submodules require external removal"
                    );
                    anyhow::ensure!(
                        row.locked.is_none() && changes(&path).await?.is_empty(),
                        "Refusing to remove a locked or dirty checkout"
                    );
                    vec!["worktree".into(), "remove".into(), "--".into(), target.clone()]
                }
                _ => unreachable!(),
            };
            steps.push(step(args));
            preview = format!(
                "{}: {target}. Removal never uses --force and must pass native in-use/managed-checkout checks.",
                action.label()
            );
        }
        Action::WorktreePrune => {
            steps.push(step(vec!["worktree".into(), "prune".into(), "--expire=now".into()]));
            let entries = super::super::worktrees::list(root).await?;
            let missing = entries
                .iter()
                .filter(|row| row.prunable.is_some() && row.locked.is_none())
                .map(|row| row.path.to_str().context("Non-UTF-8 registrations require external pruning"))
                .collect::<Result<Vec<_>>>()?;
            preview = format!(
                "Prune these missing, unlocked registrations (existing folders are not removed):\n{}",
                missing.join("\n")
            );
        }
        Action::StashSave { message, untracked } => {
            anyhow::ensure!(!clean, "No changes to stash");
            anyhow::ensure!(message.len() <= 4096 && !message.contains('\0'), "Invalid stash message");
            let mut args = vec!["stash".into(), "push".into(), "--message".into(), message.clone()];
            if *untracked {
                args.push("--include-untracked".into());
            }
            steps.push(step(args));
            preview = format!(
                "Stash tracked changes{} and clear them from this checkout. Ignored files are not included. Stash message: {message}",
                if *untracked { " and untracked files" } else { "" }
            );
        }
        Action::StashApply { oid: target } | Action::StashPop { oid: target } | Action::StashDrop { oid: target } => {
            anyhow::ensure!(
                matches!(target.len(), 40 | 64) && target.bytes().all(|b| b.is_ascii_hexdigit()),
                "Choose an exact stash object"
            );
            let entries = text(read(root, &["reflog", "show", "--format=%H", "refs/stash"]).await?)?;
            anyhow::ensure!(entries.lines().any(|oid| oid == target), "Stash is no longer in the reflog");
            if !matches!(action, Action::StashDrop { .. }) {
                anyhow::ensure!(clean, "Stash apply/pop requires a clean checkout");
                steps.push(step(vec!["stash".into(), "apply".into(), "--index".into(), target.clone()]));
            }
            if !matches!(action, Action::StashApply { .. }) {
                anyhow::ensure!(
                    entries.lines().next() == Some(target),
                    "Pop/drop currently supports the latest stash only; apply older stashes without dropping them"
                );
                steps.push(step(vec!["stash".into(), "drop".into(), "stash@{0}".into()]));
            }
            preview = format!(
                "{} {target}. Apply/pop restores the index. Conflicts keep the stash and require external Git recovery; a failed apply never proceeds to drop. Latest-stash drop is permanent.",
                action.label()
            );
        }
    }
    if !matches!(action, Action::Discard { .. }) {
        preview.push_str("\n\nConfirmed writes may run your configured Git hooks and filters. Interactive terminal input requires an external client.");
    }
    anyhow::ensure!(preview.len() <= 96 * 1024, "Operation preview exceeds its limit");
    Ok(Plan { before, action, steps, preview, patch, remote, extra, destination: reserved_destination })
}
