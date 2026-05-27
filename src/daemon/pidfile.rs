//! Single-instance lock for the daemon, backed by `flock(2)`.
//!
//! `flock` locks are tied to the open file description, so they're
//! released automatically by the kernel when the holding process exits
//! — including on SIGKILL or a hard crash — which means we never have
//! to deal with stale PID files.

use std::fs::OpenOptions;
use std::io::{self, Seek, SeekFrom, Write};
use std::os::fd::AsRawFd;
use std::path::{Path, PathBuf};

pub fn pid_path() -> io::Result<PathBuf> {
    Ok(super::storage::cache_dir()?.join("rprocd.pid"))
}

pub struct PidFile {
    // Holds the lock for the lifetime of the value: dropping the File
    // closes the fd, which releases the flock.
    _file: std::fs::File,
}

impl PidFile {
    /// Try to acquire an exclusive non-blocking flock on `path`. Returns
    /// `Ok(None)` if another live process is already holding it (i.e. a
    /// daemon is already running).
    pub fn acquire(path: &Path) -> io::Result<Option<Self>> {
        let mut file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(path)?;
        let rc = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) };
        if rc != 0 {
            let err = io::Error::last_os_error();
            if matches!(err.raw_os_error(), Some(libc::EWOULDBLOCK)) {
                return Ok(None);
            }
            return Err(err);
        }
        file.set_len(0)?;
        file.seek(SeekFrom::Start(0))?;
        writeln!(file, "{}", std::process::id())?;
        file.flush()?;
        Ok(Some(Self { _file: file }))
    }

    /// Read the PID recorded in the pidfile, if the file exists and holds a
    /// parseable integer. Used to signal a running daemon to stop.
    pub fn read_pid(path: &Path) -> Option<libc::pid_t> {
        std::fs::read_to_string(path).ok()?.trim().parse().ok()
    }

    /// Best-effort probe: returns `true` if some other process currently
    /// holds the lock. There's an unavoidable race between this check and
    /// any subsequent spawn — but a duplicate daemon will itself fail to
    /// acquire the lock in `acquire()` and exit immediately, so the worst
    /// case is one short-lived extra process.
    pub fn is_locked(path: &Path) -> bool {
        let file = match OpenOptions::new()
            .read(true)
            .write(true)
            .create(false)
            .open(path)
        {
            Ok(f) => f,
            Err(_) => return false,
        };
        let rc = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) };
        if rc == 0 {
            unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_UN) };
            false
        } else {
            true
        }
    }
}
