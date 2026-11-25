use std::collections::BTreeMap;

use hyperx_proton::gpu::{
    DxvkBackend, GpuDeviceType, GpuProbeResult, GpuProfile, GpuVendor,
    choose_dxvk_backend, probe_primary_gpu, select_primary_gpu,
};

fn mock_gpu(
    vendor_id: u32,
    device_id: u32,
    name: &str,
    device_type: GpuDeviceType,
    major: u32,
    minor: u32,
) -> GpuProfile {
    GpuProfile {
        vendor: GpuVendor::from_vendor_id(vendor_id),
        vendor_id,
        device_id,
        device_name: name.to_string(),
        device_type,
        api_major: major,
        api_minor: minor,
        api_patch: 0,
        has_gpl: minor >= 3,
        has_present_wait: minor >= 3,
        has_shader_module_id: vendor_id == 0x10DE && minor >= 3,
    }
}

#[test]
fn select_primary_gpu_prefers_discrete_ignores_llvmpipe_and_honors_filters() {
    let devices = vec![
        mock_gpu(0x10005, 0x0000, "llvmpipe (LLVM 23)", GpuDeviceType::Cpu, 1, 4),
        mock_gpu(0x8086, 0x4680, "Intel UHD Graphics 730", GpuDeviceType::Integrated, 1, 3),
        mock_gpu(0x10DE, 0x21C4, "NVIDIA GeForce GTX 1660 SUPER", GpuDeviceType::Discrete, 1, 4),
        mock_gpu(0x1002, 0x744C, "AMD Radeon RX 7900 XTX", GpuDeviceType::Discrete, 1, 3),
    ];

    // Default: Discrete with highest Vulkan API wins (1.4 > 1.3), never Cpu
    let picked = select_primary_gpu(&devices, None, None).unwrap();
    assert_eq!(picked.vendor, GpuVendor::Nvidia);
    assert_eq!(picked.device_id, 0x21C4);

    // DXVK_FILTER_DEVICE_NAME selects Intel iGPU explicitly
    let intel = select_primary_gpu(&devices, Some("uhd graphics"), None).unwrap();
    assert_eq!(intel.vendor, GpuVendor::Intel);

    // MESA_VK_DEVICE_SELECT with trailing '!' selects AMD RX 7900 XTX
    let amd = select_primary_gpu(&devices, None, Some("1002:744c!")).unwrap();
    assert_eq!(amd.vendor, GpuVendor::Amd);
    assert_eq!(amd.device_id, 0x744C);

    // MESA_VK_DEVICE_SELECT without trailing '!' also works (case-insensitive)
    let amd_no_bang = select_primary_gpu(&devices, None, Some("1002:744C")).unwrap();
    assert_eq!(amd_no_bang.vendor, GpuVendor::Amd);
    assert_eq!(amd_no_bang.device_id, 0x744C);

    // Only Cpu available still falls back to Cpu instead of None
    let cpu_only = select_primary_gpu(&devices[..1], None, None).unwrap();
    assert_eq!(cpu_only.device_type, GpuDeviceType::Cpu);
}

