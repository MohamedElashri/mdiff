//! Process metadata only: no command lines, environments or conversation files.
#[cfg(target_os = "linux")]
use anyhow::Context;
use anyhow::Result;
use std::path::PathBuf;
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Process {
    pub pid: i32,
    pub parent: i32,
    pub start: String,
    pub executable: PathBuf,
    pub cwd: PathBuf,
}
#[cfg(target_os = "macos")]
pub fn pids() -> Result<Vec<i32>> {
    let mut rows = vec![0i32; 65536];
    let n = unsafe { libc::proc_listallpids(rows.as_mut_ptr().cast(), (rows.len() * 4) as i32) };
    anyhow::ensure!(
        n > 0 && (n as usize) < rows.len(),
        "Process inventory limit reached"
    );
    rows.truncate(n as usize);
    Ok(rows)
}
#[cfg(target_os = "macos")]
pub fn inspect(pid: i32) -> Result<Process> {
    use std::{ffi::CStr, mem::size_of};
    unsafe {
        let mut bsd = std::mem::zeroed::<libc::proc_bsdinfo>();
        let n = libc::proc_pidinfo(
            pid,
            libc::PROC_PIDTBSDINFO,
            0,
            (&mut bsd as *mut libc::proc_bsdinfo).cast(),
            size_of::<libc::proc_bsdinfo>() as i32,
        );
        anyhow::ensure!(
            n as usize == size_of::<libc::proc_bsdinfo>() && bsd.pbi_uid == libc::getuid(),
            "Process unavailable"
        );
        let mut path = [0i8; 4096];
        anyhow::ensure!(
            libc::proc_pidpath(pid, path.as_mut_ptr().cast(), path.len() as u32) > 0,
            "Executable unavailable"
        );
        let executable = PathBuf::from(CStr::from_ptr(path.as_ptr()).to_str()?);
        let mut cwd = std::mem::zeroed::<libc::proc_vnodepathinfo>();
        anyhow::ensure!(
            libc::proc_pidinfo(
                pid,
                libc::PROC_PIDVNODEPATHINFO,
                0,
                (&mut cwd as *mut libc::proc_vnodepathinfo).cast(),
                size_of::<libc::proc_vnodepathinfo>() as i32
            ) as usize
                == size_of::<libc::proc_vnodepathinfo>(),
            "Process directory unavailable"
        );
        let root = PathBuf::from(CStr::from_ptr(cwd.pvi_cdir.vip_path.as_ptr().cast()).to_str()?);
        // Repeat start identity after the multi-call inspection to reject PID reuse.
        let mut end = std::mem::zeroed::<libc::proc_bsdinfo>();
        anyhow::ensure!(
            libc::proc_pidinfo(
                pid,
                libc::PROC_PIDTBSDINFO,
                0,
                (&mut end as *mut libc::proc_bsdinfo).cast(),
                size_of::<libc::proc_bsdinfo>() as i32
            ) as usize
                == size_of::<libc::proc_bsdinfo>()
                && bsd.pbi_start_tvsec == end.pbi_start_tvsec
                && bsd.pbi_start_tvusec == end.pbi_start_tvusec,
            "Process identity changed"
        );
        Ok(Process {
            pid,
            parent: bsd.pbi_ppid as i32,
            start: format!("{}:{}", bsd.pbi_start_tvsec, bsd.pbi_start_tvusec),
            executable,
            cwd: root,
        })
    }
}
#[cfg(target_os = "linux")]
pub fn pids() -> Result<Vec<i32>> {
    let mut rows = vec![];
    for entry in std::fs::read_dir("/proc")? {
        if let Ok(pid) = entry?.file_name().to_string_lossy().parse() {
            rows.push(pid);
        }
        anyhow::ensure!(rows.len() <= 65536, "Process inventory limit reached");
    }
    Ok(rows)
}
#[cfg(target_os = "linux")]
pub fn inspect(pid: i32) -> Result<Process> {
    use std::os::unix::fs::MetadataExt;
    let root = PathBuf::from(format!("/proc/{pid}"));
    anyhow::ensure!(
        std::fs::metadata(&root)?.uid() == unsafe { libc::getuid() },
        "Foreign process"
    );
    let before = std::fs::read_to_string(root.join("stat"))?;
    let fields = before
        .rsplit_once(") ")
        .context("Malformed process stat")?
        .1
        .split_whitespace()
        .collect::<Vec<_>>();
    let start = fields.get(19).context("Missing process start")?.to_string();
    let value = Process {
        pid,
        parent: fields.get(1).context("Missing parent")?.parse()?,
        start,
        executable: std::fs::read_link(root.join("exe"))?,
        cwd: std::fs::read_link(root.join("cwd"))?,
    };
    let after = std::fs::read_to_string(root.join("stat"))?;
    anyhow::ensure!(
        after
            .rsplit_once(") ")
            .context("Malformed process stat")?
            .1
            .split_whitespace()
            .nth(19)
            == Some(value.start.as_str()),
        "Process identity changed"
    );
    Ok(value)
}
pub fn tool(p: &Process, tools: &[(String, PathBuf)]) -> Option<String> {
    tools
        .iter()
        .find(|(_, path)| path == &p.executable)
        .map(|(name, _)| name.clone())
        .or_else(|| {
            p.executable
                .file_name()
                .and_then(|p| p.to_str())
                .filter(|p| ["codex", "claude"].contains(p))
                .map(str::to_owned)
        })
}
pub fn ancestor(mut pid: i32, tools: &[(String, PathBuf)]) -> Result<(Process, String)> {
    for _ in 0..12 {
        let p = inspect(pid)?;
        if let Some(tool) = tool(&p, tools) {
            return Ok((p, tool));
        }
        anyhow::ensure!(
            p.parent > 1 && p.parent != pid,
            "No supported agent ancestor"
        );
        pid = p.parent;
    }
    anyhow::bail!("Agent ancestry limit reached")
}
pub fn peer_pid(stream: &std::os::unix::net::UnixStream) -> Result<i32> {
    use std::os::fd::AsRawFd;
    #[cfg(target_os = "macos")]
    unsafe {
        let mut uid = 0;
        let mut gid = 0;
        anyhow::ensure!(
            libc::getpeereid(stream.as_raw_fd(), &mut uid, &mut gid) == 0 && uid == libc::getuid(),
            "Foreign hook peer"
        );
        let mut pid: i32 = 0;
        let mut len = std::mem::size_of::<i32>() as libc::socklen_t;
        anyhow::ensure!(
            libc::getsockopt(
                stream.as_raw_fd(),
                0,
                libc::LOCAL_PEERPID,
                (&mut pid as *mut i32).cast(),
                &mut len
            ) == 0,
            "Hook peer unavailable"
        );
        Ok(pid)
    }
    #[cfg(target_os = "linux")]
    unsafe {
        let mut cred = std::mem::zeroed::<libc::ucred>();
        let mut len = std::mem::size_of::<libc::ucred>() as libc::socklen_t;
        anyhow::ensure!(
            libc::getsockopt(
                stream.as_raw_fd(),
                libc::SOL_SOCKET,
                libc::SO_PEERCRED,
                (&mut cred as *mut libc::ucred).cast(),
                &mut len
            ) == 0
                && cred.uid == libc::getuid(),
            "Foreign hook peer"
        );
        Ok(cred.pid)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn own_identity_is_stable_and_classification_never_reads_arguments() {
        let a = inspect(std::process::id() as i32).unwrap();
        let b = inspect(a.pid).unwrap();
        assert_eq!(a.start, b.start);
        assert_eq!(a.cwd, b.cwd);
        assert!(a.executable.is_absolute());
        assert_eq!(tool(&a, &[]), None);
        assert_eq!(
            tool(&a, &[("codex".into(), a.executable.clone())]),
            Some("codex".into())
        );
    }
    #[test]
    fn local_hook_peer_uses_kernel_identity() {
        let temp = tempfile::tempdir().unwrap();
        let socket = temp.path().join("sock");
        let listener = std::os::unix::net::UnixListener::bind(&socket).unwrap();
        let _client = std::os::unix::net::UnixStream::connect(&socket).unwrap();
        let (peer, _) = listener.accept().unwrap();
        assert_eq!(peer_pid(&peer).unwrap(), std::process::id() as i32);
    }
}
