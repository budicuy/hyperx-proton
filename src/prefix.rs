use crate::config::PrefixLayout;
use std::fs::{self, File, OpenOptions};
use std::io;
use std::os::unix::io::AsRawFd;
use std::path::{Path, PathBuf};

/// Exclusive `flock` guard for a Wine prefix or compat-data directory.
pub struct PrefixLock {
    file: File,
}

impl Drop for PrefixLock {
    fn drop(&mut self) {
        unsafe {
            libc::flock(self.file.as_raw_fd(), libc::LOCK_UN);
        }
    }
}

/// Acquires an exclusive blocking `flock` on `lock_path`, creating parent
/// directories if needed.
pub fn acquire_prefix_lock(lock_path: &Path) -> Result<PrefixLock, String> {
    if let Some(parent) = lock_path.parent()
        && !parent.as_os_str().is_empty()
    {
        fs::create_dir_all(parent).map_err(|err| {
            format!(
                "failed to create lock parent dir {}: {err}",
                parent.display()
            )
        })?;
    }

    let file = OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .truncate(false)
        .open(lock_path)
        .map_err(|err| format!("failed to open lockfile {}: {err}", lock_path.display()))?;

    loop {
        let rc = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX) };
        if rc == 0 {
            return Ok(PrefixLock { file });
        }
        let err = io::Error::last_os_error();
        if err.kind() == io::ErrorKind::Interrupted {
            continue;
        }
        return Err(format!(
            "failed to acquire flock on {}: {err}",
            lock_path.display()
        ));
    }
}

/// Summary of actions performed by [`sync_prefix_dlls`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PrefixSyncReport {
    pub dlls_updated: bool,
    pub needs_wineboot: bool,
}

fn target_prefix_files(
    runner_root: &Path,
    enable_nvapi: bool,
) -> Vec<(PathBuf, &'static str, &'static str)> {
    let mut list = vec![
        (
            runner_root.join("files/lib/dxvk/x86_64-windows/d3d11.dll"),
            "drive_c/windows/system32",
            "d3d11.dll",
        ),
        (
            runner_root.join("files/lib/dxvk/x86_64-windows/dxgi.dll"),
            "drive_c/windows/system32",
            "dxgi.dll",
        ),
        (
            runner_root.join("files/lib/vkd3d-proton/x86_64-windows/d3d12.dll"),
            "drive_c/windows/system32",
            "d3d12.dll",
        ),
        (
            runner_root.join("files/lib/vkd3d-proton/x86_64-windows/d3d12core.dll"),
            "drive_c/windows/system32",
            "d3d12core.dll",
        ),
    ];

    if enable_nvapi {
        list.push((
            runner_root.join("files/lib/nvapi/x86_64-windows/nvapi64.dll"),
            "drive_c/windows/system32",
            "nvapi64.dll",
        ));
    }

    // ICU 68.2 DLLs required by wine-cachyos icu.dll forwarders (.NET / Sharphereal).
    for (arch_dir, dest_dir) in [
        ("files/lib/wine/icu/x86_64-windows", "drive_c/windows/system32"),
        ("files/lib/wine/icu/i386-windows", "drive_c/windows/syswow64"),
    ] {
        let src_dir = runner_root.join(arch_dir);
        if src_dir.is_dir() {
            for dll in ["icudt68.dll", "icuin68.dll", "icuuc68.dll"] {
                list.push((src_dir.join(dll), dest_dir, dll));
            }
        }
    }

    // Native steam.exe child-launcher shim (required by HoYoProtect AC on Linux).
    let steam_shim = runner_root.join("files/lib/wine/x86_64-windows/steam.exe");
    if steam_shim.is_file() {
        list.push((steam_shim, "drive_c/windows/system32", "steam.exe"));
    }

    list
}

fn is_regular_file(path: &Path) -> bool {
    fs::symlink_metadata(path)
        .map(|meta| meta.file_type().is_file())
        .unwrap_or(false)
}

fn remove_existing_if_present(path: &Path) -> Result<(), String> {
    match fs::symlink_metadata(path) {
        Ok(_) => fs::remove_file(path)
            .map_err(|err| format!("failed to remove old {}: {err}", path.display())),
        Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(err) => Err(format!(
            "failed to inspect {}: {err}",
            path.display()
        )),
    }
}

/// Directories inside a Wine prefix where Proton/Wine places builtin PE symlinks.
const WINE_SYSTEM_DIRS: &[&str] = &[
    "drive_c/windows",
    "drive_c/windows/system32",
    "drive_c/windows/system32/drivers",
    "drive_c/windows/syswow64",
    "drive_c/windows/syswow64/drivers",
];

fn is_foreign_wine_symlink(runner_root: &Path, entry_path: &Path) -> bool {
    let Ok(meta) = fs::symlink_metadata(entry_path) else {
        return false;
    };
    if !meta.file_type().is_symlink() {
        return false;
    }
    let Ok(target) = fs::read_link(entry_path) else {
        return false;
    };
    if target.starts_with(runner_root) {
        return false;
    }
    // Remove dangling symlinks or symlinks pointing into another Wine/Proton runner's lib/wine tree.
    let target_str = target.to_string_lossy();
    !entry_path.exists()
        || target_str.contains("/files/lib/wine/")
        || target_str.contains("/lib/wine/")
        || target_str.contains("/lib64/wine/")
}

