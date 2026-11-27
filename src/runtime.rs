use crate::config::PrefixLayout;
use crate::dll_overrides::merge_dll_overrides_with_extras;
use crate::rules::RuleEvaluation;
use std::collections::BTreeMap;
use std::fs::File;
use std::path::{Path, PathBuf};

/// Resolved runtime environment and Wine binary paths.
#[derive(Debug, Clone)]
pub struct RuntimeEnv {
    pub env: BTreeMap<String, String>,
    pub wine_bin: PathBuf,
    pub wineserver_bin: PathBuf,
    pub nvapi_enabled: bool,
}

/// Active Wine synchronization primitive tier.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SyncMode {
    Ntsync,
    Fsync,
    Esync,
    Plain,
}

impl SyncMode {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Ntsync => "ntsync",
            Self::Fsync => "fsync",
            Self::Esync => "esync",
            Self::Plain => "plain",
        }
    }
}

/// Returns true if `/dev/ntsync` is present and openable on the host kernel.
pub fn is_ntsync_available() -> bool {
    File::open("/dev/ntsync").is_ok()
}

/// Returns true if the host kernel supports `futex_waitv` (`SYS_futex_waitv = 449` on x86_64).
pub fn is_fsync_available() -> bool {
    const SYS_FUTEX_WAITV: libc::c_long = 449;
    let ret = unsafe {
        libc::syscall(
            SYS_FUTEX_WAITV,
            std::ptr::null::<libc::c_void>(),
            0usize,
            0u32,
            std::ptr::null::<libc::c_void>(),
            0i32,
        )
    };
    if ret < 0 {
        std::io::Error::last_os_error().raw_os_error() != Some(libc::ENOSYS)
    } else {
        true
    }
}

/// Resolves the 3-tier synchronization fallback (`ntsync -> fsync -> esync -> plain`).
pub fn resolve_sync_mode(
    ntsync_available: bool,
    fsync_available: bool,
    no_ntsync: bool,
    no_fsync: bool,
    no_esync: bool,
) -> SyncMode {
    if ntsync_available && !no_ntsync {
        SyncMode::Ntsync
    } else if fsync_available && !no_fsync {
        SyncMode::Fsync
    } else if !no_esync {
        SyncMode::Esync
    } else {
        SyncMode::Plain
    }
}

fn is_steam_runtime_segment(seg: &str) -> bool {
    seg.contains("steam-runtime") || seg.contains("ubuntu12_32") || seg.contains("ubuntu12_64")
}

/// Strips Steam Scout Runtime segments from `LD_LIBRARY_PATH` unless opted out,
/// restoring `SYSTEM_LD_LIBRARY_PATH` when present (or removing `LD_LIBRARY_PATH` if empty).
pub fn sanitize_steam_runtime_ld_library_path(
    ld_library_path: Option<&str>,
    system_ld_library_path: Option<&str>,
    keep_steam_runtime: bool,
) -> Option<String> {
    let raw = ld_library_path?;
    if keep_steam_runtime || !raw.split(':').any(is_steam_runtime_segment) {
        return Some(raw.to_string());
    }

    if let Some(sys) = system_ld_library_path {
        if sys.trim().is_empty() {
            return None;
        }
        return Some(sys.to_string());
    }

    let kept: Vec<&str> = raw
        .split(':')
        .filter(|seg| !seg.is_empty() && !is_steam_runtime_segment(seg))
        .collect();
    if kept.is_empty() {
        None
    } else {
        Some(kept.join(":"))
    }
}

fn is_env_enabled(host_env: &BTreeMap<String, String>, key: &str) -> bool {
    host_env.get(key).map(String::as_str) == Some("1")
}

