use super::*;
use anyhow::{Context, Result};
use std::path::{Component, Path, PathBuf};
pub async fn identity(root: &Path) -> Result<Identity> {
    let caller = tokio::fs::canonicalize(root).await?;
    let mut paths = vec![];
    for flag in ["--show-toplevel", "--absolute-git-dir", "--git-common-dir"] {
        let bytes = read(&caller, &["rev-parse", flag]).await?;
        let path = super::super::path_bytes(bytes.strip_suffix(b"\n").unwrap_or(&bytes))?;
        paths.push(tokio::fs::canonicalize(caller.join(path)).await?);
    }
    let mut inodes = vec![];
    for path in &paths {
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            let meta = tokio::fs::metadata(path).await?;
            inodes.push((meta.dev(), meta.ino()));
        }
        #[cfg(not(unix))]
        {
            let _ = path;
            inodes.push((0, 0));
        }
    }
    Ok(Identity { root: paths.remove(0), git: paths.remove(0), common: paths.remove(0), inodes })
}
pub async fn path_fingerprint(path: &Path) -> Result<String> {
    use sha2::{Digest, Sha256};
    use tokio::io::AsyncReadExt;
    let meta = match tokio::fs::symlink_metadata(path).await {
        Ok(meta) => meta,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok("missing".into()),
        Err(error) => return Err(error.into()),
    };
    let mut digest = Sha256::new();
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        digest.update(
            format!("{}:{}:{}:{}", meta.dev(), meta.ino(), meta.mode(), if meta.is_dir() { 0 } else { meta.len() })
                .as_bytes(),
        );
    }
    if meta.is_symlink() {
        digest.update(tokio::fs::read_link(path).await?.as_os_str().as_encoded_bytes());
    } else if meta.is_file() {
        anyhow::ensure!(
            meta.len() <= 64 * 1024 * 1024,
            "Selected file/index exceeds the 64 MiB safety limit; use an external Git client"
        );
        let mut file = tokio::fs::File::open(path).await?;
        let mut buffer = vec![0u8; 65536];
        let mut total = 0;
        loop {
            let count = file.read(&mut buffer).await?;
            if count == 0 {
                break;
            }
            total += count;
            anyhow::ensure!(total <= 64 * 1024 * 1024, "Selected file grew beyond its preview limit");
            digest.update(&buffer[..count]);
        }
    }
    Ok(hex::encode(digest.finalize()))
}
pub async fn validate_path(root: &Path, path: &str) -> Result<PathBuf> {
    let root = tokio::fs::canonicalize(root).await?;
    anyhow::ensure!(!path.is_empty() && path.len() <= 4096 && !path.contains('\0'), "Invalid file path");
    let relative = Path::new(path);
    anyhow::ensure!(
        relative.components().all(|part| matches!(part,Component::Normal(name) if !name.eq_ignore_ascii_case(".git"))),
        "Choose an exact repository file, not a directory or traversal path"
    );
    let file = root.join(relative);
    let parent = tokio::fs::canonicalize(file.parent().context("Missing file parent")?).await?;
    anyhow::ensure!(parent.starts_with(&root), "File parent points outside this checkout");
    if let Ok(meta) = tokio::fs::symlink_metadata(&file).await {
        anyhow::ensure!(!meta.is_dir(), "Select a file; recursive directory writes are not supported");
    }
    Ok(file)
}
async fn status_bytes(root: &Path) -> Result<Vec<u8>> {
    read(
        root,
        &[
            "-c",
            "core.fsmonitor=false",
            "-c",
            "core.hooksPath=/dev/null",
            "status",
            "--porcelain=v1",
            "--no-renames",
            "-z",
            "--untracked-files=all",
            "--ignore-submodules=all",
        ],
    )
    .await
}
pub async fn changes(root: &Path) -> Result<Vec<Change>> {
    parse_changes(&status_bytes(root).await?)
}
fn parse_changes(bytes: &[u8]) -> Result<Vec<Change>> {
    let mut records = bytes.split(|b| *b == 0).filter(|r| !r.is_empty());
    let mut rows = vec![];
    while let Some(record) = records.next() {
        anyhow::ensure!(record.len() >= 4 && record[2] == b' ', "Malformed Git status");
        let index = record[0] as char;
        let worktree = record[1] as char;
        let path =
            String::from_utf8(record[3..].to_vec()).context("Non-UTF-8 file paths require an external Git client")?;
        let old_path = if index == 'R' || index == 'C' || worktree == 'R' || worktree == 'C' {
            Some(String::from_utf8(records.next().context("Truncated rename")?.to_vec())?)
        } else {
            None
        };
        rows.push(Change { path, old_path, index, worktree, untracked: index == '?' });
        anyhow::ensure!(rows.len() <= 2000, "More than 2,000 changed paths; narrow changes in an external Git client");
    }
    Ok(rows)
}
pub async fn inspect(root: &Path, action: &Action) -> Result<Snapshot> {
    Ok(inspect_parts(root, action).await?.0)
}
pub(crate) async fn inspect_parts(root: &Path, action: &Action) -> Result<(Snapshot, Vec<Change>)> {
    let identity = identity(root).await?;
    let root = &identity.root;
    let head = text(read(root, &["rev-parse", "--verify", "--quiet", "HEAD"]).await.unwrap_or_default())?;
    let branch = text(read(root, &["symbolic-ref", "--quiet", "HEAD"]).await.unwrap_or_default())?;
    let mut operations = vec![];
    for name in
        ["MERGE_HEAD", "CHERRY_PICK_HEAD", "REVERT_HEAD", "rebase-apply", "rebase-merge", "sequencer", "BISECT_LOG"]
    {
        if identity.git.join(name).exists() {
            operations.push(name.to_string());
        }
    }
    let operation_fingerprint = super::advanced::metadata_fingerprint(&identity.git).await?;
    let bytes = status_bytes(root).await?;
    let conflicts = bytes
        .split(|b| *b == 0)
        .any(|r| r.len() > 2 && (r[0] == b'U' || r[1] == b'U' || r.starts_with(b"AA ") || r.starts_with(b"DD ")));
    let refs = read(
        root,
        &[
            "for-each-ref",
            "--format=%(refname) %(objectname)",
            "refs/heads/",
            "refs/tags/",
            "refs/remotes/",
            "refs/stash",
        ],
    )
    .await?;
    let config = read(root, &["config", "--null", "--list"]).await?;
    let mut fingerprint = serde_json::to_vec(&(&identity, &head, &branch, &operations))?;
    fingerprint.extend_from_slice(operation_fingerprint.as_bytes());
    for data in [&bytes, &refs, &config] {
        fingerprint.extend_from_slice(hash(data).as_bytes());
    }
    fingerprint.extend_from_slice(path_fingerprint(&identity.git.join("index")).await?.as_bytes());
    let rows = parse_changes(&bytes)?;
    if let Some(path) = action.path() {
        fingerprint.extend_from_slice(path_fingerprint(&validate_path(root, path).await?).await?.as_bytes());
    } else if matches!(action, Action::StashSave { .. } | Action::Recover { .. }) {
        let mut total = 0u64;
        for row in &rows {
            let path = validate_path(root, &row.path).await?;
            total += tokio::fs::symlink_metadata(&path).await.map(|m| m.len()).unwrap_or(0);
            anyhow::ensure!(total <= 64 * 1024 * 1024, "Stash preview exceeds 64 MiB; use an external Git client");
            fingerprint.extend_from_slice(path_fingerprint(&path).await?.as_bytes());
        }
    }
    if matches!(
        action,
        Action::WorktreeCreate { .. }
            | Action::WorktreeLock { .. }
            | Action::WorktreeUnlock { .. }
            | Action::WorktreeRemove { .. }
            | Action::WorktreePrune
            | Action::BranchDelete { .. }
    ) {
        fingerprint.extend(read(root, &["worktree", "list", "--porcelain", "-z"]).await?);
    }
    let stash_fingerprint = if matches!(
        action,
        Action::StashSave { .. } | Action::StashApply { .. } | Action::StashPop { .. } | Action::StashDrop { .. }
    ) {
        let has_stash = refs.split(|b| *b == b'\n').any(|line| line.starts_with(b"refs/stash "));
        let reflog = if has_stash {
            read(root, &["reflog", "show", "--format=%H%x00%gs%x00", "refs/stash"]).await?
        } else {
            vec![]
        };
        fingerprint.extend_from_slice(&reflog);
        hash(&reflog)
    } else {
        String::new()
    };
    Ok((
        Snapshot {
            identity,
            stash_fingerprint,
            fingerprint: hash(&fingerprint),
            head: (!head.is_empty()).then_some(head),
            branch: (!branch.is_empty()).then_some(branch),
            operations,
            conflicts,
        },
        rows,
    ))
}
pub async fn patch(root: &Path, path: &str, staged: bool) -> Result<Patch> {
    validate_path(root, path).await?;
    let mut args = vec![
        "diff",
        "--no-ext-diff",
        "--no-textconv",
        "--no-renames",
        "--full-index",
        "--no-color",
        "--src-prefix=a/",
        "--dst-prefix=b/",
    ];
    if staged {
        args.push("--cached");
    }
    args.extend(["--", path]);
    let bytes = read(root, &args).await?;
    let fingerprint = hash(&bytes);
    let Ok(text) = String::from_utf8(bytes) else {
        return Ok(Patch {
            fingerprint,
            text: "Non-UTF-8 patch: use whole-file staging or an external Git client.".into(),
            hunks: vec![],
            partial: false,
        });
    };
    let attributes = read(root, &["check-attr", "-z", "filter", "--", path]).await?;
    let filter = attributes.split(|b| *b == 0).nth(2).unwrap_or(b"unspecified");
    let filtered = filter != b"unspecified" && filter != b"unset";
    let partial = !filtered
        && text.len() <= 256 * 1024
        && !text.contains("GIT binary patch")
        && !text.lines().any(|line| line.starts_with("Binary files "))
        && !text.contains("new file mode")
        && !text.contains("deleted file mode")
        && !text.contains("old mode")
        && !text.contains("160000")
        && !text.contains("120000");
    let mut header = String::new();
    let mut hunks: Vec<String> = vec![];
    for line in text.split_inclusive('\n') {
        if line.starts_with("@@ ") {
            hunks.push(String::new());
        }
        if let Some(hunk) = hunks.last_mut() {
            hunk.push_str(line);
        } else {
            header.push_str(line);
        }
    }
    let partial = partial && hunks.len() <= 50;
    let hunks = if partial { hunks.into_iter().map(|h| format!("{header}{h}")).collect() } else { vec![] };
    let text = if text.len() > 64 * 1024 {
        let mut end = 64 * 1024;
        while !text.is_char_boundary(end) {
            end -= 1;
        }
        format!(
            "{}\n… Display truncated; use whole-file staging or an external editor for the complete diff.",
            &text[..end]
        )
    } else {
        text
    };
    Ok(Patch { text, hunks, partial, fingerprint })
}
pub async fn remote_url(root: &Path, remote: &str) -> Result<(String, String)> {
    let url = text(read(root, &["remote", "get-url", remote]).await?)?;
    super::validate_url(&url)?;
    let digest = hash(url.as_bytes());
    Ok((url, digest))
}
pub async fn remote_head(root: &Path, remote: &str, branch: &str) -> Result<Option<String>> {
    let reference = format!("refs/heads/{branch}");
    let output = text(read(root, &["ls-remote", "--refs", remote, &reference]).await?)?;
    if output.is_empty() {
        return Ok(None);
    }
    let (oid, actual) = output.split_once('\t').context("Malformed remote ref")?;
    anyhow::ensure!(
        actual == reference && oid.bytes().all(|b| b.is_ascii_hexdigit()) && matches!(oid.len(), 40 | 64),
        "Ambiguous remote ref; inspect externally"
    );
    Ok(Some(oid.into()))
}