#[test]
fn choose_dxvk_backend_auto_selects_sarek_for_vulkan_1_2_and_handles_overrides() {
    let vk12_intel = GpuProbeResult::Detected(mock_gpu(
        0x8086, 0x1912, "Intel HD 530", GpuDeviceType::Integrated, 1, 2,
    ));
    let vk14_nvidia = GpuProbeResult::Detected(mock_gpu(
        0x10DE, 0x21C4, "GTX 1660 SUPER", GpuDeviceType::Discrete, 1, 4,
    ));
    let unavail = GpuProbeResult::Unavailable("no vulkan".to_string());

    // Vulkan 1.2 + bundled -> Sarek
    let sel = choose_dxvk_backend(&vk12_intel, None, None, true);
    assert_eq!(sel.backend, DxvkBackend::Sarek);
    assert!(sel.warning.is_none());

    // Vulkan 1.2 + NOT bundled -> fallback Standard with warning
    let sel_missing = choose_dxvk_backend(&vk12_intel, None, None, false);
    assert_eq!(sel_missing.backend, DxvkBackend::Standard);
    assert!(sel_missing.warning.is_some());

    // Vulkan 1.4 + auto -> Standard
    assert_eq!(choose_dxvk_backend(&vk14_nvidia, None, None, true).backend, DxvkBackend::Standard);

    // Rule override sarek wins over auto on Vulkan 1.4, env override standard beats rule
    assert_eq!(
        choose_dxvk_backend(&vk14_nvidia, None, Some(DxvkBackend::Sarek), true).backend,
        DxvkBackend::Sarek
    );
    assert_eq!(
        choose_dxvk_backend(&vk14_nvidia, Some("standard"), Some(DxvkBackend::Sarek), true).backend,
        DxvkBackend::Standard
    );

    // Unavailable GPU -> fail-soft Standard
    assert_eq!(choose_dxvk_backend(&unavail, None, None, true).backend, DxvkBackend::Standard);
}

#[test]
fn probe_primary_gpu_respects_no_probe_env_and_is_fail_soft() {
    let mut env = BTreeMap::new();
    env.insert("HYPERX_PROTON_NO_GPU_PROBE".to_string(), "1".to_string());
    assert!(matches!(probe_primary_gpu(&env), GpuProbeResult::Unavailable(_)));
}

use hyperx_proton::dll_overrides::merge_dll_overrides_with_extras;
use hyperx_proton::rules::{
    EnsureFileRule, apply_ensure_files, builtin_rules, evaluate_rules, load_rules,
    should_use_steam_shim,
};

#[test]
fn builtin_rules_match_wuwa_and_hoyoverse_with_conditional_steam_shim() {
    let rules = builtin_rules();
    let empty_env = BTreeMap::new();

    let wuwa = evaluate_rules(&rules, "/games/wuwa/Client/Binaries/Win64/Client-Win64-Shipping.exe", &empty_env);
    assert_eq!(wuwa.matched_rule_ids, vec!["wuthering-waves"]);
    assert_eq!(wuwa.steam_game_id.as_deref(), Some("3513350"));
    assert!(wuwa.dll_overrides.contains(&"KRSDKExternal.exe=d".to_string()));
    assert_eq!(wuwa.use_steam_shim, Some(false));
    assert!(!should_use_steam_shim(&wuwa, &empty_env, true));
    assert_eq!(wuwa.ensure_files.len(), 2);

    let hsr = evaluate_rules(&rules, "/games/HSR/StarRail.exe", &empty_env);
    assert_eq!(hsr.matched_rule_ids, vec!["honkai-star-rail"]);
    assert!(should_use_steam_shim(&hsr, &empty_env, true));

    let mut no_shim = BTreeMap::new();
    no_shim.insert("HYPERX_PROTON_NO_STEAM_SHIM".to_string(), "1".to_string());
    assert!(!should_use_steam_shim(&hsr, &no_shim, true));

    let mut force_shim = BTreeMap::new();
    force_shim.insert("HYPERX_PROTON_FORCE_STEAM_SHIM".to_string(), "1".to_string());
    assert!(should_use_steam_shim(&wuwa, &force_shim, true));

    // Unknown game does NOT trigger steam shim by default
    let other = evaluate_rules(&rules, "/games/Cyberpunk/bin/x64/Cyberpunk2077.exe", &empty_env);
    assert!(other.matched_rule_ids.is_empty());
    assert!(!should_use_steam_shim(&other, &empty_env, true));
}

