use super::*;
use std::{
    io::{BufRead, Write},
    os::unix::{fs::PermissionsExt, net::UnixListener},
    time::Duration,
};
#[derive(Serialize)]
pub struct Bundle {
    pub helper: PathBuf,
    pub claude: PathBuf,
    pub codex: PathBuf,
    pub socket: PathBuf,
}
pub fn bundle(app: &tauri::AppHandle) -> Result<Bundle> {
    let root = app.path().app_config_dir()?.join("agent-hooks-v1");
    Ok(Bundle {
        helper: root.join("metadata.py"),
        claude: root.join("claude-settings.json"),
        codex: root.join("codex-hooks.json"),
        socket: socket(app),
    })
}
fn socket(app: &tauri::AppHandle) -> PathBuf {
    std::env::temp_dir().join(format!(
        "mdiff-agents-{}-{}.sock",
        unsafe { libc::getuid() },
        &crate::storage::digest(app.config().identifier.as_str())[..12]
    ))
}
fn quote(text: &str) -> String {
    format!("'{}'", text.replace('\'', "'\\''"))
}
pub async fn install(app: &tauri::AppHandle) -> Result<()> {
    let python = std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default())
        .map(|p| p.join("python3"))
        .find(|p| {
            p.is_file()
                && p.metadata()
                    .is_ok_and(|m| m.permissions().mode() & 0o111 != 0)
        })
        .context("Install Python 3 before generating local state hooks")?
        .canonicalize()?;
    let b = bundle(app)?;
    let root = b.helper.parent().unwrap();
    tokio::fs::create_dir_all(root).await?;
    tokio::fs::set_permissions(root, std::fs::Permissions::from_mode(0o700)).await?;
    diffle::persist::write_file_atomic(&b.helper, include_str!("hook.py")).await?;
    for (tool, path) in [("claude", &b.claude), ("codex", &b.codex)] {
        let command = format!(
            "exec {} {} {} {}",
            quote(python.to_str().context("Non UTF-8 Python path")?),
            quote(b.helper.to_str().context("Non UTF-8 hook path")?),
            quote(b.socket.to_str().unwrap()),
            tool
        );
        let events = [
            "SessionStart",
            "UserPromptSubmit",
            "PreToolUse",
            "PermissionRequest",
            "Notification",
            "Stop",
            "SessionEnd",
            "SubagentStart",
            "SubagentStop",
        ];
        let hooks=events.into_iter().filter(|name|tool=="claude" || *name!="Notification").map(|name|(name.to_owned(),serde_json::json!([{"hooks":[{"type":"command","command":command,"timeout":1}]}]))).collect::<serde_json::Map<_,_>>();
        diffle::persist::write_file_atomic(
            path,
            &serde_json::to_string_pretty(&serde_json::json!({"hooks":hooks}))?,
        )
        .await?;
    }
    Ok(())
}
pub async fn remove(app: &tauri::AppHandle) -> Result<()> {
    let b = bundle(app)?;
    if b.helper.exists() {
        diffle::persist::write_file_atomic(
            &b.helper,
            "# mdiff hook delivery disabled; inert helper for existing command references.\n",
        )
        .await?;
    }
    for p in [b.claude, b.codex] {
        match tokio::fs::remove_file(p).await {
            Ok(()) => (),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => (),
            Err(e) => return Err(e.into()),
        }
    }
    Ok(())
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Event {
    tool: String,
    sent: u64,
    hook_event_name: String,
    session_id: String,
    cwd: PathBuf,
    agent_id: Option<String>,
    notification_type: Option<String>,
}
fn label(event: &Event) -> Result<&'static str> {
    Ok(match event.hook_event_name.as_str() {
        "SessionStart" => "started",
        "UserPromptSubmit" | "PreToolUse" => "working",
        "PermissionRequest" => "waiting",
        "Notification"
            if [Some("permission_prompt"), Some("idle_prompt")]
                .contains(&event.notification_type.as_deref()) =>
        {
            "waiting"
        }
        "Notification" => "unknown",
        "Stop" | "SubagentStop" => "turn completed",
        "SessionEnd" => "ended",
        "SubagentStart" => "working",
        _ => anyhow::bail!("Unsupported hook event"),
    })
}
fn monotonic() -> u64 {
    let mut t = std::mem::MaybeUninit::<libc::timespec>::zeroed();
    unsafe {
        libc::clock_gettime(libc::CLOCK_MONOTONIC, t.as_mut_ptr());
        let t = t.assume_init();
        t.tv_sec as u64 * 1_000_000_000 + t.tv_nsec as u64
    }
}
fn apply(app: &tauri::AppHandle, peer: i32, event: Event) -> Result<()> {
    anyhow::ensure!(
        event.session_id.len() <= 128
            && !event.session_id.is_empty()
            && event
                .agent_id
                .as_ref()
                .is_none_or(|s| !s.is_empty() && s.len() <= 128),
        "Invalid hook identity"
    );
    anyhow::ensure!(
        monotonic().abs_diff(event.sent) <= 2_000_000_000,
        "Stale local hook event"
    );
    let s = app.state::<Agents>();
    let (generation, tools) = {
        let d = s.data.lock().unwrap();
        anyhow::ensure!(
            d.preferences.enabled && d.preferences.hooks,
            "Agent hooks disabled"
        );
        (d.generation, preferences::tools(&d.preferences))
    };
    let (p, tool) = process::ancestor(peer, &tools)?;
    anyhow::ensure!(
        tool == event.tool && event.cwd.is_absolute(),
        "Hook tool mismatch"
    );
    let cwd = event.cwd.canonicalize()?;
    anyhow::ensure!(cwd == p.cwd.canonicalize()?, "Hook checkout changed");
    let status = label(&event)?;
    let mut d = s.data.lock().unwrap();
    anyhow::ensure!(d.generation == generation, "Hook configuration changed");
    let watch = d
        .watches
        .iter()
        .filter(|w| cwd.starts_with(&w.root))
        .max_by_key(|w| w.root.components().count())
        .cloned()
        .context("Unwatched hook checkout")?;
    let base = crate::storage::digest(&format!("{}:{}:{}", p.pid, p.start, watch.context));
    let session_base = crate::storage::digest(&format!("{base}:{}", event.session_id));
    let id = if let Some(agent) = &event.agent_id {
        crate::storage::digest(&format!("{session_base}:{agent}"))
    } else {
        session_base.clone()
    };
    let launch = d
        .rows
        .iter()
        .find(|a| {
            a.pid == p.pid
                && a.start == p.start
                && a.context == watch.context
                && a.provenance == "launch"
        })
        .cloned();
    d.rows.retain(|a| {
        !(a.pid == p.pid
            && a.start == p.start
            && a.context == watch.context
            && ["detected", "launch"].contains(&a.provenance.as_str()))
    });
    if let Some(old) = d.rows.iter_mut().find(|a| a.id == id) {
        anyhow::ensure!(event.sent > old.sequence, "Out-of-order hook event");
        old.sequence = event.sent;
        old.state = status.into();
        old.provenance = "hook".into();
        old.session = Some(event.session_id);
        old.seen = now();
        old.reported = Some(now());
    } else {
        anyhow::ensure!(d.rows.len() < 256, "Agent activity limit reached");
        d.rows.push(Activity {
            id,
            tool,
            session: Some(event.session_id),
            parent: event.agent_id.map(|_| session_base),
            root: watch.root,
            context: watch.context,
            state: status.into(),
            provenance: "hook".into(),
            started: now(),
            seen: now(),
            reported: Some(now()),
            sequence: event.sent,
            pid: p.pid,
            start: p.start,
            branch: None,
            commit: launch.as_ref().and_then(|a| a.commit.clone()),
            terminal: launch.as_ref().and_then(|a| a.terminal.clone()),
        });
    }
    drop(d);
    Agents::changed(app);
    Ok(())
}
pub fn serve(app: tauri::AppHandle) {
    std::thread::spawn(move || {
        let path = socket(&app);
        // Never unlink a live listener belonging to another instance.
        if std::os::unix::net::UnixStream::connect(&path).is_ok() {
            return;
        }
        if let Ok(metadata) = std::fs::symlink_metadata(&path) {
            use std::os::unix::fs::{FileTypeExt, MetadataExt};
            if !metadata.file_type().is_socket() || metadata.uid() != unsafe { libc::getuid() } {
                return;
            }
            if std::fs::remove_file(&path).is_err() {
                return;
            }
        }
        let Ok(listener) = UnixListener::bind(&path) else {
            return;
        };
        let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else {
                break;
            };
            let _ = stream.set_read_timeout(Some(Duration::from_millis(750)));
            let _ = stream.set_write_timeout(Some(Duration::from_millis(100)));
            let peer = process::peer_pid(&stream);
            let mut bytes = vec![];
            let read = std::io::BufReader::new(std::io::Read::take(&mut stream, 8193))
                .read_until(b'\n', &mut bytes);
            if read.is_ok() && bytes.len() <= 8192 {
                if let (Ok(peer), Ok(event)) = (peer, serde_json::from_slice::<Event>(&bytes)) {
                    let _ = apply(&app, peer, event);
                }
            }
            let _ = stream.write_all(b".");
        }
    });
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn hook_states_are_reports_not_process_ownership_or_permission_decisions() {
        let mut event: Event = serde_json::from_str(
            r#"{"sent":0,"tool":"claude","hook_event_name":"Stop","session_id":"session","cwd":"/repo"}"#,
        )
        .unwrap();
        assert_eq!(label(&event).unwrap(), "turn completed");
        event.hook_event_name = "PermissionRequest".into();
        assert_eq!(label(&event).unwrap(), "waiting");
        assert!(serde_json::from_str::<Event>(r#"{"sent":0,"tool":"codex","hook_event_name":"Stop","session_id":"s","cwd":"/repo","prompt":"private"}"#).is_err());
        assert!(quote("a ' b").contains("'\\''"));
    }
}

#[cfg(test)]
mod bridge_tests {
    use super::*;
    use std::{
        io::{BufRead, Write},
        process::{Command, Stdio},
    };
    #[test]
    fn helper_forwards_only_bounded_metadata_and_never_returns_a_permission_decision() {
        let temp = tempfile::tempdir().unwrap();
        let helper = temp.path().join("metadata 'quoted'.py");
        std::fs::write(&helper, include_str!("hook.py")).unwrap();
        let socket = temp.path().join("test.sock");
        let listener = UnixListener::bind(&socket).unwrap();
        listener.set_nonblocking(true).unwrap();
        let mut child = Command::new("python3")
            .arg(&helper)
            .arg(&socket)
            .arg("codex")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        child.stdin.take().unwrap().write_all(br#"{"hook_event_name":"PermissionRequest","session_id":"s","cwd":"/repo","transcript_path":"NEVER_READ","tool_input":{"command":"SECRET"},"last_assistant_message":"SECRET"}"#).unwrap();
        let mut peer = None;
        for _ in 0..100 {
            if let Ok((stream, _)) = listener.accept() {
                peer = Some(stream);
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        let mut peer = peer.expect("metadata helper did not connect");
        assert_eq!(process::peer_pid(&peer).unwrap(), child.id() as i32);
        peer.set_read_timeout(Some(Duration::from_secs(1))).unwrap();
        let mut line = String::new();
        std::io::BufReader::new(&peer).read_line(&mut line).unwrap();
        let value: serde_json::Value = serde_json::from_str(&line).unwrap();
        assert!(value.get("transcript_path").is_none());
        assert!(!line.contains("SECRET"));
        assert_eq!(value["tool"], "codex");
        assert!(value["sent"].as_u64().unwrap().abs_diff(monotonic()) < 2_000_000_000);
        peer.write_all(b".").unwrap();
        let output = child.wait_with_output().unwrap();
        assert!(output.status.success());
        assert!(output.stdout.is_empty());
        assert!(output.stderr.is_empty());
    }
    #[test]
    fn malformed_and_oversized_hook_payloads_fail_open_without_connecting() {
        let temp = tempfile::tempdir().unwrap();
        let helper = temp.path().join("hook.py");
        std::fs::write(&helper, include_str!("hook.py")).unwrap();
        for payload in [b"broken json".to_vec(), vec![b'x'; 65537]] {
            let mut child = Command::new("python3")
                .arg(&helper)
                .arg(temp.path().join("absent.sock"))
                .arg("codex")
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .spawn()
                .unwrap();
            child.stdin.take().unwrap().write_all(&payload).unwrap();
            let output = child.wait_with_output().unwrap();
            assert!(output.status.success());
            assert!(output.stdout.is_empty());
        }
    }
}

pub(super) async fn install_for_checkout(
    app: &tauri::AppHandle,
    root: &std::path::Path,
    tool: &str,
) -> Result<()> {
    let bundle = bundle(app)?;
    let source = if tool == "codex" {
        bundle.codex
    } else {
        bundle.claude
    };
    if source.exists() {
        anyhow::ensure!(
            tokio::fs::metadata(&source).await?.len() <= 32768,
            "Owned hook configuration exceeds limit"
        );
        merge_checkout_hooks(root, tool, &tokio::fs::read(source).await?).await?;
    }
    Ok(())
}
async fn merge_checkout_hooks(root: &std::path::Path, tool: &str, source: &[u8]) -> Result<()> {
    let (directory, file) = if tool == "codex" {
        (".codex", "hooks.json")
    } else {
        (".claude", "settings.local.json")
    };
    let directory = root.join(directory);
    match tokio::fs::symlink_metadata(&directory).await {
        Ok(m) => anyhow::ensure!(
            m.is_dir() && !m.file_type().is_symlink(),
            "Agent settings directory must not be a symlink"
        ),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            tokio::fs::create_dir(&directory).await?
        }
        Err(e) => return Err(e.into()),
    }
    let file = directory.join(file);
    let mut value = match tokio::fs::symlink_metadata(&file).await {
        Ok(m) => {
            anyhow::ensure!(
                m.is_file() && !m.file_type().is_symlink() && m.len() <= 65536,
                "Existing agent settings must be a bounded regular file"
            );
            serde_json::from_slice::<serde_json::Value>(&tokio::fs::read(&file).await?)?
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => serde_json::json!({}),
        Err(e) => return Err(e.into()),
    };
    let object = value
        .as_object_mut()
        .context("Agent settings must be an object")?;
    let target = object
        .entry("hooks")
        .or_insert_with(|| serde_json::json!({}))
        .as_object_mut()
        .context("Agent hooks must be an object")?;
    let added: serde_json::Value = serde_json::from_slice(source)?;
    for (name, groups) in added["hooks"]
        .as_object()
        .context("Invalid owned hook configuration")?
    {
        let entries = target
            .entry(name.clone())
            .or_insert_with(|| serde_json::json!([]))
            .as_array_mut()
            .context("Existing agent hook event must be an array")?;
        for group in groups.as_array().context("Invalid owned hook event")? {
            if !entries.contains(group) {
                entries.push(group.clone());
            }
        }
    }
    let text = serde_json::to_string_pretty(&value)?;
    anyhow::ensure!(text.len() <= 131072, "Merged hook settings exceed limit");
    diffle::persist::write_file_atomic(&file, &text).await?;
    Ok(())
}
#[cfg(test)]
mod additive_tests {
    use super::*;
    #[tokio::test]
    async fn project_hooks_append_without_replacing_policies_or_permission_settings() {
        let temp = tempfile::tempdir().unwrap();
        let directory = temp.path().join(".claude");
        std::fs::create_dir(&directory).unwrap();
        let file = directory.join("settings.local.json");
        std::fs::write(&file,r#"{"permissions":{"deny":["Bash(rm:*)"]},"hooks":{"PreToolUse":[{"hooks":[{"type":"command","command":"user-policy"}]}]}}"#).unwrap();
        let ours=br#"{"hooks":{"PreToolUse":[{"hooks":[{"type":"command","command":"mdiff-metadata"}]}]}}"#;
        merge_checkout_hooks(temp.path(), "claude", ours)
            .await
            .unwrap();
        merge_checkout_hooks(temp.path(), "claude", ours)
            .await
            .unwrap();
        let merged: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&file).unwrap()).unwrap();
        assert_eq!(merged["permissions"]["deny"][0], "Bash(rm:*)");
        assert_eq!(merged["hooks"]["PreToolUse"].as_array().unwrap().len(), 2);
        assert_eq!(
            merged["hooks"]["PreToolUse"][0]["hooks"][0]["command"],
            "user-policy"
        );
    }
    #[tokio::test]
    async fn hook_installation_refuses_settings_symlinks() {
        let temp = tempfile::tempdir().unwrap();
        let outside = temp.path().join("outside");
        std::fs::create_dir(&outside).unwrap();
        std::os::unix::fs::symlink(&outside, temp.path().join(".codex")).unwrap();
        assert!(
            merge_checkout_hooks(temp.path(), "codex", br#"{"hooks":{}}"#)
                .await
                .is_err()
        );
        assert!(!outside.join("hooks.json").exists());
    }
}
