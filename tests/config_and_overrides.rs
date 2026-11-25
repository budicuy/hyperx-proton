use std::fs;
use std::path::Path;
use hyperx_proton::config::{load_manifest, resolve_prefix_layout};
use hyperx_proton::dll_overrides::merge_dll_overrides;

#[test]
fn resolve_prefix_prefers_compat_data_pfx_over_flat_wineprefix() {
    let layout = resolve_prefix_layout(
        Some(Path::new("/tmp/prefixes/SHARED")),
        Some(Path::new("/tmp/prefixes/SHARED")),
    )
    .expect("should resolve compat data layout");
    assert_eq!(layout.wineprefix, Path::new("/tmp/prefixes/SHARED/pfx"));
    assert_eq!(
        layout.compat_data_dir.as_deref(),
        Some(Path::new("/tmp/prefixes/SHARED"))
    );
    assert_eq!(
        layout.lock_file,
        Path::new("/tmp/prefixes/SHARED/.hyperx-proton.lock")
    );
}

#[test]
fn resolve_prefix_falls_back_to_standalone_wineprefix_with_internal_lock() {
    let layout = resolve_prefix_layout(None, Some(Path::new("/tmp/custom_pfx")))
        .expect("should resolve standalone wineprefix");
    assert_eq!(layout.wineprefix, Path::new("/tmp/custom_pfx"));
    assert!(layout.compat_data_dir.is_none());
    assert_eq!(
        layout.lock_file,
        Path::new("/tmp/custom_pfx/.hyperx-proton.lock")
    );
}

#[test]
fn resolve_prefix_errors_when_both_unset() {
    assert!(resolve_prefix_layout(None, None).is_err());
    assert!(resolve_prefix_layout(Some(Path::new("")), Some(Path::new(""))).is_err());
}

#[test]
fn load_manifest_validates_native_mode_and_rejects_non_native() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(
        dir.path().join("hyperx-native.json"),
        r#"{"schema_version":1,"runner_mode":"native","requires_steamrt":false,"name":"HyperX-Proton","version":"hyperx-proton-11.0-20261125","arch":"x86_64"}"#,
    )
    .unwrap();
    let m = load_manifest(dir.path()).expect("valid native manifest");
    assert_eq!(m.version, "hyperx-proton-11.0-20261125");
    assert!(!m.requires_steamrt);

    fs::write(
        dir.path().join("hyperx-native.json"),
        r#"{"schema_version":1,"runner_mode":"steamrt","requires_steamrt":true,"name":"Other","version":"1.0","arch":"x86_64"}"#,
    )
    .unwrap();
    assert!(load_manifest(dir.path()).is_err());
}

#[test]
fn merge_dll_overrides_preserves_user_overrides_and_adds_dx11_dx12() {
    let merged = merge_dll_overrides(Some("jsproxy=n,b;KRSDKExternal.exe=d;d3d11=b"), false);
    assert!(merged.contains("jsproxy=n,b"));
    assert!(merged.contains("KRSDKExternal.exe=d"));
    assert!(merged.contains("d3d11=b"));
    assert!(!merged.contains("d3d11=n"));
    assert!(merged.contains("dxgi=n"));
    assert!(merged.contains("d3d12=n"));
    assert!(merged.contains("d3d12core=n"));
    assert!(merged.contains("lsteamclient=d"));
    assert!(merged.contains("mscoree=d"));
    assert!(merged.contains("mshtml=d"));
    assert!(merged.contains("winemenubuilder.exe=d"));
    assert!(!merged.contains("nvapi64=n"));

    let with_nvapi = merge_dll_overrides(None, true);
    assert!(with_nvapi.contains("nvapi64=n"));
}