#[test]
fn custom_rules_support_multi_match_merge_same_id_replace_and_corrupt_json_fail_soft() {
    let tmp = tempfile::tempdir().unwrap();
    let rules_dir = tmp.path().join("rules");
    std::fs::create_dir_all(&rules_dir).unwrap();

    // 1. Valid custom rule with NEW id matching WuWa -> merges with builtin without dropping usebitmap.flag
    std::fs::write(
        rules_dir.join("01-wuwa-tweak.json"),
        r#"{
            "id": "my-wuwa-tweak",
            "name": "My WuWa Tweak",
            "match_criteria": { "exe_any": ["client-win64-shipping.exe"] },
            "env": { "DXVK_CONFIG": "dxgi.maxFrameRate = 60" },
            "dll_overrides": ["KRSDKExternal.exe=n", "customfoo=d"],
            "dxvk_backend": "sarek"
        }"#,
    ).unwrap();

    // 2. Valid array file replacing builtin genshin-impact by same id
    std::fs::write(
        rules_dir.join("02-override-genshin.json"),
        r#"[{
            "id": "genshin-impact",
            "name": "Custom Genshin",
            "match_criteria": { "exe_any": ["genshinimpact.exe"] },
            "use_steam_shim": false
        }]"#,
    ).unwrap();

    // 3. Corrupt JSON must emit warning and NOT break loading
    std::fs::write(rules_dir.join("03-broken.json"), "{ invalid json").unwrap();

    let mut env = BTreeMap::new();
    env.insert("HYPERX_PROTON_RULES_DIR".to_string(), rules_dir.display().to_string());

    let loaded = load_rules(&env);
    assert_eq!(loaded.builtin_count, 4);
    assert_eq!(loaded.custom_count, 2);
    assert_eq!(loaded.warnings.len(), 1);

    let eval_wuwa = evaluate_rules(&loaded.rules, "/games/wuwa/Client-Win64-Shipping.exe", &env);
    assert_eq!(eval_wuwa.matched_rule_ids, vec!["wuthering-waves", "my-wuwa-tweak"]);
    assert_eq!(eval_wuwa.steam_game_id.as_deref(), Some("3513350"));
    assert_eq!(eval_wuwa.ensure_files.len(), 2);
    assert_eq!(eval_wuwa.env.get("DXVK_CONFIG").map(String::as_str), Some("dxgi.maxFrameRate = 60"));
    assert_eq!(eval_wuwa.dxvk_backend, Some(DxvkBackend::Sarek));

    // Custom rule dll_override wins over built-in, user wins over both
    let merged = merge_dll_overrides_with_extras(Some("customfoo=b"), &eval_wuwa.dll_overrides, false);
    assert!(merged.starts_with("customfoo=b;"));
    assert!(merged.contains("KRSDKExternal.exe=n"));
    assert!(!merged.contains("KRSDKExternal.exe=d"));

    // Replaced genshin-impact rule now has use_steam_shim = false
    let eval_gi = evaluate_rules(&loaded.rules, "/games/GenshinImpact.exe", &env);
    assert_eq!(eval_gi.use_steam_shim, Some(false));
}

#[test]
fn apply_ensure_files_creates_valid_flag_and_rejects_path_traversal() {
    let tmp = tempfile::tempdir().unwrap();
    let exe_dir = tmp.path().join("game");
    let sdk = exe_dir.join("ThirdParty/KrPcSdk_Global/KRSDKRes/KRSDKWebViewOSR");
    std::fs::create_dir_all(&sdk).unwrap();
    std::fs::write(sdk.join("KRWebClient.exe"), b"bin").unwrap();

    let actions = vec![
        EnsureFileRule {
            if_exists_rel: "ThirdParty/KrPcSdk_Global/KRSDKRes/KRSDKWebViewOSR/KRWebClient.exe".to_string(),
            create_rel: "ThirdParty/KrPcSdk_Global/KRSDKRes/KRSDKWebViewOSR/usebitmap.flag".to_string(),
        },
        EnsureFileRule {
            if_exists_rel: "ThirdParty/KrPcSdk_Global/KRSDKRes/KRSDKWebViewOSR/KRWebClient.exe".to_string(),
            create_rel: "../escaped.flag".to_string(),
        },
        EnsureFileRule {
            if_exists_rel: "ThirdParty/KrPcSdk_Global/KRSDKRes/KRSDKWebViewOSR/KRWebClient.exe".to_string(),
            create_rel: tmp.path().join("abs.flag").display().to_string(),
        },
    ];

    let created = apply_ensure_files(&exe_dir, &actions);
    assert_eq!(created, 1);
    assert!(sdk.join("usebitmap.flag").is_file());
    assert!(!tmp.path().join("escaped.flag").exists());
    assert!(!tmp.path().join("abs.flag").exists());
}