/// Builds the Wine execution environment with explicit `ntsync` and `fsync` availability,
/// preserving host/game overrides while configuring 3-tier sync, Steam `LD_LIBRARY_PATH`
/// sanitization, Wayland, `PATH`, `WINEDLLOVERRIDES`, NVAPI opt-in, and
/// game rule overrides (`User > Rule > Defaults`).
pub fn build_runtime_env_with_sync(
    runner_root: &Path,
    layout: &PrefixLayout,
    host_env: &BTreeMap<String, String>,
    ntsync_available: bool,
    fsync_available: bool,
    rule_eval: &RuleEvaluation,
) -> RuntimeEnv {
    let mut env = host_env.clone();

    let keep_steam_rt = is_env_enabled(&env, "HYPERX_PROTON_KEEP_STEAM_RUNTIME_LD");
    match sanitize_steam_runtime_ld_library_path(
        env.get("LD_LIBRARY_PATH").map(String::as_str),
        env.get("SYSTEM_LD_LIBRARY_PATH").map(String::as_str),
        keep_steam_rt,
    ) {
        Some(sanitized) => {
            env.insert("LD_LIBRARY_PATH".to_string(), sanitized);
        }
        None => {
            env.remove("LD_LIBRARY_PATH");
        }
    }

    for (key, val) in &rule_eval.env {
        env.entry(key.clone()).or_insert_with(|| val.clone());
    }

    if let Some(rule_steam_id) = &rule_eval.steam_game_id {
        let should_fill = match env.get("SteamGameId").map(|s| s.trim()) {
            None | Some("") | Some("0") => true,
            Some(_) => false,
        };
        if should_fill {
            env.insert("SteamGameId".to_string(), rule_steam_id.clone());
        }
    }

    let runner_bin = runner_root.join("files/bin");
    let new_path = match env.get("PATH") {
        Some(existing) if !existing.is_empty() => format!("{}:{existing}", runner_bin.display()),
        _ => runner_bin.display().to_string(),
    };
    env.insert("PATH".to_string(), new_path);

    let wine_dll_path = runner_root.join("files/lib/wine");
    env.insert(
        "WINEDLLPATH".to_string(),
        wine_dll_path.display().to_string(),
    );

    env.insert(
        "WINEPREFIX".to_string(),
        layout.wineprefix.display().to_string(),
    );
    // Pure WoW64 runner (no 32-bit Unix libs): WINEARCH=wow64 tells wine-cachyos
    // loader to execute 32-bit Windows helpers via WoW64 thunks instead of
    // looking for a separate 32-bit i386-unix/wine binary.
    env.insert("WINEARCH".to_string(), "wow64".to_string());

    let nvapi_enabled = is_env_enabled(&env, "PROTON_ENABLE_NVAPI")
        || is_env_enabled(&env, "HYPERX_PROTON_NVAPI");

    let merged_overrides = merge_dll_overrides_with_extras(
        env.get("WINEDLLOVERRIDES").map(String::as_str),
        &rule_eval.dll_overrides,
        nvapi_enabled,
    );
    env.insert("WINEDLLOVERRIDES".to_string(), merged_overrides);

    let sync_mode = resolve_sync_mode(
        ntsync_available,
        fsync_available,
        is_env_enabled(&env, "PROTON_NO_NTSYNC"),
        is_env_enabled(&env, "PROTON_NO_FSYNC"),
        is_env_enabled(&env, "PROTON_NO_ESYNC"),
    );
    match sync_mode {
        SyncMode::Ntsync => {
            env.insert("PROTON_USE_NTSYNC".to_string(), "1".to_string());
            env.insert("WINEFSYNC".to_string(), "0".to_string());
            env.insert("WINEESYNC".to_string(), "0".to_string());
        }
        SyncMode::Fsync => {
            env.insert("PROTON_USE_NTSYNC".to_string(), "0".to_string());
            env.insert("WINEFSYNC".to_string(), "1".to_string());
            env.insert("WINEESYNC".to_string(), "0".to_string());
        }
        SyncMode::Esync => {
            env.insert("PROTON_USE_NTSYNC".to_string(), "0".to_string());
            env.insert("WINEFSYNC".to_string(), "0".to_string());
            env.insert("WINEESYNC".to_string(), "1".to_string());
        }
        SyncMode::Plain => {
            env.insert("PROTON_USE_NTSYNC".to_string(), "0".to_string());
            env.insert("WINEFSYNC".to_string(), "0".to_string());
            env.insert("WINEESYNC".to_string(), "0".to_string());
        }
    }

    // Avoid Vulkan OPWR login-webview input stalls and d3dcompiler issues
    // (matches Proton-CachyOS fixes for WuWa, Genshin, and ZZZ) unless overridden.
    env.entry("WINE_DISABLE_VULKAN_OPWR".to_string())
        .or_insert_with(|| "1".to_string());

    // Pin GPU/DXVK/VKD3D shader caches under <compat_data>/shadercache (or
    // STEAM_COMPAT_SHADER_PATH) unless the user/rule already specified an override.
    let shader_root = env
        .get("STEAM_COMPAT_SHADER_PATH")
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .map(PathBuf::from)
        .or_else(|| {
            layout
                .compat_data_dir
                .as_ref()
                .map(|dir| dir.join("shadercache"))
        });
    if let Some(root) = shader_root {
        env.entry("DXVK_SHADER_CACHE_PATH".to_string())
            .or_insert_with(|| root.join("dxvk").display().to_string());
        env.entry("DXVK_STATE_CACHE_PATH".to_string())
            .or_insert_with(|| root.join("dxvk").display().to_string());
        env.entry("VKD3D_SHADER_CACHE_PATH".to_string())
            .or_insert_with(|| root.join("vkd3d").display().to_string());
        env.entry("__GL_SHADER_DISK_CACHE".to_string())
            .or_insert_with(|| "1".to_string());
        env.entry("__GL_SHADER_DISK_CACHE_PATH".to_string())
            .or_insert_with(|| root.join("nvidia").display().to_string());
        env.entry("MESA_SHADER_CACHE_DISABLE".to_string())
            .or_insert_with(|| "false".to_string());
        env.entry("MESA_SHADER_CACHE_DIR".to_string())
            .or_insert_with(|| root.join("mesa").display().to_string());
    }

    if is_env_enabled(&env, "PROTON_ENABLE_WAYLAND")
        && env.get("WAYLAND_DISPLAY").is_some_and(|v| !v.is_empty())
    {
        env.insert("DISPLAY".to_string(), String::new());
    }

    RuntimeEnv {
        env,
        wine_bin: runner_bin.join("wine"),
        wineserver_bin: runner_bin.join("wineserver"),
        nvapi_enabled,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_layout(compat_data_dir: Option<PathBuf>) -> PrefixLayout {
        PrefixLayout {
            wineprefix: PathBuf::from("/tmp/compat/pfx"),
            compat_data_dir,
            lock_file: PathBuf::from("/tmp/compat/prefix.lock"),
        }
    }

    #[test]
    fn shader_cache_defaults_support_dxvk3_and_do_not_force_nvidia_skip_cleanup() {
        let layout = test_layout(Some(PathBuf::from("/tmp/compat")));
        let host_env = BTreeMap::new();
        let rt = build_runtime_env_with_sync(
            Path::new("/tmp/runner"),
            &layout,
            &host_env,
            true,
            true,
            &RuleEvaluation::default(),
        );

        assert_eq!(
            rt.env.get("DXVK_SHADER_CACHE_PATH").map(String::as_str),
            Some("/tmp/compat/shadercache/dxvk")
        );
        assert_eq!(
            rt.env.get("DXVK_STATE_CACHE_PATH").map(String::as_str),
            Some("/tmp/compat/shadercache/dxvk")
        );
        assert_eq!(
            rt.env.get("VKD3D_SHADER_CACHE_PATH").map(String::as_str),
            Some("/tmp/compat/shadercache/vkd3d")
        );
        assert_eq!(
            rt.env.get("__GL_SHADER_DISK_CACHE").map(String::as_str),
            Some("1")
        );
        assert_eq!(
            rt.env.get("__GL_SHADER_DISK_CACHE_PATH").map(String::as_str),
            Some("/tmp/compat/shadercache/nvidia")
        );
        assert_eq!(rt.env.get("__GL_SHADER_DISK_CACHE_SKIP_CLEANUP"), None);
        assert_eq!(
            rt.env.get("MESA_SHADER_CACHE_DISABLE").map(String::as_str),
            Some("false")
        );
        assert_eq!(
            rt.env.get("MESA_SHADER_CACHE_DIR").map(String::as_str),
            Some("/tmp/compat/shadercache/mesa")
        );
    }

    #[test]
    fn shader_cache_respects_launcher_provided_overrides() {
        let layout = test_layout(Some(PathBuf::from("/tmp/compat")));
        let mut host_env = BTreeMap::new();
        host_env.insert(
            "STEAM_COMPAT_SHADER_PATH".to_string(),
            "/tmp/per-game/wuthering-waves".to_string(),
        );
        host_env.insert(
            "DXVK_SHADER_CACHE_PATH".to_string(),
            "/custom/dxvk".to_string(),
        );
        host_env.insert(
            "__GL_SHADER_DISK_CACHE_SIZE".to_string(),
            "4294967296".to_string(),
        );
        host_env.insert("MESA_SHADER_CACHE_MAX_SIZE".to_string(), "4G".to_string());
        host_env.insert("MESA_SHADER_CACHE_DISABLE".to_string(), "true".to_string());

        let rt = build_runtime_env_with_sync(
            Path::new("/tmp/runner"),
            &layout,
            &host_env,
            true,
            true,
            &RuleEvaluation::default(),
        );

        assert_eq!(
            rt.env.get("STEAM_COMPAT_SHADER_PATH").map(String::as_str),
            Some("/tmp/per-game/wuthering-waves")
        );
        assert_eq!(
            rt.env.get("DXVK_SHADER_CACHE_PATH").map(String::as_str),
            Some("/custom/dxvk")
        );
        assert_eq!(
            rt.env.get("DXVK_STATE_CACHE_PATH").map(String::as_str),
            Some("/tmp/per-game/wuthering-waves/dxvk")
        );
        assert_eq!(
            rt.env.get("VKD3D_SHADER_CACHE_PATH").map(String::as_str),
            Some("/tmp/per-game/wuthering-waves/vkd3d")
        );
        assert_eq!(
            rt.env.get("__GL_SHADER_DISK_CACHE_PATH").map(String::as_str),
            Some("/tmp/per-game/wuthering-waves/nvidia")
        );
        assert_eq!(
            rt.env.get("__GL_SHADER_DISK_CACHE_SIZE").map(String::as_str),
            Some("4294967296")
        );
        assert_eq!(
            rt.env.get("MESA_SHADER_CACHE_DIR").map(String::as_str),
            Some("/tmp/per-game/wuthering-waves/mesa")
        );
        assert_eq!(
            rt.env.get("MESA_SHADER_CACHE_MAX_SIZE").map(String::as_str),
            Some("4G")
        );
        assert_eq!(
            rt.env.get("MESA_SHADER_CACHE_DISABLE").map(String::as_str),
            Some("true")
        );
    }
}
