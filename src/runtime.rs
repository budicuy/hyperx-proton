use crate::config::PrefixLayout;
use crate::dll_overrides::merge_dll_overrides_with_extras;
use crate::gpu::{GpuProbeResult, GpuVendor};
use crate::rules::RuleEvaluation;
use crate::topology::{compute_shader_compiler_threads, detect_physical_cores};
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

/// Returns true if `/dev/ntsync` is present and openable on the host kernel.
pub fn is_ntsync_available() -> bool {
    File::open("/dev/ntsync").is_ok()
}

fn is_env_enabled(host_env: &BTreeMap<String, String>, key: &str) -> bool {
    host_env.get(key).map(String::as_str) == Some("1")
}

/// Builds the Wine execution environment, preserving host/game overrides while
/// configuring NTSYNC/FSYNC, Wayland, `PATH`, `WINEDLLOVERRIDES`, vendor-aware
/// shader caches, and game rule overrides (`User > Rule > Defaults`).
pub fn build_runtime_env_with_context(
    runner_root: &Path,
    layout: &PrefixLayout,
    host_env: &BTreeMap<String, String>,
    ntsync_available: bool,
    gpu_probe: &GpuProbeResult,
    rule_eval: &RuleEvaluation,
) -> RuntimeEnv {
    let mut env = host_env.clone();

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

    let force_nvapi = is_env_enabled(&env, "HYPERX_PROTON_FORCE_NVAPI");
    let want_nvapi = force_nvapi
        || is_env_enabled(&env, "PROTON_ENABLE_NVAPI")
        || is_env_enabled(&env, "HYPERX_PROTON_NVAPI");
    let nvapi_allowed = match gpu_probe.vendor() {
        Some(GpuVendor::Amd | GpuVendor::Intel) => force_nvapi,
        _ => true,
    };
    let nvapi_enabled = want_nvapi && nvapi_allowed;

    let merged_overrides = merge_dll_overrides_with_extras(
        env.get("WINEDLLOVERRIDES").map(String::as_str),
        &rule_eval.dll_overrides,
        nvapi_enabled,
    );
    env.insert("WINEDLLOVERRIDES".to_string(), merged_overrides);

    let no_ntsync = is_env_enabled(&env, "PROTON_NO_NTSYNC");
    if ntsync_available && !no_ntsync {
        env.insert("PROTON_USE_NTSYNC".to_string(), "1".to_string());
        env.insert("WINEFSYNC".to_string(), "0".to_string());
        env.insert("WINEESYNC".to_string(), "0".to_string());
    } else {
        env.insert("PROTON_USE_NTSYNC".to_string(), "0".to_string());
        env.insert("WINEFSYNC".to_string(), "1".to_string());
    }

    // Avoid Vulkan OPWR login-webview input stalls and d3dcompiler issues
    // (matches Proton-CachyOS fixes for WuWa, Genshin, and ZZZ) unless overridden.
    env.entry("WINE_DISABLE_VULKAN_OPWR".to_string())
        .or_insert_with(|| "1".to_string());

    // Size DXVK/vkd3d shader compiler threads from host physical core topology so
    // background shader compilation does not starve the game's RenderThread.
    let shader_threads = env
        .get("HYPERX_PROTON_SHADER_THREADS")
        .and_then(|s| s.trim().parse::<usize>().ok())
        .filter(|&n| n > 0)
        .unwrap_or_else(|| compute_shader_compiler_threads(detect_physical_cores()));
    let shader_threads_str = shader_threads.to_string();

    // DXVK v3.x reads `dxvk.numCompilerThreads` from `DXVK_CONFIG` (not a standalone env var).
    let existing_dxvk_config = env
        .get("DXVK_CONFIG")
        .map(String::as_str)
        .unwrap_or("")
        .trim();
    if !existing_dxvk_config
        .to_ascii_lowercase()
        .contains("dxvk.numcompilerthreads")
    {
        let merged_dxvk_config = if existing_dxvk_config.is_empty() {
            format!("dxvk.numCompilerThreads = {shader_threads}")
        } else {
            format!(
                "{}; dxvk.numCompilerThreads = {shader_threads}",
                existing_dxvk_config.trim_end_matches(';')
            )
        };
        env.insert("DXVK_CONFIG".to_string(), merged_dxvk_config);
    }

    env.entry("VKD3D_WORKER_THREADS".to_string())
        .or_insert_with(|| shader_threads_str);
    env.entry("DXVK_STATE_CACHE".to_string())
        .or_insert_with(|| "1".to_string());

    match gpu_probe.vendor() {
        Some(GpuVendor::Amd | GpuVendor::Intel) => {
            env.entry("MESA_SHADER_CACHE_DISABLE".to_string())
                .or_insert_with(|| "false".to_string());
            env.entry("MESA_SHADER_CACHE_MAX_SIZE".to_string())
                .or_insert_with(|| "2G".to_string());
        }
        Some(GpuVendor::Nvidia) | Some(GpuVendor::Other(_)) | None => {
            // Ensure NVIDIA shader disk cache is enabled with a safe 2 GiB (INT32_MAX) ceiling
            // so vkd3d-proton's shaderModuleIdentifier cache does not thrash at NVIDIA's 1 GiB default
            // while avoiding the 32-bit overflow bug on >4 GiB values.
            env.entry("__GL_SHADER_DISK_CACHE".to_string())
                .or_insert_with(|| "1".to_string());
            env.entry("__GL_SHADER_DISK_CACHE_SKIP_CLEANUP".to_string())
                .or_insert_with(|| "1".to_string());
            env.entry("__GL_SHADER_DISK_CACHE_SIZE".to_string())
                .or_insert_with(|| "2147483647".to_string());
        }
    }

    // Silence high-frequency DXVK/vkd3d info log spam (e.g. disk-cache flush lines)
    // during gameplay unless the user explicitly enables debug logging.
    env.entry("DXVK_LOG_LEVEL".to_string())
        .or_insert_with(|| "error".to_string());
    env.entry("VKD3D_DEBUG".to_string())
        .or_insert_with(|| "none".to_string());

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

/// Backward-compatible wrapper using default GPU probe (`Unavailable`) and no rules.
pub fn build_runtime_env(
    runner_root: &Path,
    layout: &PrefixLayout,
    host_env: &BTreeMap<String, String>,
    ntsync_available: bool,
) -> RuntimeEnv {
    build_runtime_env_with_context(
        runner_root,
        layout,
        host_env,
        ntsync_available,
        &GpuProbeResult::Unavailable("not probed".to_string()),
        &RuleEvaluation::default(),
    )
}
