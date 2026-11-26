use hyperx_proton::config::resolve_prefix_layout;
use hyperx_proton::rules::RuleEvaluation;
use hyperx_proton::runtime::{
    SyncMode, build_runtime_env_with_sync, resolve_sync_mode,
    sanitize_steam_runtime_ld_library_path,
};
use std::collections::BTreeMap;
use std::path::Path;

#[test]
fn resolve_sync_mode_falls_back_across_ntsync_fsync_esync_and_plain() {
    assert_eq!(
        resolve_sync_mode(true, true, false, false, false),
        SyncMode::Ntsync
    );
    assert_eq!(
        resolve_sync_mode(false, true, false, false, false),
        SyncMode::Fsync
    );
    assert_eq!(
        resolve_sync_mode(true, true, true, false, false),
        SyncMode::Fsync
    );
    assert_eq!(
        resolve_sync_mode(false, false, false, false, false),
        SyncMode::Esync
    );
    assert_eq!(
        resolve_sync_mode(false, true, false, true, false),
        SyncMode::Esync
    );
    assert_eq!(
        resolve_sync_mode(true, true, true, true, false),
        SyncMode::Esync
    );
    assert_eq!(
        resolve_sync_mode(false, false, false, false, true),
        SyncMode::Plain
    );
    assert_eq!(
        resolve_sync_mode(true, true, true, true, true),
        SyncMode::Plain
    );
}

#[test]
fn build_runtime_env_with_sync_sets_esync_and_sanitizes_steam_scout_ld_library_path() {
    let runner = Path::new("/opt/hyperx-proton");
    let layout = resolve_prefix_layout(Some(Path::new("/tmp/SHARED")), None).unwrap();

    // 1. No ntsync & no fsync -> WINEESYNC=1; empty SYSTEM_LD_LIBRARY_PATH removes LD_LIBRARY_PATH completely
    let mut host_env = BTreeMap::new();
    host_env.insert(
        "LD_LIBRARY_PATH".to_string(),
        "/home/u/.steam/ubuntu12_32/steam-runtime/pinned_libs_64:/usr/lib".to_string(),
    );
    host_env.insert("SYSTEM_LD_LIBRARY_PATH".to_string(), String::new());
    host_env.insert(
        "LD_PRELOAD".to_string(),
        "/home/u/.steam/gameoverlayrenderer.so".to_string(),
    );

    let rt = build_runtime_env_with_sync(
        runner,
        &layout,
        &host_env,
        false,
        false,
        &RuleEvaluation::default(),
    );
    assert_eq!(rt.env.get("PROTON_USE_NTSYNC").map(String::as_str), Some("0"));
    assert_eq!(rt.env.get("WINEFSYNC").map(String::as_str), Some("0"));
    assert_eq!(rt.env.get("WINEESYNC").map(String::as_str), Some("1"));
    assert!(
        !rt.env.contains_key("LD_LIBRARY_PATH"),
        "empty SYSTEM_LD_LIBRARY_PATH must unset LD_LIBRARY_PATH"
    );
    assert_eq!(
        rt.env.get("LD_PRELOAD").map(String::as_str),
        Some("/home/u/.steam/gameoverlayrenderer.so")
    );

    // 1b. PROTON_NO_ESYNC=1 with no ntsync & no fsync -> all 3 sync vars are "0"
    let mut plain_env = host_env.clone();
    plain_env.insert("PROTON_NO_ESYNC".to_string(), "1".to_string());
    let rt_plain = build_runtime_env_with_sync(
        runner,
        &layout,
        &plain_env,
        false,
        false,
        &RuleEvaluation::default(),
    );
    assert_eq!(
        rt_plain.env.get("PROTON_USE_NTSYNC").map(String::as_str),
        Some("0")
    );
    assert_eq!(rt_plain.env.get("WINEFSYNC").map(String::as_str), Some("0"));
    assert_eq!(rt_plain.env.get("WINEESYNC").map(String::as_str), Some("0"));

    // 1c. PROTON_NO_FSYNC=1 with ntsync=false, fsync=true -> WINEESYNC=1
    let mut no_fsync_env = host_env.clone();
    no_fsync_env.insert("PROTON_NO_FSYNC".to_string(), "1".to_string());
    let rt_no_fsync = build_runtime_env_with_sync(
        runner,
        &layout,
        &no_fsync_env,
        false,
        true,
        &RuleEvaluation::default(),
    );
    assert_eq!(
        rt_no_fsync.env.get("PROTON_USE_NTSYNC").map(String::as_str),
        Some("0")
    );
    assert_eq!(
        rt_no_fsync.env.get("WINEFSYNC").map(String::as_str),
        Some("0")
    );
    assert_eq!(
        rt_no_fsync.env.get("WINEESYNC").map(String::as_str),
        Some("1")
    );

    // 2. Non-empty SYSTEM_LD_LIBRARY_PATH is restored verbatim
    host_env.insert(
        "SYSTEM_LD_LIBRARY_PATH".to_string(),
        "/opt/custom/lib:/usr/local/lib".to_string(),
    );
    let rt2 = build_runtime_env_with_sync(
        runner,
        &layout,
        &host_env,
        false,
        false,
        &RuleEvaluation::default(),
    );
    assert_eq!(
        rt2.env.get("LD_LIBRARY_PATH").map(String::as_str),
        Some("/opt/custom/lib:/usr/local/lib")
    );

    // 3. Without SYSTEM_LD_LIBRARY_PATH, only steam-runtime/ubuntu12_* segments are stripped
    let stripped = sanitize_steam_runtime_ld_library_path(
        Some("/opt/foo:/home/u/.steam/ubuntu12_32/steam-runtime/lib:/usr/local/lib"),
        None,
        false,
    );
    assert_eq!(stripped.as_deref(), Some("/opt/foo:/usr/local/lib"));

    let stripped_all = sanitize_steam_runtime_ld_library_path(
        Some("/home/u/.steam/ubuntu12_64/lib:/home/u/.steam/steam-runtime/lib"),
        None,
        false,
    );
    assert_eq!(stripped_all, None);

    // 4. Opt-out keeps Steam runtime LD_LIBRARY_PATH intact
    let kept = sanitize_steam_runtime_ld_library_path(
        Some("/home/u/.steam/ubuntu12_32/steam-runtime/lib"),
        Some(""),
        true,
    );
    assert_eq!(
        kept.as_deref(),
        Some("/home/u/.steam/ubuntu12_32/steam-runtime/lib")
    );
}

