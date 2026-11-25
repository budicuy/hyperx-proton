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
