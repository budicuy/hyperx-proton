use hyperx_proton::config::resolve_prefix_layout;
use hyperx_proton::gpu::GpuProbeResult;
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
    let probe = GpuProbeResult::Unavailable("test".to_string());

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
        &probe,
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
        &probe,
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
        &probe,
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
        &probe,
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