use hyperx_proton::doctor::{CheckSeverity, check_glibc_floor, find_host_lib, parse_glibc_version};
use std::os::unix::fs::PermissionsExt;
use std::process::Command;

#[test]
fn glibc_version_parser_and_floor_gating_work_across_distros() {
    assert_eq!(parse_glibc_version("2.35"), Some((2, 35)));
    assert_eq!(parse_glibc_version("2.38.1"), Some((2, 38)));
    assert_eq!(parse_glibc_version("2.44"), Some((2, 44)));
    assert_eq!(parse_glibc_version("invalid"), None);
    assert_eq!(parse_glibc_version(""), None);

    assert_eq!(check_glibc_floor("2.35").severity, CheckSeverity::Fail);
    assert_eq!(check_glibc_floor("2.36").severity, CheckSeverity::Fail);
    assert_eq!(check_glibc_floor("2.37").severity, CheckSeverity::Fail);
    assert_eq!(check_glibc_floor("2.38").severity, CheckSeverity::Ok);
    assert_eq!(check_glibc_floor("2.39").severity, CheckSeverity::Ok);
    assert_eq!(check_glibc_floor("2.44").severity, CheckSeverity::Ok);
    assert_eq!(check_glibc_floor("2.38.1").severity, CheckSeverity::Ok);
    assert_eq!(check_glibc_floor("weird").severity, CheckSeverity::Warn);
    assert_eq!(check_glibc_floor("").severity, CheckSeverity::Warn);

    let libc_found = find_host_lib("libc.so.6");
    assert!(
        libc_found.is_some(),
        "dlopen must resolve libc.so.6 via dynamic linker"
    );
    assert!(find_host_lib("libhyperx_definitely_missing_999.so").is_none());
}

