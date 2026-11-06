//! Unix reads relative to a pinned repository directory. No path component may
//! follow a symlink; renaming a directory during a read cannot redirect it.
use std::ffi::CString;
use std::fs::File;
use std::io::{self, Read};
use std::os::fd::{AsRawFd, FromRawFd};
use std::os::unix::ffi::OsStrExt;
use std::path::{Component, Path};

fn absent(error: &io::Error) -> bool {
    matches!(error.raw_os_error(), Some(libc::ENOENT | libc::ENOTDIR | libc::ELOOP))
}

fn open_at(parent: &File, name: &CString, flags: libc::c_int) -> io::Result<File> {
    // SAFETY: parent is a live descriptor, name is NUL terminated, and no mode
    // argument is required because these calls never create a file.
    let fd = unsafe { libc::openat(parent.as_raw_fd(), name.as_ptr(), flags | libc::O_CLOEXEC) };
    if fd < 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: openat returned a fresh descriptor, now owned solely by this File.
    Ok(unsafe { File::from_raw_fd(fd) })
}

pub(super) fn read(root: &File, path: &Path, limit: usize, links: bool) -> io::Result<Option<Vec<u8>>> {
    let parts = path
        .components()
        .map(|p| match p {
            Component::Normal(name) => CString::new(name.as_bytes()).ok(),
            _ => None,
        })
        .collect::<Option<Vec<_>>>();
    let Some(parts) = parts else { return Ok(None) };
    let Some((name, parents)) = parts.split_last() else { return Ok(None) };
    let mut dir = root.try_clone()?;
    for name in parents {
        dir = match open_at(&dir, name, libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW) {
            Ok(dir) => dir,
            Err(e) if absent(&e) => return Ok(None),
            Err(e) => return Err(e),
        };
    }
    let file = match open_at(&dir, name, libc::O_RDONLY | libc::O_NOFOLLOW | libc::O_NONBLOCK) {
        Ok(file) => file,
        Err(e) if e.raw_os_error() == Some(libc::ELOOP) && links => {
            // readlinkat reads the link itself relative to the pinned parent,
            // even if another process replaces the final entry meanwhile.
            let mut buf = vec![0; 256];
            loop {
                // SAFETY: the output buffer is writable for its full length.
                let n = unsafe { libc::readlinkat(dir.as_raw_fd(), name.as_ptr(), buf.as_mut_ptr().cast(), buf.len()) };
                if n < 0 {
                    let e = io::Error::last_os_error();
                    return if absent(&e) || e.raw_os_error() == Some(libc::EINVAL) { Ok(None) } else { Err(e) };
                }
                if (n as usize) < buf.len() {
                    buf.truncate((n as usize).min(limit));
                    return Ok(Some(buf));
                }
                if buf.len() >= 1024 * 1024 {
                    return Err(io::Error::other("Symlink target too long"));
                }
                buf.resize(buf.len() * 2, 0);
            }
        }
        Err(e) if absent(&e) => return Ok(None),
        Err(e) => return Err(e),
    };
    if !file.metadata()?.is_file() {
        return Ok(None);
    }
    let mut buf = Vec::new();
    file.take(limit as u64).read_to_end(&mut buf)?;
    Ok(Some(buf))
}