use hyperx_proton::config::resolve_prefix_layout;
use hyperx_proton::rules::RuleEvaluation;
use hyperx_proton::runtime::build_runtime_env_with_context;
use hyperx_proton::shader_warm::{collect_shader_cache_dirs_for_vendor, prewarm_shader_cache_dirs};

#[test]
fn runtime_env_applies_vendor_specific_cache_and_blocks_nvapi_on_amd_intel() {
    let runner = std::path::Path::new("/opt/hyperx-proton");
    let layout = resolve_prefix_layout(Some(std::path::Path::new("/tmp/SHARED")), None).unwrap();

    let mut host_env = BTreeMap::new();
    host_env.insert("PROTON_ENABLE_NVAPI".to_string(), "1".to_string());

    let amd = GpuProbeResult::Detected(mock_gpu(0x1002, 0x744C, "AMD RX 7900", GpuDeviceType::Discrete, 1, 3));
    let rt_amd = build_runtime_env_with_context(runner, &layout, &host_env, true, &amd, &RuleEvaluation::default());
    assert!(!rt_amd.nvapi_enabled);
    assert!(!rt_amd.env.get("WINEDLLOVERRIDES").unwrap().contains("nvapi64=n"));
    assert!(!rt_amd.env.contains_key("__GL_SHADER_DISK_CACHE"));
    assert_eq!(rt_amd.env.get("MESA_SHADER_CACHE_DISABLE").map(String::as_str), Some("false"));
    assert_eq!(rt_amd.env.get("MESA_SHADER_CACHE_MAX_SIZE").map(String::as_str), Some("2G"));

    let intel = GpuProbeResult::Detected(mock_gpu(0x8086, 0x4680, "Intel UHD 730", GpuDeviceType::Integrated, 1, 3));
    let rt_intel = build_runtime_env_with_context(runner, &layout, &host_env, true, &intel, &RuleEvaluation::default());
    assert!(!rt_intel.nvapi_enabled);
    assert_eq!(rt_intel.env.get("MESA_SHADER_CACHE_MAX_SIZE").map(String::as_str), Some("2G"));

    // HYPERX_PROTON_FORCE_NVAPI=1 overrides vendor guard
    let mut force_env = host_env.clone();
    force_env.insert("HYPERX_PROTON_FORCE_NVAPI".to_string(), "1".to_string());
    let rt_forced = build_runtime_env_with_context(runner, &layout, &force_env, true, &amd, &RuleEvaluation::default());
    assert!(rt_forced.nvapi_enabled);
    assert!(rt_forced.env.get("WINEDLLOVERRIDES").unwrap().contains("nvapi64=n"));

    // NVIDIA allows NVAPI and sets NVIDIA 2 GiB cache, not Mesa
    let nv = GpuProbeResult::Detected(mock_gpu(0x10DE, 0x21C4, "GTX 1660 SUPER", GpuDeviceType::Discrete, 1, 4));
    let rt_nv = build_runtime_env_with_context(runner, &layout, &host_env, true, &nv, &RuleEvaluation::default());
    assert!(rt_nv.nvapi_enabled);
    assert!(rt_nv.env.get("WINEDLLOVERRIDES").unwrap().contains("nvapi64=n"));
    assert_eq!(rt_nv.env.get("__GL_SHADER_DISK_CACHE_SIZE").map(String::as_str), Some("2147483647"));
    assert!(!rt_nv.env.contains_key("MESA_SHADER_CACHE_MAX_SIZE"));
}

