use crate::config::PrefixLayout;
use crate::gpu::GpuVendor;
use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File};
use std::os::unix::io::AsRawFd;
use std::path::{Path, PathBuf};
use std::thread::{self, JoinHandle};

const MAX_PREWARM_FILES: usize = 256;
const MAX_PREWARM_BYTES: u64 = 512 * 1024 * 1024;

/// Statistics reported by the asynchronous shader cache pre-warmer.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct PrewarmStats {
    pub files_advised: usize,
    pub bytes_advised: u64,
}

fn is_shader_cache_file(path: &Path) -> bool {
    let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
        return false;
    };
    let lower = name.to_ascii_lowercase();
    lower.ends_with(".dxvk-cache")
        || lower.ends_with(".dxvk.bin")
        || lower.ends_with(".bin")
        || lower.ends_with(".toc")
        || lower.ends_with(".cache")
        || lower.ends_with(".foz")
        || lower.ends_with(".idx")
        || lower.starts_with("vkd3d-proton.cache")
}

/// Collects existing shader cache directories for the detected GPU vendor
/// (NVIDIA GLCache vs Mesa RADV/ANV cache) plus common DXVK/vkd3d directories.
pub fn collect_shader_cache_dirs_for_vendor(
    layout: &PrefixLayout,
    env: &BTreeMap<String, String>,
    exe_dir: Option<&Path>,
    vendor: Option<GpuVendor>,
) -> Vec<PathBuf> {
    let mut unique = BTreeSet::new();
    let mut dirs = Vec::new();

    let mut push_if_dir = |p: PathBuf| {
        if p.is_dir() && unique.insert(p.clone()) {
            dirs.push(p);
        }
    };

    for key in ["DXVK_STATE_CACHE_PATH", "VKD3D_SHADER_CACHE_PATH"] {
        if let Some(val) = env.get(key) {
            let trimmed = val.trim();
            if !trimmed.is_empty() {
                push_if_dir(PathBuf::from(trimmed));
            }
        }
    }

    if let Some(dir) = exe_dir {
        push_if_dir(dir.to_path_buf());
    }

    push_if_dir(
        layout
            .wineprefix
            .join("drive_c/users/steamuser/AppData/Local/dxvk"),
    );

    if let Some(compat) = &layout.compat_data_dir {
        push_if_dir(compat.join("shadercache"));
    }

    match vendor {
        None | Some(GpuVendor::Nvidia) => {
            if let Some(val) = env.get("__GL_SHADER_DISK_CACHE_PATH") {
                let trimmed = val.trim();
                if !trimmed.is_empty() {
                    push_if_dir(PathBuf::from(trimmed));
                }
            }
            if let Some(home) = env.get("HOME") {
                let trimmed = home.trim();
                if !trimmed.is_empty() {
                    push_if_dir(PathBuf::from(trimmed).join(".cache/nvidia/GLCache"));
                }
            }
        }
        Some(GpuVendor::Amd | GpuVendor::Intel) => {
            if let Some(val) = env.get("MESA_SHADER_CACHE_DIR") {
                let trimmed = val.trim();
                if !trimmed.is_empty() {
                    push_if_dir(PathBuf::from(trimmed));
                }
            }
            if let Some(xdg) = env.get("XDG_CACHE_HOME") {
                let trimmed = xdg.trim();
                if !trimmed.is_empty() {
                    let xdg_path = PathBuf::from(trimmed);
                    push_if_dir(xdg_path.join("mesa_shader_cache"));
                    push_if_dir(xdg_path.join("mesa_shader_cache_sf"));
                }
            }
            if let Some(home) = env.get("HOME") {
                let trimmed = home.trim();
                if !trimmed.is_empty() {
                    let home_path = PathBuf::from(trimmed);
                    push_if_dir(home_path.join(".cache/mesa_shader_cache"));
                    push_if_dir(home_path.join(".cache/mesa_shader_cache_sf"));
                }
            }
        }
        Some(GpuVendor::Other(_)) => {}
    }

    dirs
}

/// Backward-compatible wrapper collecting shader cache directories (NVIDIA + common).
pub fn collect_shader_cache_dirs(
    layout: &PrefixLayout,
    env: &BTreeMap<String, String>,
    exe_dir: Option<&Path>,
) -> Vec<PathBuf> {
    collect_shader_cache_dirs_for_vendor(layout, env, exe_dir, None)
}

/// Issues non-blocking `POSIX_FADV_WILLNEED` hints for shader cache files under `dirs`
/// so the kernel populates the page cache before DXVK/vkd3d/NVIDIA reads them.
pub fn prewarm_shader_cache_dirs(dirs: &[PathBuf]) -> PrewarmStats {
    let mut stats = PrewarmStats::default();
    let mut stack: Vec<(PathBuf, usize)> = dirs.iter().cloned().map(|d| (d, 0)).collect();

    while let Some((dir, depth)) = stack.pop() {
        if stats.files_advised >= MAX_PREWARM_FILES || stats.bytes_advised >= MAX_PREWARM_BYTES {
            break;
        }
        let Ok(entries) = fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            if stats.files_advised >= MAX_PREWARM_FILES || stats.bytes_advised >= MAX_PREWARM_BYTES
            {
                break;
            }
            let path = entry.path();
            let Ok(meta) = fs::symlink_metadata(&path) else {
                continue;
            };
            if meta.file_type().is_dir() {
                if depth < 3 {
                    stack.push((path, depth + 1));
                }
                continue;
            }
            if !meta.file_type().is_file() || !is_shader_cache_file(&path) {
                continue;
            }
            let file_len = meta.len();
            if file_len == 0 {
                continue;
            }
            let Ok(file) = File::open(&path) else {
                continue;
            };
            let remaining = MAX_PREWARM_BYTES.saturating_sub(stats.bytes_advised);
            let advise_len = file_len.min(remaining);
            let rc = unsafe {
                libc::posix_fadvise(
                    file.as_raw_fd(),
                    0,
                    advise_len as libc::off_t,
                    libc::POSIX_FADV_WILLNEED,
                )
            };
            if rc == 0 {
                stats.files_advised += 1;
                stats.bytes_advised = stats.bytes_advised.saturating_add(advise_len);
            }
        }
    }

    stats
}

/// Spawns a low-priority background thread to pre-warm shader cache files in the kernel page cache.
pub fn spawn_shader_cache_prewarm(dirs: Vec<PathBuf>) -> Option<JoinHandle<PrewarmStats>> {
    if dirs.is_empty() {
        return None;
    }

    thread::Builder::new()
        .name("hx-shader-warm".to_string())
        .spawn(move || {
            unsafe {
                let param = libc::sched_param { sched_priority: 0 };
                let _ = libc::sched_setscheduler(0, libc::SCHED_BATCH, &param);
                let _ = libc::setpriority(libc::PRIO_PROCESS, 0, 10);
            }
            prewarm_shader_cache_dirs(&dirs)
        })
        .ok()
}
