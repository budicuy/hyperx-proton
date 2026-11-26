use std::collections::BTreeMap;

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
            "dll_overrides": ["KRSDKExternal.exe=n", "customfoo=d"]
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
use hyperx_proton::runtime::build_runtime_env_with_sync;

#[test]
fn runtime_env_stays_neutral_and_supports_nvapi_opt_in() {
    let runner = std::path::Path::new("/opt/hyperx-proton");
    let layout = resolve_prefix_layout(Some(std::path::Path::new("/tmp/SHARED")), None).unwrap();

    let host_env = BTreeMap::new();
    let rt_default = build_runtime_env_with_sync(
        runner,
        &layout,
        &host_env,
        true,
        true,
        &RuleEvaluation::default(),
    );
    assert!(!rt_default.nvapi_enabled);
    assert!(!rt_default.env.get("WINEDLLOVERRIDES").unwrap().contains("nvapi64=n"));
    for key in [
        "DXVK_CONFIG",
        "VKD3D_WORKER_THREADS",
        "DXVK_STATE_CACHE",
        "MESA_SHADER_CACHE_DISABLE",
        "MESA_SHADER_CACHE_MAX_SIZE",
        "__GL_SHADER_DISK_CACHE",
        "__GL_SHADER_DISK_CACHE_SKIP_CLEANUP",
        "__GL_SHADER_DISK_CACHE_SIZE",
        "DXVK_LOG_LEVEL",
        "VKD3D_DEBUG",
    ] {
        assert!(!rt_default.env.contains_key(key), "must not auto-inject {key}");
    }

    let mut nvapi_env = BTreeMap::new();
    nvapi_env.insert("PROTON_ENABLE_NVAPI".to_string(), "1".to_string());
    let rt_nvapi = build_runtime_env_with_sync(
        runner,
        &layout,
        &nvapi_env,
        true,
        true,
        &RuleEvaluation::default(),
    );
    assert!(rt_nvapi.nvapi_enabled);
    assert!(rt_nvapi.env.get("WINEDLLOVERRIDES").unwrap().contains("nvapi64=n"));

    let mut hyperx_nvapi_env = BTreeMap::new();
    hyperx_nvapi_env.insert("HYPERX_PROTON_NVAPI".to_string(), "1".to_string());
    let rt_hyperx_nvapi = build_runtime_env_with_sync(
        runner,
        &layout,
        &hyperx_nvapi_env,
        true,
        true,
        &RuleEvaluation::default(),
    );
    assert!(rt_hyperx_nvapi.nvapi_enabled);
    assert!(rt_hyperx_nvapi.env.get("WINEDLLOVERRIDES").unwrap().contains("nvapi64=n"));
}

#[test]
fn runtime_env_respects_user_over_rule_over_defaults_for_dxvk_config_and_opwr() {
    let runner = std::path::Path::new("/opt/hyperx-proton");
    let layout = resolve_prefix_layout(Some(std::path::Path::new("/tmp/SHARED")), None).unwrap();

    let mut rule_env = BTreeMap::new();
    rule_env.insert("WINE_DISABLE_VULKAN_OPWR".to_string(), "0".to_string());
    rule_env.insert("DXVK_CONFIG".to_string(), "dxgi.maxFrameRate = 60".to_string());
    let rule_eval = RuleEvaluation {
        steam_game_id: Some("3513350".to_string()),
        env: rule_env,
        ..Default::default()
    };

    // Without user override: rule wins over runtime defaults and DXVK_CONFIG is passed verbatim
    let mut host_env = BTreeMap::new();
    host_env.insert("SteamGameId".to_string(), "0".to_string());
    let rt = build_runtime_env_with_sync(runner, &layout, &host_env, true, true, &rule_eval);
    assert_eq!(rt.env.get("SteamGameId").map(String::as_str), Some("3513350"));
    assert_eq!(rt.env.get("WINE_DISABLE_VULKAN_OPWR").map(String::as_str), Some("0"));
    assert_eq!(
        rt.env.get("DXVK_CONFIG").map(String::as_str),
        Some("dxgi.maxFrameRate = 60")
    );

    // With explicit user override: user beats rule
    host_env.insert("WINE_DISABLE_VULKAN_OPWR".to_string(), "1".to_string());
    host_env.insert("SteamGameId".to_string(), "999999".to_string());
    host_env.insert("DXVK_CONFIG".to_string(), "dxgi.maxFrameRate = 120".to_string());
    let rt_user = build_runtime_env_with_sync(runner, &layout, &host_env, true, true, &rule_eval);
    assert_eq!(rt_user.env.get("WINE_DISABLE_VULKAN_OPWR").map(String::as_str), Some("1"));
    assert_eq!(rt_user.env.get("SteamGameId").map(String::as_str), Some("999999"));
    assert_eq!(
        rt_user.env.get("DXVK_CONFIG").map(String::as_str),
        Some("dxgi.maxFrameRate = 120")
    );
}

