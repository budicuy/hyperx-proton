use std::collections::BTreeMap;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::Command;

use hyperx_proton::config::resolve_prefix_layout;
use hyperx_proton::runtime::build_runtime_env;

fn create_stub_runner(root: &Path, with_dlls: bool) {
    fs::create_dir_all(root.join("files/bin")).unwrap();
    fs::write(
        root.join("hyperx-native.json"),
        r#"{"schema_version":1,"runner_mode":"native","requires_steamrt":false,"name":"HyperX-Proton","version":"hyperx-proton-11.0-20261125","arch":"x86_64"}"#,
    )
    .unwrap();

    if with_dlls {
        fs::create_dir_all(root.join("files/lib/dxvk/x86_64-windows")).unwrap();
        fs::create_dir_all(root.join("files/lib/vkd3d-proton/x86_64-windows")).unwrap();
        fs::write(
            root.join("files/lib/dxvk/x86_64-windows/d3d11.dll"),
            b"DXVK_D3D11",
        )
        .unwrap();
        fs::write(
            root.join("files/lib/dxvk/x86_64-windows/dxgi.dll"),
            b"DXVK_DXGI",
        )
        .unwrap();
        fs::write(
            root.join("files/lib/vkd3d-proton/x86_64-windows/d3d12.dll"),
            b"VKD3D_12",
        )
        .unwrap();
        fs::write(
            root.join("files/lib/vkd3d-proton/x86_64-windows/d3d12core.dll"),
            b"VKD3D_CORE",
        )
        .unwrap();
    }

    let wine_stub = root.join("files/bin/wine");
    fs::write(
        &wine_stub,
        r#"#!/usr/bin/env bash
set -euo pipefail
echo "wine:$*" >> "$TEST_CALL_LOG"
echo "WINEPREFIX=${WINEPREFIX:-}" >> "$TEST_CALL_LOG"
echo "PROTON_USE_NTSYNC=${PROTON_USE_NTSYNC:-}" >> "$TEST_CALL_LOG"
if [[ "${1:-}" == "wineboot" ]]; then
  exit 0
fi
exit "${TEST_WINE_EXIT:-0}"
"#,
    )
    .unwrap();
    fs::set_permissions(&wine_stub, fs::Permissions::from_mode(0o755)).unwrap();

    let wineserver_stub = root.join("files/bin/wineserver");
    fs::write(
        &wineserver_stub,
        r#"#!/usr/bin/env bash
set -euo pipefail
echo "wineserver:$*" >> "$TEST_CALL_LOG"
"#,
    )
    .unwrap();
    fs::set_permissions(&wineserver_stub, fs::Permissions::from_mode(0o755)).unwrap();
}