/// Returns true if any Windows system directory inside `wineprefix` still holds
/// symlinks pointing to another Proton/Wine installation or dangling links.
pub fn has_foreign_runner_symlinks(runner_root: &Path, wineprefix: &Path) -> bool {
    for rel in WINE_SYSTEM_DIRS {
        let dir = wineprefix.join(rel);
        let Ok(entries) = fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            if is_foreign_wine_symlink(runner_root, &entry.path()) {
                return true;
            }
        }
    }
    false
}

/// Removes stale symlinks in `drive_c/windows/{,system32,syswow64}` left behind
/// by other Proton/Wine runners (e.g. GE-Proton, Proton-CachyOS) so `wineboot -u`
/// repopulates them with this runner's matching binaries without touching
/// `user.reg`, `system.reg`, `drive_c/users/`, or any regular user files.
pub fn prune_foreign_runner_symlinks(runner_root: &Path, wineprefix: &Path) -> Result<usize, String> {
    let mut removed = 0usize;
    for rel in WINE_SYSTEM_DIRS {
        let dir = wineprefix.join(rel);
        let Ok(entries) = fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if is_foreign_wine_symlink(runner_root, &path) {
                fs::remove_file(&path).map_err(|err| {
                    format!(
                        "failed to remove stale foreign runner symlink {}: {err}",
                        path.display()
                    )
                })?;
                removed += 1;
            }
        }
    }
    Ok(removed)
}

/// Returns true when the prefix needs `wineboot -u` before DLL deployment.
pub fn prefix_needs_wineboot(
    runner_root: &Path,
    layout: &PrefixLayout,
    runner_version: &str,
) -> bool {
    let version_stamp_path = layout.wineprefix.join(".hyperx-proton-version");
    let current_stamp = fs::read_to_string(&version_stamp_path)
        .ok()
        .map(|s| s.trim().to_string());
    let version_changed = current_stamp.as_deref() != Some(runner_version);
    let system_reg_missing = !layout.wineprefix.join("system.reg").is_file();
    version_changed
        || system_reg_missing
        || has_foreign_runner_symlinks(runner_root, &layout.wineprefix)
}

/// Non-destructively syncs DX11/DX12 (and opt-in NVAPI) DLLs into `system32`
/// and updates version stamps without touching `user.reg`, `system.reg`, or
/// `drive_c/users/`.
pub fn sync_prefix_dlls(
    runner_root: &Path,
    layout: &PrefixLayout,
    runner_version: &str,
    enable_nvapi: bool,
) -> Result<PrefixSyncReport, String> {
    let sys32 = layout.wineprefix.join("drive_c/windows/system32");
    fs::create_dir_all(&sys32).map_err(|err| {
        format!(
            "failed to create prefix system32 {}: {err}",
            sys32.display()
        )
    })?;

    let version_stamp_path = layout.wineprefix.join(".hyperx-proton-version");
    let current_stamp = fs::read_to_string(&version_stamp_path)
        .ok()
        .map(|s| s.trim().to_string());
    let version_changed = current_stamp.as_deref() != Some(runner_version);
    let system_reg_missing = !layout.wineprefix.join("system.reg").is_file();

    let dlls = target_prefix_files(runner_root, enable_nvapi);
    let all_dlls_present = dlls
        .iter()
        .all(|(_, dest_dir, name)| is_regular_file(&layout.wineprefix.join(dest_dir).join(name)));

    let dlls_updated = if version_changed || !all_dlls_present {
        for (src, dest_dir, name) in &dlls {
            if !src.is_file() {
                return Err(format!("missing bundled runtime file: {}", src.display()));
            }
            let target_dir = layout.wineprefix.join(dest_dir);
            fs::create_dir_all(&target_dir).map_err(|err| {
                format!(
                    "failed to create prefix dir {}: {err}",
                    target_dir.display()
                )
            })?;
            let dst = target_dir.join(name);
            remove_existing_if_present(&dst)?;
            fs::copy(src, &dst).map_err(|err| {
                format!(
                    "failed to copy {} to {}: {err}",
                    src.display(),
                    dst.display()
                )
            })?;
        }

        fs::write(&version_stamp_path, format!("{runner_version}\n")).map_err(|err| {
            format!(
                "failed to write {}: {err}",
                version_stamp_path.display()
            )
        })?;

        if let Some(compat) = &layout.compat_data_dir {
            let compat_version = compat.join("version");
            fs::write(&compat_version, format!("{runner_version}\n")).map_err(|err| {
                format!(
                    "failed to write {}: {err}",
                    compat_version.display()
                )
            })?;
        }

        true
    } else {
        false
    };

    Ok(PrefixSyncReport {
        dlls_updated,
        needs_wineboot: version_changed || system_reg_missing,
    })
}
