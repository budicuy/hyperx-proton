use std::fs;
use std::sync::{Arc, Barrier};
use std::thread;
use hyperx_proton::config::resolve_prefix_layout;
use hyperx_proton::prefix::{acquire_prefix_lock, sync_prefix_dlls};

#[test]
fn prepare_prefix_preserves_existing_registry_and_user_files() {
    let tmp = tempfile::tempdir().unwrap();
    let runner = tmp.path().join("runner");
    let compat = tmp.path().join("SHARED");
    let pfx = compat.join("pfx");
    let sys32 = pfx.join("drive_c/windows/system32");
    let users = pfx.join("drive_c/users/steamuser/AppData");
    fs::create_dir_all(&sys32).unwrap();
    fs::create_dir_all(&users).unwrap();
    fs::write(pfx.join("user.reg"), "KEEP_USER_REG").unwrap();
    fs::write(pfx.join("system.reg"), "KEEP_SYSTEM_REG").unwrap();
    fs::write(users.join("login.token"), "KEEP_TOKEN").unwrap();

    // Mock bundled DX11/DX12/NVAPI DLLs
    fs::create_dir_all(runner.join("files/lib/dxvk/x86_64-windows")).unwrap();
    fs::create_dir_all(runner.join("files/lib/vkd3d-proton/x86_64-windows")).unwrap();
    fs::create_dir_all(runner.join("files/lib/nvapi/x86_64-windows")).unwrap();
    fs::write(
        runner.join("files/lib/dxvk/x86_64-windows/d3d11.dll"),
        b"DXVK_D3D11",
    )
    .unwrap();
    fs::write(
        runner.join("files/lib/dxvk/x86_64-windows/dxgi.dll"),
        b"DXVK_DXGI",
    )
    .unwrap();
    fs::write(
        runner.join("files/lib/vkd3d-proton/x86_64-windows/d3d12.dll"),
        b"VKD3D_12",
    )
    .unwrap();
    fs::write(
        runner.join("files/lib/vkd3d-proton/x86_64-windows/d3d12core.dll"),
        b"VKD3D_CORE",
    )
    .unwrap();
    fs::write(
        runner.join("files/lib/nvapi/x86_64-windows/nvapi64.dll"),
        b"NVAPI64",
    )
    .unwrap();

    let layout = resolve_prefix_layout(Some(&compat), None).unwrap();
    let report = sync_prefix_dlls(&runner, &layout, "hyperx-proton-11.0-20261125", false).unwrap();
    assert!(report.dlls_updated);
    assert!(report.needs_wineboot);
    assert_eq!(
        fs::read_to_string(pfx.join("user.reg")).unwrap(),
        "KEEP_USER_REG"
    );
    assert_eq!(
        fs::read_to_string(pfx.join("system.reg")).unwrap(),
        "KEEP_SYSTEM_REG"
    );
    assert_eq!(
        fs::read_to_string(users.join("login.token")).unwrap(),
        "KEEP_TOKEN"
    );
    assert!(
        !fs::symlink_metadata(sys32.join("d3d11.dll"))
            .unwrap()
            .file_type()
            .is_symlink()
    );
    assert_eq!(fs::read(sys32.join("d3d11.dll")).unwrap(), b"DXVK_D3D11");
    assert_eq!(
        fs::read(sys32.join("d3d12core.dll")).unwrap(),
        b"VKD3D_CORE"
    );
    assert!(!sys32.join("nvapi64.dll").exists());
    assert_eq!(
        fs::read_to_string(compat.join("version")).unwrap().trim(),
        "hyperx-proton-11.0-20261125"
    );
    assert_eq!(
        fs::read_to_string(pfx.join(".hyperx-proton-version"))
            .unwrap()
            .trim(),
        "hyperx-proton-11.0-20261125"
    );

    // Second run with same version is idempotent
    let report2 =
        sync_prefix_dlls(&runner, &layout, "hyperx-proton-11.0-20261125", false).unwrap();
    assert!(!report2.dlls_updated);
    assert!(!report2.needs_wineboot);
}

#[test]
fn prefix_lock_serializes_concurrent_initialization() {
    let tmp = tempfile::tempdir().unwrap();
    let lock_path = tmp.path().join(".hyperx-proton.lock");
    let barrier = Arc::new(Barrier::new(2));
    let lp1 = lock_path.clone();
    let b1 = Arc::clone(&barrier);
    let t1 = thread::spawn(move || {
        let _g = acquire_prefix_lock(&lp1).unwrap();
        b1.wait();
        std::thread::sleep(std::time::Duration::from_millis(80));
        std::time::Instant::now()
    });
    barrier.wait();
    let _g2 = acquire_prefix_lock(&lock_path).unwrap();
    let t2_acquired = std::time::Instant::now();
    let t1_released = t1.join().unwrap();
    assert!(t2_acquired >= t1_released - std::time::Duration::from_millis(5));
}