#[test]
fn build_runtime_env_enables_ntsync_when_device_present_and_respects_opt_out() {
    let runner = Path::new("/opt/hyperx-proton");
    let layout = resolve_prefix_layout(Some(Path::new("/tmp/SHARED")), None).unwrap();

    let mut host_env = BTreeMap::new();
    host_env.insert("PATH".to_string(), "/usr/bin:/bin".to_string());
    host_env.insert("WINEDLLOVERRIDES".to_string(), "jsproxy=n,b".to_string());
    host_env.insert("STUB_WINTRUST".to_string(), "1".to_string());
    host_env.insert("PROTON_ENABLE_WAYLAND".to_string(), "1".to_string());
    host_env.insert("PROTON_ENABLE_HDR".to_string(), "1".to_string());
    host_env.insert("__GL_SHADER_DISK_CACHE".to_string(), "1".to_string());
    host_env.insert(
        "__GL_SHADER_DISK_CACHE_PATH".to_string(),
        "/tmp/shaders".to_string(),
    );

    let rt = build_runtime_env(runner, &layout, &host_env, true);
    assert_eq!(rt.env.get("PROTON_USE_NTSYNC").map(String::as_str), Some("1"));
    assert_eq!(rt.env.get("WINEFSYNC").map(String::as_str), Some("0"));
    assert_eq!(rt.env.get("WINEESYNC").map(String::as_str), Some("0"));
    assert!(
        rt.env
            .get("PATH")
            .unwrap()
            .starts_with("/opt/hyperx-proton/files/bin:")
    );
    let overrides = rt.env.get("WINEDLLOVERRIDES").unwrap();
    assert!(overrides.contains("jsproxy=n,b"));
    assert!(overrides.contains("d3d11=n"));
    assert_eq!(rt.env.get("STUB_WINTRUST").map(String::as_str), Some("1"));
    assert_eq!(
        rt.env.get("PROTON_ENABLE_WAYLAND").map(String::as_str),
        Some("1")
    );
    assert_eq!(
        rt.env.get("PROTON_ENABLE_HDR").map(String::as_str),
        Some("1")
    );
    assert_eq!(
        rt.env.get("__GL_SHADER_DISK_CACHE").map(String::as_str),
        Some("1")
    );
    assert_eq!(
        rt.env
            .get("__GL_SHADER_DISK_CACHE_PATH")
            .map(String::as_str),
        Some("/tmp/shaders")
    );

    // Opt-out via PROTON_NO_NTSYNC=1
    let mut opt_out_env = host_env.clone();
    opt_out_env.insert("PROTON_NO_NTSYNC".to_string(), "1".to_string());
    let rt_opt_out = build_runtime_env(runner, &layout, &opt_out_env, true);
    assert_eq!(
        rt_opt_out.env.get("PROTON_USE_NTSYNC").map(String::as_str),
        Some("0")
    );
    assert_eq!(
        rt_opt_out.env.get("WINEFSYNC").map(String::as_str),
        Some("1")
    );

    // Unavailable /dev/ntsync
    let rt_no_dev = build_runtime_env(runner, &layout, &host_env, false);
    assert_eq!(
        rt_no_dev.env.get("PROTON_USE_NTSYNC").map(String::as_str),
        Some("0")
    );
    assert_eq!(
        rt_no_dev.env.get("WINEFSYNC").map(String::as_str),
        Some("1")
    );
}

#[test]
fn cli_waitforexitandrun_runs_stub_wine_and_waits_for_wineserver() {
    let tmp = tempfile::tempdir().unwrap();
    let runner = tmp.path().join("runner");
    let compat = tmp.path().join("SHARED");
    let log_file = tmp.path().join("calls.log");
    create_stub_runner(&runner, true);

    let bin = env!("CARGO_BIN_EXE_hyperx-proton");
    let output = Command::new(bin)
        .args(["waitforexitandrun", "/tmp/fake.exe", "-dx11"])
        .env("HYPERX_PROTON_ROOT", &runner)
        .env("STEAM_COMPAT_DATA_PATH", &compat)
        .env("TEST_CALL_LOG", &log_file)
        .env("TEST_WINE_EXIT", "7")
        .output()
        .expect("run hyperx-proton");

    assert_eq!(output.status.code(), Some(7));
    let log = fs::read_to_string(&log_file).unwrap();
    assert!(log.contains("wine:wineboot -u"), "log was:\n{log}");
    assert!(log.contains("wine:/tmp/fake.exe -dx11"), "log was:\n{log}");
    assert!(log.contains("wineserver:-w"), "log was:\n{log}");
    assert!(
        compat
            .join("pfx/drive_c/windows/system32/d3d11.dll")
            .is_file()
    );
}

#[test]
fn cli_version_and_doctor_commands() {
    let tmp = tempfile::tempdir().unwrap();
    let runner = tmp.path().join("runner");
    create_stub_runner(&runner, false);

    let bin = env!("CARGO_BIN_EXE_hyperx-proton");
    let ver_out = Command::new(bin)
        .arg("version")
        .env("HYPERX_PROTON_ROOT", &runner)
        .output()
        .expect("run version");
    assert!(ver_out.status.success());
    assert_eq!(
        String::from_utf8_lossy(&ver_out.stdout).trim(),
        "hyperx-proton-11.0-20261125"
    );

    let doc_out = Command::new(bin)
        .arg("doctor")
        .env("HYPERX_PROTON_ROOT", &runner)
        .output()
        .expect("run doctor");
    assert!(!doc_out.status.success());
    let doc_text = format!(
        "{}{}",
        String::from_utf8_lossy(&doc_out.stdout),
        String::from_utf8_lossy(&doc_out.stderr)
    );
    assert!(doc_text.contains("FAIL"), "doctor output:\n{doc_text}");
}