use hyperx_proton::prefix::{prefix_needs_wineboot, sync_prefix_dlls};
use std::os::unix::fs::PermissionsExt;
use std::process::Command;

#[test]
fn prefix_syncs_dlls_and_removes_stale_nvapi() {
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
        ("files/lib/vkd3d-proton/x86_64-windows/d3d12.dll", b"VKD3D12".as_slice()),
        ("files/lib/vkd3d-proton/x86_64-windows/d3d12core.dll", b"VKD3DCORE".as_slice()),
        ("files/lib/nvapi/x86_64-windows/nvapi64.dll", b"NVAPI64".as_slice()),
    ] {
        let p = runner.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, content).unwrap();
    }

    let layout = resolve_prefix_layout(Some(&compat), None).unwrap();
    let r1 = sync_prefix_dlls(&runner, &layout, "v1", true).unwrap();
    assert!(r1.dlls_updated);
    assert_eq!(std::fs::read(pfx.join("drive_c/windows/system32/d3d11.dll")).unwrap(), b"STD_D3D11");
    assert!(pfx.join("drive_c/windows/system32/nvapi64.dll").is_file());

    // Disable NVAPI: NO wineboot needed, stale nvapi64.dll deleted!
    assert!(!prefix_needs_wineboot(&runner, &layout, "v1"));
    let r2 = sync_prefix_dlls(&runner, &layout, "v1", false).unwrap();
    assert!(!r2.dlls_updated);
    assert!(!r2.needs_wineboot);
    assert!(!pfx.join("drive_c/windows/system32/nvapi64.dll").exists());
    assert_eq!(std::fs::read_to_string(pfx.join("user.reg")).unwrap(), "KEEP_USER");
}

#[test]
fn cli_launches_wuwa_without_steam_shim_and_hsr_with_steam_shim_and_doctor_reports_phase2() {
    let tmp = tempfile::tempdir().unwrap();
    let runner = tmp.path().join("runner");
    let compat = tmp.path().join("SHARED");
    let log_file = tmp.path().join("calls.log");
    let rules_dir = tmp.path().join("empty-rules");
    std::fs::create_dir_all(&rules_dir).unwrap();

    // Minimal stub runner with dxvk, vkd3d, steam.exe shim
    std::fs::create_dir_all(runner.join("files/bin")).unwrap();
    std::fs::write(
        runner.join("hyperx-native.json"),
        r#"{"schema_version":1,"runner_mode":"native","requires_steamrt":false,"name":"HyperX-Proton","version":"v1","arch":"x86_64"}"#,
    ).unwrap();
    for (rel, content) in [
        ("files/lib/dxvk/x86_64-windows/d3d11.dll", b"STD11".as_slice()),
        ("files/lib/dxvk/x86_64-windows/dxgi.dll", b"STDX".as_slice()),
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
    assert!(out.contains("rules:engine"), "doctor:\n{out}");
}