#[test]
fn runtime_env_respects_user_over_rule_over_defaults_for_dxvk_config_and_opwr() {
    let runner = std::path::Path::new("/opt/hyperx-proton");
    let layout = resolve_prefix_layout(Some(std::path::Path::new("/tmp/SHARED")), None).unwrap();
    let nv = GpuProbeResult::Detected(mock_gpu(0x10DE, 0x21C4, "GTX 1660 SUPER", GpuDeviceType::Discrete, 1, 4));

    let mut rule_eval = RuleEvaluation::default();
    rule_eval.steam_game_id = Some("3513350".to_string());
    rule_eval.env.insert("WINE_DISABLE_VULKAN_OPWR".to_string(), "0".to_string());
    rule_eval.env.insert("DXVK_CONFIG".to_string(), "dxgi.maxFrameRate = 60".to_string());

    // Without user override: rule wins over runtime defaults AND dxvk.numCompilerThreads appends cleanly
    let mut host_env = BTreeMap::new();
    host_env.insert("SteamGameId".to_string(), "0".to_string());
    let rt = build_runtime_env_with_context(runner, &layout, &host_env, true, &nv, &rule_eval);
    assert_eq!(rt.env.get("SteamGameId").map(String::as_str), Some("3513350"));
    assert_eq!(rt.env.get("WINE_DISABLE_VULKAN_OPWR").map(String::as_str), Some("0"));
    let dxvk_cfg = rt.env.get("DXVK_CONFIG").unwrap();
    assert!(dxvk_cfg.contains("dxgi.maxFrameRate = 60"), "got: {dxvk_cfg}");
    assert!(dxvk_cfg.contains("dxvk.numCompilerThreads = "), "got: {dxvk_cfg}");

    // With explicit user override: user beats rule
    host_env.insert("WINE_DISABLE_VULKAN_OPWR".to_string(), "1".to_string());
    host_env.insert("SteamGameId".to_string(), "999999".to_string());
    let rt_user = build_runtime_env_with_context(runner, &layout, &host_env, true, &nv, &rule_eval);
    assert_eq!(rt_user.env.get("WINE_DISABLE_VULKAN_OPWR").map(String::as_str), Some("1"));
    assert_eq!(rt_user.env.get("SteamGameId").map(String::as_str), Some("999999"));
}

#[test]
fn shader_warm_collects_and_advises_mesa_cache_for_amd_and_intel() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join("home");
    let mesa_sf = home.join(".cache/mesa_shader_cache_sf");
    let nv_gl = home.join(".cache/nvidia/GLCache");
    std::fs::create_dir_all(&mesa_sf).unwrap();
    std::fs::create_dir_all(&nv_gl).unwrap();
    std::fs::write(mesa_sf.join("radv.foz"), vec![1u8; 4096]).unwrap();
    std::fs::write(mesa_sf.join("index.idx"), vec![2u8; 512]).unwrap();
    std::fs::write(nv_gl.join("nv.bin"), vec![3u8; 4096]).unwrap();

    let layout = resolve_prefix_layout(Some(&tmp.path().join("SHARED")), None).unwrap();
    let mut env = BTreeMap::new();
    env.insert("HOME".to_string(), home.display().to_string());

    let amd_dirs = collect_shader_cache_dirs_for_vendor(&layout, &env, None, Some(GpuVendor::Amd));
    assert!(amd_dirs.contains(&mesa_sf));
    assert!(!amd_dirs.contains(&nv_gl));
    let stats = prewarm_shader_cache_dirs(&amd_dirs);
    assert_eq!(stats.files_advised, 2);
    assert_eq!(stats.bytes_advised, 4608);
}

use hyperx_proton::prefix::{prefix_needs_wineboot, sync_prefix_dlls_with_options};
use std::os::unix::fs::PermissionsExt;
use std::process::Command;