#[test]
fn cli_runinprefix_bootstraps_prefix_and_skips_game_rules_and_steam_shim() {
    let tmp = tempfile::tempdir().unwrap();
    let runner = tmp.path().join("runner");
    let compat = tmp.path().join("SHARED");
    let log_file = tmp.path().join("calls.log");
    let rules_dir = tmp.path().join("empty-rules");
    let game_dir = tmp.path().join("game");
    std::fs::create_dir_all(&rules_dir).unwrap();
    std::fs::create_dir_all(&game_dir).unwrap();

    std::fs::create_dir_all(runner.join("files/bin")).unwrap();
    std::fs::write(
        runner.join("hyperx-native.json"),
        r#"{"schema_version":1,"runner_mode":"native","requires_steamrt":false,"name":"HyperX-Proton","version":"v1","arch":"x86_64"}"#,
    )
    .unwrap();
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
    std::fs::write(
        &wine_stub,
        "#!/usr/bin/env bash\necho \"wine:$*\" >> \"$TEST_CALL_LOG\"\nif [[ \"$1\" == \"wineboot\" ]]; then\n  mkdir -p \"$WINEPREFIX\"\n  touch \"$WINEPREFIX/system.reg\"\nfi\n",
    )
    .unwrap();
    std::fs::set_permissions(&wine_stub, std::fs::Permissions::from_mode(0o755)).unwrap();
    let ws_stub = runner.join("files/bin/wineserver");
    std::fs::write(
        &ws_stub,
        "#!/usr/bin/env bash\necho \"ws:$*\" >> \"$TEST_CALL_LOG\"\n",
    )
    .unwrap();
    std::fs::set_permissions(&ws_stub, std::fs::Permissions::from_mode(0o755)).unwrap();

    let bin = env!("CARGO_BIN_EXE_hyperx-proton");
    // 1. New prefix with StarRail.exe: must initialize prefix + sync DLLs, must NOT prepend steam.exe shim
    let st = Command::new(bin)
        .args(["runinprefix", "/tmp/HSR/StarRail.exe", "--helper"])
        .env("HYPERX_PROTON_ROOT", &runner)
        .env("STEAM_COMPAT_DATA_PATH", &compat)
        .env("HYPERX_PROTON_RULES_DIR", &rules_dir)
        .env("TEST_CALL_LOG", &log_file)
        .status()
        .unwrap();
    assert!(st.success());

    let log = std::fs::read_to_string(&log_file).unwrap();
    assert!(
        log.contains("wine:wineboot -u"),
        "first runinprefix must initialize prefix:\n{log}"
    );
    assert!(
        log.contains("wine:/tmp/HSR/StarRail.exe --helper"),
        "log:\n{log}"
    );
    assert!(
        !log.contains("steam.exe"),
        "runinprefix must not invoke steam.exe shim:\n{log}"
    );
    assert!(compat.join("pfx/drive_c/windows/system32/d3d11.dll").is_file());

    // 2. Existing prefix with WuWa-like exe: must NOT re-run wineboot -u and must NOT create usebitmap.flag
    std::fs::write(&log_file, "").unwrap();
    let wuwa_exe = game_dir.join("Client-Win64-Shipping.exe");
    std::fs::write(&wuwa_exe, b"EXE").unwrap();
    let st2 = Command::new(bin)
        .args(["runinprefix", wuwa_exe.to_str().unwrap(), "--check"])
        .env("HYPERX_PROTON_ROOT", &runner)
        .env("STEAM_COMPAT_DATA_PATH", &compat)
        .env("HYPERX_PROTON_RULES_DIR", &rules_dir)
        .env("TEST_CALL_LOG", &log_file)
        .status()
        .unwrap();
    assert!(st2.success());

    let log2 = std::fs::read_to_string(&log_file).unwrap();
    assert!(
        !log2.contains("wineboot"),
        "existing prefix must not re-run wineboot:\n{log2}"
    );
    assert!(
        !log2.contains("steam.exe"),
        "runinprefix must not invoke steam.exe shim:\n{log2}"
    );
    assert!(
        !game_dir.join("usebitmap.flag").exists(),
        "runinprefix must not apply game rules ensure_files"
    );

    // 3. Doctor reports host:glibc and sync tier
    let doc = Command::new(bin)
        .arg("doctor")
        .env("HYPERX_PROTON_ROOT", &runner)
        .env("HYPERX_PROTON_RULES_DIR", &rules_dir)
        .output()
        .unwrap();
    let out = String::from_utf8_lossy(&doc.stdout);
    assert!(out.contains("host:glibc"), "doctor:\n{out}");
    assert!(out.contains("sync:"), "doctor:\n{out}");
}
