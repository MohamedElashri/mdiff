//! Explicitly owned, noninteractive Git write processes. Unlike read helpers, dropping
//! a renderer request never owns this future. Cancellation reaps the whole process group.
use anyhow::{Context, Result};
use std::{path::Path, process::Stdio, time::Duration};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    sync::watch,
};
#[derive(Debug)]
pub struct Exit {
    pub code: Option<i32>,
    pub interrupted: bool,
}
pub async fn run(
    root: &Path,
    args: &[String],
    input: Option<Vec<u8>>,
    index: Option<&Path>,
    cancel: watch::Receiver<bool>,
    network: bool,
) -> Result<Exit> {
    run_with_editor(root, args, input, index, cancel, network, None).await
}
pub async fn run_with_editor(
    root: &Path,
    args: &[String],
    input: Option<Vec<u8>>,
    index: Option<&Path>,
    cancel: watch::Receiver<bool>,
    network: bool,
    sequence_editor: Option<&Path>,
) -> Result<Exit> {
    let _interactive_slot = if network {
        None
    } else {
        Some(
            tokio::select! {slot=crate::scheduler::acquire()=>slot,_=cancelled(cancel.clone())=>anyhow::bail!("Cancelled before Git started")},
        )
    };
    let mut command = tokio::process::Command::new("git");
    command
        .args(args)
        .current_dir(root)
        .stdin(if input.is_some() { Stdio::piped() } else { Stdio::null() })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_EDITOR", "true")
        .env("GIT_SEQUENCE_EDITOR", "true");
    for key in ["GIT_LITERAL_PATHSPECS", "GIT_GLOB_PATHSPECS", "GIT_NOGLOB_PATHSPECS", "GIT_ICASE_PATHSPECS"] {
        command.env_remove(key);
    }
    for key in ["GIT_DIR", "GIT_WORK_TREE", "GIT_INDEX_FILE", "GIT_COMMON_DIR", "GIT_NAMESPACE", "GIT_PREFIX"] {
        command.env_remove(key);
    }
    for key in [
        "GIT_TRACE",
        "GIT_TRACE_CURL",
        "GIT_CURL_VERBOSE",
        "GIT_TRACE_PACKET",
        "GIT_TRACE2",
        "GIT_TRACE2_EVENT",
        "GIT_TRACE2_PERF",
    ] {
        command.env(key, "0");
    }
    if let Some(editor) = sequence_editor {
        let editor = editor.to_str().context("Sequence editor path requires UTF-8")?;
        command.env("GIT_SEQUENCE_EDITOR", format!("'{}'", editor.replace('\'', "'\"'\"'")));
    }
    if let Some(index) = index {
        command.env("GIT_INDEX_FILE", index);
    }
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.as_std_mut().process_group(0);
    }
    anyhow::ensure!(!*cancel.borrow(), "Operation cancelled before Git started");
    let mut child = command.spawn().context("Could not start Git")?;
    let pid = child.id();
    // Drain without retaining hook/credential output: it may contain secrets. A noisy hook
    // cannot allocate unbounded memory or block Git on a full stdout/stderr pipe.
    let stdout = child.stdout.take().unwrap();
    let stderr = child.stderr.take().unwrap();
    let mut drains = tokio::spawn(async move {
        async fn drain(mut stream: impl tokio::io::AsyncRead + Unpin) {
            let mut buffer = vec![0u8; 8192];
            while let Ok(n) = stream.read(&mut buffer).await {
                if n == 0 {
                    break;
                }
            }
        }
        tokio::join!(drain(stdout), drain(stderr));
    });
    let stdin = child.stdin.take();
    let feed = tokio::spawn(async move {
        if let (Some(mut stdin), Some(bytes)) = (stdin, input) {
            let _ = stdin.write_all(&bytes).await;
        }
    });
    let waited = tokio::select! {
        status=child.wait()=>Some(status?),
        _=cancelled(cancel.clone())=>None,
        _=tokio::time::sleep(Duration::from_secs(if network {300} else {120}))=>None,
    };
    let interrupted = waited.is_none();
    let status = if let Some(status) = waited {
        status
    } else {
        #[cfg(unix)]
        if let Some(pid) = pid {
            // This is our separately created group, never the user's terminal group.
            unsafe {
                libc::kill(-(pid as i32), libc::SIGTERM);
            }
        }
        // Keep the child handle unreaped during the grace period, then kill the
        // entire owned group even when Git exited before a TERM-ignoring hook.
        tokio::time::sleep(Duration::from_millis(250)).await;
        #[cfg(unix)]
        if let Some(pid) = pid {
            unsafe {
                libc::kill(-(pid as i32), libc::SIGKILL);
            }
        }
        let _ = child.start_kill();
        child.wait().await?
    };
    // A background hook must not keep pipes/the job alive indefinitely after Git exits.
    if tokio::time::timeout(Duration::from_secs(1), &mut drains).await.is_err() {
        drains.abort();
    }
    feed.abort();
    Ok(Exit { code: status.code(), interrupted })
}

/// Losing the UI/controller is not a cancellation request.
pub async fn cancelled(mut cancel: watch::Receiver<bool>) {
    loop {
        if *cancel.borrow_and_update() {
            return;
        }
        if cancel.changed().await.is_err() {
            std::future::pending::<()>().await;
        }
    }
}