#[test]
fn prefix_switches_dxvk_backend_without_wineboot_and_removes_stale_nvapi() {
    let tmp = tempfile::tempdir().unwrap();
    let runner = tmp.path().join("runner");
    let compat = tmp.path().join("SHARED");
    let pfx = compat.join("pfx");
    std::fs::create_dir_all(pfx.join("drive_c/windows/system32")).unwrap();
    std::fs::write(pfx.join("system.reg"), "KEEP_SYS").unwrap();
    std::fs::write(pfx.join("user.reg"), "KEEP_USER").unwrap();

    for (rel, content) in [
        ("files/lib/dxvk/x86_64-windows/d3d11.dll", b"STD_D3D11".as_slice()),
        ("files/lib/dxvk/x86_64-windows/dxgi.dll", b"STD_DXGI".as_slice()),
        ("files/lib/dxvk-sarek/x86_64-windows/d3d11.dll", b"SAREK_D3D11".as_slice()),
        ("files/lib/dxvk-sarek/x86_64-windows/dxgi.dll", b"SAREK_DXGI".as_slice()),
        ("files/lib/vkd3d-proton/x86_64-windows/d3d12.dll", b"VKD3D12".as_slice()),
        ("files/lib/vkd3d-proton/x86_64-windows/d3d12core.dll", b"VKD3DCORE".as_slice()),
        ("files/lib/nvapi/x86_64-windows/nvapi64.dll", b"NVAPI64".as_slice()),
    ] {
        let p = runner.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, content).unwrap();
    }

    let layout = resolve_prefix_layout(Some(&compat), None).unwrap();
    let r1 = sync_prefix_dlls_with_options(&runner, &layout, "v1", true, DxvkBackend::Standard).unwrap();
    assert!(r1.dlls_updated);
    assert_eq!(std::fs::read(pfx.join("drive_c/windows/system32/d3d11.dll")).unwrap(), b"STD_D3D11");
    assert!(pfx.join("drive_c/windows/system32/nvapi64.dll").is_file());

    // Switch to Sarek & disable NVAPI (e.g. AMD/Intel): NO wineboot needed, DLLs swapped, stale nvapi64.dll deleted!
    assert!(!prefix_needs_wineboot(&runner, &layout, "v1"));
    let r2 = sync_prefix_dlls_with_options(&runner, &layout, "v1", false, DxvkBackend::Sarek).unwrap();
    assert!(r2.dlls_updated);
    assert!(!r2.needs_wineboot);
    assert_eq!(std::fs::read(pfx.join("drive_c/windows/system32/d3d11.dll")).unwrap(), b"SAREK_D3D11");
    assert!(!pfx.join("drive_c/windows/system32/nvapi64.dll").exists());
    assert_eq!(std::fs::read_to_string(pfx.join(".hyperx-proton-dxvk-backend")).unwrap().trim(), "sarek");
    assert_eq!(std::fs::read_to_string(pfx.join("user.reg")).unwrap(), "KEEP_USER");

    // Repeating with same backend is idempotent
    let r3 = sync_prefix_dlls_with_options(&runner, &layout, "v1", false, DxvkBackend::Sarek).unwrap();
    assert!(!r3.dlls_updated);
}

