use std::fs::{self, File, OpenOptions};
use std::io;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::io::AsRawFd;
use std::path::Path;

/// Linux `FICLONE` ioctl request code (`_IOW(0x94, 9, int)`).
const FICLONE: libc::c_ulong = 0x4004_9409;

/// Default target timer slack (1 microsecond) inherited by Wine/game threads.
pub const DEFAULT_TIMERSLACK_NS: u64 = 1_000;

/// Target soft `RLIMIT_NOFILE` ceiling for open-world asset streaming and sync FDs.
pub const TARGET_NOFILE_SOFT: u64 = 524_288;

/// Outcome of [`reflink_or_copy`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CopyMethod {
    Reflink,
    RegularCopy,
}

/// Summary of process-level kernel tuning applied before executing Wine.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProcessTuneReport {
    pub timerslack_applied: bool,
    pub timerslack_ns: u64,
    pub nofile_soft: Option<u64>,
}

/// Applies fail-soft Linux kernel tuning (`PR_SET_TIMERSLACK` and `RLIMIT_NOFILE`)
/// to the current process so all spawned Wine/DXVK/vkd3d child processes inherit it.
pub fn apply_process_tuning(timerslack_ns: u64) -> ProcessTuneReport {
    let timerslack_applied = if timerslack_ns > 0 {
        let rc = unsafe {
            libc::prctl(
                libc::PR_SET_TIMERSLACK,
                timerslack_ns as libc::c_ulong,
                0,
                0,
                0,
            )
        };
        rc == 0
    } else {
        false
    };

    let nofile_soft = raise_nofile_limit(TARGET_NOFILE_SOFT);

    ProcessTuneReport {
        timerslack_applied,
        timerslack_ns,
        nofile_soft,
    }
}

/// Raises the soft `RLIMIT_NOFILE` up to `min(target_soft, rlim_max)` if currently lower.
pub fn raise_nofile_limit(target_soft: u64) -> Option<u64> {
    let mut lim = libc::rlimit {
        rlim_cur: 0,
        rlim_max: 0,
    };
    if unsafe { libc::getrlimit(libc::RLIMIT_NOFILE, &mut lim) } != 0 {
        return None;
    }

    let desired = target_soft.min(lim.rlim_max);
    if lim.rlim_cur < desired {
        lim.rlim_cur = desired;
        if unsafe { libc::setrlimit(libc::RLIMIT_NOFILE, &lim) } == 0 {
            return Some(desired);
        }
    }

    Some(lim.rlim_cur)
}

/// Copies `src` to `dst`, preferring zero-copy CoW reflink (`FICLONE`, e.g. Btrfs/XFS)
/// and transparently falling back to a regular buffered copy on unsupported filesystems.
pub fn reflink_or_copy(src: &Path, dst: &Path) -> Result<CopyMethod, String> {
    let src_meta = fs::metadata(src)
        .map_err(|err| format!("failed to stat {}: {err}", src.display()))?;
    let mut src_file = File::open(src)
        .map_err(|err| format!("failed to open {}: {err}", src.display()))?;

    let mut dst_file = OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(dst)
        .map_err(|err| format!("failed to create {}: {err}", dst.display()))?;

    let rc = unsafe { libc::ioctl(dst_file.as_raw_fd(), FICLONE, src_file.as_raw_fd()) };
    let method = if rc == 0 {
        CopyMethod::Reflink
    } else {
        io::copy(&mut src_file, &mut dst_file).map_err(|err| {
            format!(
                "failed to copy {} to {}: {err}",
                src.display(),
                dst.display()
            )
        })?;
        CopyMethod::RegularCopy
    };

    let perms = fs::Permissions::from_mode(src_meta.permissions().mode() & 0o777);
    let _ = dst_file.set_permissions(perms);

    Ok(method)
}
