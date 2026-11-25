use std::fs;
use std::sync::{Arc, Barrier};
use std::thread;
use hyperx_proton::config::resolve_prefix_layout;
use hyperx_proton::prefix::{
    acquire_prefix_lock, has_foreign_runner_symlinks, prune_foreign_runner_symlinks,
    sync_prefix_dlls,
};

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
    fs::create_dir_all(runner.join("files/lib/wine/icu/x86_64-windows")).unwrap();
    fs::create_dir_all(runner.join("files/lib/wine/icu/i386-windows")).unwrap();
    fs::create_dir_all(runner.join("files/lib/wine/x86_64-windows")).unwrap();
    for arch in ["x86_64-windows", "i386-windows"] {
        for dll in ["icudt68.dll", "icuin68.dll", "icuuc68.dll"] {
            fs::write(
                runner.join(format!("files/lib/wine/icu/{arch}/{dll}")),
                dll.as_bytes(),
            )
            .unwrap();
        }
    }
    fs::write(
        runner.join("files/lib/wine/x86_64-windows/steam.exe"),
        b"STEAM_SHIM",
    )
    .unwrap();
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
    assert_eq!(fs::read(sys32.join("icuuc68.dll")).unwrap(), b"icuuc68.dll");
    assert_eq!(
        fs::read(pfx.join("drive_c/windows/syswow64/icuuc68.dll")).unwrap(),
        b"icuuc68.dll"
    );
    assert_eq!(fs::read(sys32.join("steam.exe")).unwrap(), b"STEAM_SHIM");
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

#[test]
fn prune_foreign_runner_symlinks_cleans_old_proton_links_without_touching_user_files() {
    use std::os::unix::fs::symlink;

    let tmp = tempfile::tempdir().unwrap();
    let runner = tmp.path().join("hyperx-runner");
    let foreign = tmp.path().join("GE-Proton11-14/files/lib/wine/x86_64-windows");
    let pfx = tmp.path().join("SHARED/pfx");
    let sys32 = pfx.join("drive_c/windows/system32");
    let users = pfx.join("drive_c/users/steamuser");
    fs::create_dir_all(&runner).unwrap();
    fs::create_dir_all(&foreign).unwrap();
    fs::create_dir_all(&sys32).unwrap();
    fs::create_dir_all(&users).unwrap();

    fs::write(foreign.join("wineboot.exe"), b"OLD_GE").unwrap();
    fs::write(foreign.join("winex11.drv"), b"OLD_GE").unwrap();
    symlink(foreign.join("wineboot.exe"), sys32.join("wineboot.exe")).unwrap();
    symlink(foreign.join("winex11.drv"), sys32.join("winex11.drv")).unwrap();
    symlink(tmp.path().join("nonexistent/explorer.exe"), pfx.join("drive_c/windows/explorer.exe")).unwrap();

    // Regular files and user data must be preserved
    fs::write(sys32.join("custom_user.dll"), b"USER_DLL").unwrap();
    fs::write(pfx.join("user.reg"), "KEEP_USER_REG").unwrap();
    fs::write(users.join("save.dat"), "KEEP_SAVE").unwrap();

    assert!(has_foreign_runner_symlinks(&runner, &pfx));
    let removed = prune_foreign_runner_symlinks(&runner, &pfx).unwrap();
    assert_eq!(removed, 3);
    assert!(!has_foreign_runner_symlinks(&runner, &pfx));
    assert!(!sys32.join("wineboot.exe").exists());
    assert!(!sys32.join("winex11.drv").exists());
    assert!(!pfx.join("drive_c/windows/explorer.exe").exists());
    assert_eq!(fs::read(sys32.join("custom_user.dll")).unwrap(), b"USER_DLL");
    assert_eq!(fs::read_to_string(pfx.join("user.reg")).unwrap(), "KEEP_USER_REG");
    assert_eq!(fs::read_to_string(users.join("save.dat")).unwrap(), "KEEP_SAVE");
}
