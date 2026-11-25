use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

/// Counts unique `(physical_package_id, core_id)` pairs from sysfs CPU topology entries.
pub fn count_unique_physical_cores(pairs: &[(u32, u32)]) -> usize {
    let unique: BTreeSet<(u32, u32)> = pairs.iter().copied().collect();
    unique.len()
}

/// Detects the host's physical CPU core count from `/sys/devices/system/cpu`,
/// falling back to `available_parallelism() / 2` (at least 1).
pub fn detect_physical_cores() -> usize {
    detect_physical_cores_from_sysfs(Path::new("/sys/devices/system/cpu"))
        .filter(|&count| count > 0)
        .unwrap_or_else(|| {
            let logical = std::thread::available_parallelism()
                .map(|n| n.get())
                .unwrap_or(2);
            (logical / 2).max(1)
        })
}

/// Reads unique `(physical_package_id, core_id)` pairs under `cpu_root` (e.g. `/sys/devices/system/cpu`).
pub fn detect_physical_cores_from_sysfs(cpu_root: &Path) -> Option<usize> {
    let entries = fs::read_dir(cpu_root).ok()?;
    let mut pairs = Vec::new();

    for entry in entries.flatten() {
        let name = entry.file_name();
        let name_str = name.to_string_lossy();
        let Some(rest) = name_str.strip_prefix("cpu") else {
            continue;
        };
        if rest.is_empty() || !rest.bytes().all(|b| b.is_ascii_digit()) {
            continue;
        }

        let topo = entry.path().join("topology");
        let pkg = fs::read_to_string(topo.join("physical_package_id"))
            .ok()
            .and_then(|s| s.trim().parse::<u32>().ok());
        let core = fs::read_to_string(topo.join("core_id"))
            .ok()
            .and_then(|s| s.trim().parse::<u32>().ok());

        if let (Some(p), Some(c)) = (pkg, core) {
            pairs.push((p, c));
        }
    }

    let count = count_unique_physical_cores(&pairs);
    (count > 0).then_some(count)
}

/// Computes a conservative shader compiler thread budget that avoids starving
/// the game's RenderThread/GameThread while keeping shader compilation fast.
pub fn compute_shader_compiler_threads(physical_cores: usize) -> usize {
    match physical_cores {
        0..=2 => 1,
        3..=4 => 2,
        5..=8 => physical_cores - 2,
        _ => (physical_cores / 2).clamp(6, 8),
    }
}
