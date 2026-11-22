//! Unix PTY actor: finite input/output credit, nonblocking I/O, and owned-session teardown.
use anyhow::{Context, Result};
use portable_pty::{native_pty_system, CommandBuilder, PtySize};
use serde::Serialize;
use std::{
    collections::VecDeque,
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering},
        mpsc, Arc, Mutex,
    },
    time::{Duration, Instant},
};

pub const CHUNK: usize = 8192;
pub const WINDOW: u64 = 4;
pub const INPUT_LIMIT: usize = 65536;
pub type Sink = Arc<dyn Fn(Vec<u8>) -> bool + Send + Sync>;
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Exit {
    pub code: Option<u32>,
    pub message: String,
}
enum Command {
    Input(Vec<u8>),
    Resize(u16, u16),
}
pub struct Actor {
    pub pid: u32,
    close: Arc<AtomicBool>,
    input: Arc<AtomicUsize>,
    sent: Arc<AtomicU64>,
    acked: Arc<AtomicU64>,
    tx: mpsc::SyncSender<Command>,
    pub done: tokio::sync::watch::Receiver<bool>,
    pub status: Arc<Mutex<Option<Exit>>>,
}
impl Actor {
    pub fn stop(&self) {
        self.close.store(true, Ordering::SeqCst);
    }
    pub fn ack(&self, seq: u64) -> Result<()> {
        anyhow::ensure!(
            seq > 0 && seq <= self.sent.load(Ordering::SeqCst),
            "Invalid terminal acknowledgement"
        );
        self.acked.fetch_max(seq, Ordering::SeqCst);
        Ok(())
    }
    pub fn input(&self, data: Vec<u8>) -> Result<()> {
        anyhow::ensure!(
            !self.close.load(Ordering::SeqCst) && !*self.done.borrow(),
            "Terminal is no longer running"
        );
        anyhow::ensure!(
            !data.is_empty() && data.len() <= INPUT_LIMIT,
            "Terminal input exceeds 64 KiB"
        );
        let len = data.len();
        self.input
            .try_update(Ordering::SeqCst, Ordering::SeqCst, |old| {
                old.checked_add(len).filter(|n| *n <= INPUT_LIMIT)
            })
            .map_err(|_| {
                anyhow::anyhow!("Terminal input is busy; nothing from this input was queued")
            })?;
        if let Err(error) = self.tx.try_send(Command::Input(data)) {
            self.input.fetch_sub(len, Ordering::SeqCst);
            return Err(anyhow::anyhow!("Terminal input is busy: {error}"));
        }
        Ok(())
    }
    pub fn resize(&self, cols: u16, rows: u16) -> Result<()> {
        validate_size(cols, rows)?;
        self.tx
            .try_send(Command::Resize(cols, rows))
            .context("Terminal resize queue is busy")
    }
    pub async fn finished(&self) -> Result<()> {
        let mut done = self.done.clone();
        while !*done.borrow_and_update() {
            done.changed()
                .await
                .context("Terminal worker disconnected")?;
        }
        Ok(())
    }
}
pub fn validate_size(cols: u16, rows: u16) -> Result<()> {
    anyhow::ensure!(
        (2..=300).contains(&cols) && (1..=120).contains(&rows),
        "Terminal size must be 2–300 columns and 1–120 rows"
    );
    Ok(())
}
pub fn shell() -> Result<PathBuf> {
    let default = if cfg!(target_os = "macos") {
        "/bin/zsh"
    } else {
        "/bin/bash"
    };
    let path = std::env::var_os("SHELL")
        .map(PathBuf::from)
        .unwrap_or_else(|| default.into());
    anyhow::ensure!(
        path.is_absolute() && path.is_file(),
        "Configured login shell is unavailable; use an external terminal"
    );
    Ok(path)
}
pub fn command(root: &Path, shell: &Path) -> Result<CommandBuilder> {
    anyhow::ensure!(
        root.is_absolute() && root.is_dir(),
        "Terminal checkout is unavailable; no shell was started"
    );
    let mut cmd = CommandBuilder::new(shell);
    cmd.args(["-l", "-i"]);
    cmd.cwd(root);
    cmd.env("TERM", "xterm-256color");
    cmd.env("COLORTERM", "truecolor");
    cmd.env("TERM_PROGRAM", "mdiff");
    cmd.env("PWD", root);
    clean_environment(&mut cmd);
    Ok(cmd)
}
pub(crate) fn clean_environment(cmd: &mut CommandBuilder) {
    for key in [
        "GIT_DIR",
        "GIT_WORK_TREE",
        "GIT_INDEX_FILE",
        "GIT_COMMON_DIR",
        "GIT_NAMESPACE",
        "GIT_PREFIX",
        "GIT_LITERAL_PATHSPECS",
        "GIT_GLOB_PATHSPECS",
        "GIT_NOGLOB_PATHSPECS",
        "GIT_ICASE_PATHSPECS",
        "GIT_CONFIG_COUNT",
        "GIT_CONFIG_PARAMETERS",
        "GIT_ASKPASS",
        "SSH_ASKPASS",
        "GH_TOKEN",
        "GITHUB_TOKEN",
        "GH_ENTERPRISE_TOKEN",
        "GITHUB_ENTERPRISE_TOKEN",
        "GITLAB_TOKEN",
        "GITLAB_ACCESS_TOKEN",
        "REX_SESSION",
        "REX_BLOCK",
    ] {
        cmd.env_remove(key);
    }
}
pub fn frame(seq: u64, kind: u32, data: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(16 + data.len());
    out.extend_from_slice(&seq.to_le_bytes());
    out.extend_from_slice(&kind.to_le_bytes());
    out.extend_from_slice(&[0; 4]);
    out.extend_from_slice(data);
    out
}
#[cfg(unix)]
fn exited(pid: u32) -> Result<bool> {
    let mut info = std::mem::MaybeUninit::<libc::siginfo_t>::zeroed();
    let result = unsafe {
        libc::waitid(
            libc::P_PID,
            pid as libc::id_t,
            info.as_mut_ptr(),
            libc::WEXITED | libc::WNOHANG | libc::WNOWAIT,
        )
    };
    if result < 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    Ok(unsafe { info.assume_init().si_pid() } != 0)
}
#[cfg(target_os = "macos")]
fn pids() -> Result<Vec<i32>> {
    #[link(name = "proc")]
    unsafe extern "C" {
        fn proc_listallpids(buffer: *mut libc::c_void, buffersize: libc::c_int) -> libc::c_int;
    }
    let mut pids = vec![0i32; 65536];
    let count = unsafe { proc_listallpids(pids.as_mut_ptr().cast(), (pids.len() * 4) as i32) };
    anyhow::ensure!(
        count > 0 && (count as usize) < pids.len(),
        "Could not enumerate terminal process session"
    );
    pids.truncate(count as usize);
    Ok(pids)
}
#[cfg(target_os = "linux")]
fn pids() -> Result<Vec<i32>> {
    let mut pids = vec![];
    for row in std::fs::read_dir("/proc")? {
        if let Ok(pid) = row?.file_name().to_string_lossy().parse() {
            pids.push(pid);
            anyhow::ensure!(pids.len() <= 65536, "Process inventory limit reached");
        }
    }
    Ok(pids)
}
#[cfg(unix)]
fn signal_session(pid: u32, signal: i32) -> Result<()> {
    // The leader remains unreaped while scanning/signalling, preventing session-ID reuse.
    for child in pids()? {
        if child > 1
            && unsafe { libc::getsid(child) } == pid as i32
            && unsafe { libc::kill(child, signal) } < 0
        {
            let error = std::io::Error::last_os_error();
            if error.raw_os_error() != Some(libc::ESRCH) {
                return Err(error.into());
            }
        }
    }
    Ok(())
}
// Also runs if worker creation fails or the worker unwinds after spawning a child.
struct OwnedChild {
    child: Box<dyn portable_pty::Child + Send + Sync>,
    pid: u32,
    reaped: bool,
}
impl OwnedChild {
    fn wait(&mut self) -> Result<portable_pty::ExitStatus> {
        let result = self.child.wait()?;
        self.reaped = true;
        Ok(result)
    }
}
impl Drop for OwnedChild {
    fn drop(&mut self) {
        if !self.reaped {
            let _ = signal_session(self.pid, libc::SIGKILL);
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }
}
#[cfg(unix)]
pub fn spawn(
    root: &Path,
    command: CommandBuilder,
    cols: u16,
    rows: u16,
    sink: Sink,
    lease: Box<dyn Send>,
) -> Result<Actor> {
    validate_size(cols, rows)?;
    anyhow::ensure!(
        root.is_absolute() && root.is_dir(),
        "Checkout disappeared before terminal startup"
    );
    let pair = native_pty_system().openpty(PtySize {
        cols,
        rows,
        pixel_width: 0,
        pixel_height: 0,
    })?;
    let fd = pair
        .master
        .as_raw_fd()
        .context("PTY does not expose nonblocking I/O")?;
    let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
    anyhow::ensure!(
        flags >= 0 && unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) } >= 0,
        "Could not configure nonblocking terminal I/O"
    );
    let mut reader = pair.master.try_clone_reader()?;
    let mut writer = pair.master.take_writer()?;
    let mut child = pair.slave.spawn_command(command)?;
    drop(pair.slave);
    let pid = match child.process_id() {
        Some(pid) => pid,
        None => {
            let _ = child.kill();
            let _ = child.wait();
            anyhow::bail!("PTY child has no process ID");
        }
    };
    let mut child = OwnedChild {
        child,
        pid,
        reaped: false,
    };
    let (tx, rx) = mpsc::sync_channel(16);
    let close = Arc::new(AtomicBool::new(false));
    let input = Arc::new(AtomicUsize::new(0));
    let sent = Arc::new(AtomicU64::new(0));
    let acked = Arc::new(AtomicU64::new(0));
    let status = Arc::new(Mutex::new(None));
    let (done_tx, done) = tokio::sync::watch::channel(false);
    let actor = Actor {
        pid,
        close: close.clone(),
        input: input.clone(),
        sent: sent.clone(),
        acked: acked.clone(),
        tx,
        done,
        status: status.clone(),
    };
    std::thread::Builder::new()
        .name("mdiff-pty".into())
        .spawn(move || {
            let run = (|| -> Result<Exit> {
                let mut pending = VecDeque::<Vec<u8>>::new();
                let mut offset = 0;
                let mut bytes = [0u8; CHUNK];
                let mut at_eof = false;
                let mut quitting = None;
                let mut natural = false;
                loop {
                    if quitting.is_none() && (close.load(Ordering::SeqCst) || exited(pid)?) {
                        natural = !close.load(Ordering::SeqCst);
                        signal_session(pid, libc::SIGTERM)?;
                        quitting = Some(Instant::now());
                    }
                    if let Some(start) = quitting {
                        if start.elapsed() >= Duration::from_millis(250) {
                            signal_session(pid, libc::SIGKILL)?;
                            break;
                        }
                    }
                    for _ in 0..16 {
                        match rx.try_recv() {
                            Ok(Command::Input(data)) => pending.push_back(data),
                            Ok(Command::Resize(cols, rows)) => {
                                pair.master.resize(PtySize {
                                    cols,
                                    rows,
                                    pixel_width: 0,
                                    pixel_height: 0,
                                })?;
                            }
                            Err(mpsc::TryRecvError::Empty) => break,
                            Err(mpsc::TryRecvError::Disconnected) => {
                                close.store(true, Ordering::SeqCst);
                                break;
                            }
                        }
                    }
                    if quitting.is_none() {
                        if let Some(data) = pending.front() {
                            match writer.write(&data[offset..]) {
                                Ok(n) => {
                                    offset += n;
                                    input.fetch_sub(n, Ordering::SeqCst);
                                    if offset == data.len() {
                                        pending.pop_front();
                                        offset = 0;
                                    }
                                }
                                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => (),
                                Err(e) => return Err(e.into()),
                            }
                        }
                    }
                    if !at_eof
                        && sent.load(Ordering::SeqCst) - acked.load(Ordering::SeqCst) < WINDOW
                    {
                        match reader.read(&mut bytes) {
                            Ok(0) => at_eof = true,
                            Ok(n) => {
                                let seq = sent.fetch_add(1, Ordering::SeqCst) + 1;
                                if !sink(frame(seq, 0, &bytes[..n])) {
                                    close.store(true, Ordering::SeqCst);
                                }
                            }
                            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => (),
                            Err(e) if e.raw_os_error() == Some(libc::EIO) => at_eof = true,
                            Err(e) => return Err(e.into()),
                        }
                    }
                    if !at_eof
                        && quitting.is_none()
                        && sent.load(Ordering::SeqCst) - acked.load(Ordering::SeqCst) < WINDOW
                    {
                        let mut poll = libc::pollfd {
                            fd,
                            events: libc::POLLIN
                                | if pending.is_empty() { 0 } else { libc::POLLOUT },
                            revents: 0,
                        };
                        // Input/resize/close commands wait at most 20 ms; output wakes immediately.
                        unsafe {
                            libc::poll(&mut poll, 1, 20);
                        }
                    } else {
                        std::thread::sleep(Duration::from_millis(20));
                    }
                }
                let result = child.wait()?;
                // At most the remaining PTY kernel output is discarded on explicit close.
                // A natural exit drains a finite tail without bypassing output credit.
                if natural {
                    let mut idle = Instant::now();
                    while !at_eof && !close.load(Ordering::SeqCst) {
                        if sent.load(Ordering::SeqCst) - acked.load(Ordering::SeqCst) >= WINDOW {
                            idle = Instant::now();
                            std::thread::sleep(Duration::from_millis(5));
                            continue;
                        }
                        if idle.elapsed() > Duration::from_secs(2) {
                            break;
                        }
                        match reader.read(&mut bytes) {
                            Ok(0) => break,
                            Ok(n) => {
                                idle = Instant::now();
                                let seq = sent.fetch_add(1, Ordering::SeqCst) + 1;
                                if !sink(frame(seq, 0, &bytes[..n])) {
                                    break;
                                }
                            }
                            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                                std::thread::sleep(Duration::from_millis(5))
                            }
                            Err(_) => break,
                        }
                    }
                }
                Ok(Exit {
                    code: Some(result.exit_code()),
                    message: if natural {
                        "Shell exited."
                    } else {
                        "Shell and its process-session jobs stopped."
                    }
                    .into(),
                })
            })();
            let result = match run {
                Ok(result) => result,
                Err(error) => {
                    let _ = signal_session(pid, libc::SIGKILL);
                    let _ = child.child.kill();
                    let _ = child.wait();
                    Exit {
                        code: None,
                        message: format!("Terminal ended after an error: {error}"),
                    }
                }
            };
            drop(reader);
            drop(writer);
            drop(pair.master);
            drop(lease);
            *status.lock().unwrap() = Some(result.clone());
            let _ = sink(frame(
                0,
                1,
                &serde_json::to_vec(&result).unwrap_or_default(),
            ));
            let _ = done_tx.send(true);
        })?;
    Ok(actor)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture(script: &str) -> (tempfile::TempDir, PathBuf, CommandBuilder) {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("checkout 'quotes' —\n folder");
        std::fs::create_dir(&root).unwrap();
        let root = root.canonicalize().unwrap();
        let mut command = CommandBuilder::new("/bin/sh");
        command.args(["-c", script]);
        command.cwd(&root);
        (tmp, root, command)
    }
    fn output() -> (Sink, mpsc::Receiver<Vec<u8>>) {
        let (tx, rx) = mpsc::channel();
        (Arc::new(move |bytes| tx.send(bytes).is_ok()), rx)
    }
    fn data(frame: &[u8]) -> Option<(u64, &[u8])> {
        let seq = u64::from_le_bytes(frame[..8].try_into().unwrap());
        (u32::from_le_bytes(frame[8..12].try_into().unwrap()) == 0).then_some((seq, &frame[16..]))
    }
    #[tokio::test]
    async fn real_pty_cwd_unicode_input_resize_and_natural_exit() {
        let (_tmp, root, command) =
            fixture("pwd; stty size; read line; printf 'INPUT:%s\\n' \"$line\"; stty size");
        let (sink, rx) = output();
        let actor = spawn(&root, command, 80, 24, sink, Box::new(())).unwrap();
        let mut text = Vec::new();
        while !String::from_utf8_lossy(&text).contains("24 80") {
            let frame = rx.recv_timeout(Duration::from_secs(3)).unwrap();
            if let Some((seq, bytes)) = data(&frame) {
                text.extend(bytes);
                actor.ack(seq).unwrap();
            }
        }
        actor.resize(120, 42).unwrap();
        actor.input("héllo 世界\r".as_bytes().to_vec()).unwrap();
        while !*actor.done.borrow() {
            if let Ok(frame) = rx.recv_timeout(Duration::from_secs(2)) {
                if let Some((seq, bytes)) = data(&frame) {
                    text.extend(bytes);
                    actor.ack(seq).unwrap();
                }
            }
        }
        let text = String::from_utf8(text).unwrap();
        assert!(text.contains(root.to_str().unwrap().split('\n').next().unwrap()));
        assert!(text.contains("INPUT:héllo 世界"));
        assert!(text.contains("42 120"));
        assert_eq!(actor.status.lock().unwrap().as_ref().unwrap().code, Some(0));
    }
    #[tokio::test]
    async fn output_credit_stalls_a_flood_without_losing_unicode_or_the_final_tail() {
        let(_tmp,root,command)=fixture("i=0; while [ $i -lt 12000 ]; do printf 'é世界:%s\\n' \"$i\"; i=$((i+1)); done; printf FINAL_MARKER");
        let (sink, rx) = output();
        let actor = spawn(&root, command, 80, 24, sink, Box::new(())).unwrap();
        std::thread::sleep(Duration::from_millis(200));
        assert!(actor.sent.load(Ordering::SeqCst) <= WINDOW);
        assert!(!*actor.done.borrow());
        let mut text = Vec::new();
        loop {
            let frame = rx.recv_timeout(Duration::from_secs(5)).unwrap();
            if let Some((seq, bytes)) = data(&frame) {
                assert!(bytes.len() <= CHUNK);
                text.extend(bytes);
                actor.ack(seq).unwrap();
            } else {
                break;
            }
        }
        tokio::time::timeout(Duration::from_secs(3), actor.finished())
            .await
            .unwrap()
            .unwrap();
        let text = String::from_utf8(text).unwrap();
        assert!(text.contains("é世界:11999"));
        assert!(text.ends_with("FINAL_MARKER"));
    }
    #[tokio::test]
    async fn close_kills_term_ignoring_background_jobs_before_releasing_the_checkout_lease() {
        struct Lease(Arc<AtomicBool>);
        impl Drop for Lease {
            fn drop(&mut self) {
                self.0.store(true, Ordering::SeqCst);
            }
        }
        let (_tmp, root, command) =
            fixture("trap '' TERM; sleep 100 & printf 'CHILD:%s\\n' \"$!\"; wait");
        let (sink, rx) = output();
        let released = Arc::new(AtomicBool::new(false));
        let actor = spawn(
            &root,
            command,
            80,
            24,
            sink,
            Box::new(Lease(released.clone())),
        )
        .unwrap();
        let mut bytes = vec![];
        while !String::from_utf8_lossy(&bytes).contains("CHILD:") {
            let frame = rx.recv_timeout(Duration::from_secs(3)).unwrap();
            if let Some((seq, data)) = data(&frame) {
                bytes.extend(data);
                actor.ack(seq).unwrap();
            }
        }
        let child = String::from_utf8_lossy(&bytes)
            .split("CHILD:")
            .nth(1)
            .unwrap()
            .split_whitespace()
            .next()
            .unwrap()
            .parse::<i32>()
            .unwrap();
        assert_eq!(unsafe { libc::getsid(child) }, actor.pid as i32);
        actor.stop();
        tokio::time::timeout(Duration::from_secs(3), actor.finished())
            .await
            .unwrap()
            .unwrap();
        assert!(released.load(Ordering::SeqCst));
        assert_eq!(unsafe { libc::kill(actor.pid as i32, 0) }, -1);
        for _ in 0..100 {
            if unsafe { libc::kill(child, 0) } < 0 {
                return;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        panic!("background terminal child was not reaped");
    }
    #[tokio::test]
    async fn bad_dimensions_input_and_forged_ack_are_rejected_and_dropped_controller_stops() {
        assert!(validate_size(0, 24).is_err());
        assert!(validate_size(80, 121).is_err());
        let (_tmp, root, command) = fixture("sleep 100");
        let (sink, _rx) = output();
        let actor = spawn(&root, command, 80, 24, sink, Box::new(())).unwrap();
        assert!(actor.ack(100).is_err());
        assert!(actor.input(vec![b'x'; INPUT_LIMIT + 1]).is_err());
        let pid = actor.pid;
        drop(actor);
        for _ in 0..100 {
            if unsafe { libc::kill(pid as i32, 0) } < 0 {
                return;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        panic!("dropped terminal controller leaked its shell");
    }
    #[test]
    fn spawning_a_missing_program_never_leaves_an_owned_process() {
        let (_tmp, root, _) = fixture("");
        let mut cmd = CommandBuilder::new(root.join("missing-program"));
        cmd.cwd(&root);
        let (sink, _) = output();
        assert!(spawn(&root, cmd, 80, 24, sink, Box::new(())).is_err());
    }
}