#[test]
fn cli_launches_wuwa_without_steam_shim_and_hsr_with_steam_shim_and_doctor_reports_phase2() {
    let tmp = tempfile::tempdir().unwrap();
    let runner = tmp.path().join("runner");
    let compat = tmp.path().join("SHARED");
    let log_file = tmp.path().join("calls.log");
    let rules_dir = tmp.path().join("empty-rules");
    std::fs::create_dir_all(&rules_dir).unwrap();

    // Minimal stub runner with dxvk, dxvk-sarek, vkd3d, steam.exe shim
    std::fs::create_dir_all(runner.join("files/bin")).unwrap();
    std::fs::write(
        runner.join("hyperx-native.json"),
        r#"{"schema_version":1,"runner_mode":"native","requires_steamrt":false,"name":"HyperX-Proton","version":"v1","arch":"x86_64"}"#,
    ).unwrap();
    for (rel, content) in [
        ("files/lib/dxvk/x86_64-windows/d3d11.dll", b"STD11".as_slice()),
        ("files/lib/dxvk/x86_64-windows/dxgi.dll", b"STDX".as_slice()),
        ("files/lib/dxvk-sarek/x86_64-windows/d3d11.dll", b"SAR11".as_slice()),
        ("files/lib/dxvk-sarek/x86_64-windows/dxgi.dll", b"SARX".as_slice()),
        ("files/lib/vkd3d-proton/x86_64-windows/d3d12.dll", b"V12".as_slice()),
        ("files/lib/vkd3d-proton/x86_64-windows/d3d12core.dll", b"V12C".as_slice()),
        ("files/lib/wine/x86_64-windows/steam.exe", b"SHIM".as_slice()),
    ] {
        let p = runner.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, content).unwrap();
    }
    let wine_stub = runner.join("files/bin/wine");
    std::fs::write(&wine_stub, "#!/usr/bin/env bash\necho \"wine:$*\" >> \"$TEST_CALL_LOG\"\n").unwrap();
    std::fs::set_permissions(&wine_stub, std::fs::Permissions::from_mode(0o755)).unwrap();
    let ws_stub = runner.join("files/bin/wineserver");
    std::fs::write(&ws_stub, "#!/usr/bin/env bash\necho \"ws:$*\" >> \"$TEST_CALL_LOG\"\n").unwrap();
    std::fs::set_permissions(&ws_stub, std::fs::Permissions::from_mode(0o755)).unwrap();

    // WuWa fake directory with KRWebClient.exe
    let wuwa_dir = tmp.path().join("wuwa/Client/Binaries/Win64");
    let webview = wuwa_dir.join("ThirdParty/KrPcSdk_Global/KRSDKRes/KRSDKWebViewOSR");
    std::fs::create_dir_all(&webview).unwrap();
    std::fs::write(webview.join("KRWebClient.exe"), b"cef").unwrap();
    let wuwa_exe = wuwa_dir.join("Client-Win64-Shipping.exe");
    std::fs::write(&wuwa_exe, b"pe").unwrap();

    let bin = env!("CARGO_BIN_EXE_hyperx-proton");
    let st1 = Command::new(bin)
        .args(["run", wuwa_exe.to_str().unwrap()])
        .env("HYPERX_PROTON_ROOT", &runner)
        .env("STEAM_COMPAT_DATA_PATH", &compat)
        .env("HYPERX_PROTON_RULES_DIR", &rules_dir)
        .env("HYPERX_PROTON_NO_GPU_PROBE", "1")
        .env("TEST_CALL_LOG", &log_file)
        .status()
        .unwrap();
    assert!(st1.success());
    assert!(webview.join("usebitmap.flag").is_file());

    // Launch HSR -> MUST prepend C:\windows\system32\steam.exe
    let st2 = Command::new(bin)
        .args(["run", "/tmp/HSR/StarRail.exe"])
        .env("HYPERX_PROTON_ROOT", &runner)
        .env("STEAM_COMPAT_DATA_PATH", &compat)
        .env("HYPERX_PROTON_RULES_DIR", &rules_dir)
        .env("HYPERX_PROTON_NO_GPU_PROBE", "1")
        .env("TEST_CALL_LOG", &log_file)
        .status()
        .unwrap();
    assert!(st2.success());

    let log = std::fs::read_to_string(&log_file).unwrap();
    assert!(log.contains(&format!("wine:{}", wuwa_exe.display())), "log:\n{log}");
    assert!(!log.contains(&format!("steam.exe {}", wuwa_exe.display())), "WuWa must not use steam shim:\n{log}");
    assert!(log.contains("wine:C:\\windows\\system32\\steam.exe /tmp/HSR/StarRail.exe"), "HSR must use steam shim:\n{log}");

    // Doctor reports Phase 2 checks
    let doc = Command::new(bin)
        .arg("doctor")
        .env("HYPERX_PROTON_ROOT", &runner)
        .env("HYPERX_PROTON_RULES_DIR", &rules_dir)
        .output()
        .unwrap();
    let out = String::from_utf8_lossy(&doc.stdout);
    assert!(out.contains("gpu:vulkan"), "doctor:\n{out}");
    assert!(out.contains("gpu:dxvk-backend"), "doctor:\n{out}");
    assert!(out.contains("dxvk-sarek:d3d11.dll"), "doctor:\n{out}");
    assert!(out.contains("rules:engine"), "doctor:\n{out}");
}
