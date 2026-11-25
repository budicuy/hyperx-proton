# HyperX-Proton Fase 3 (Universal x86_64 & Cross-Distro Portability) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Menjadikan `HyperX-Proton` universal untuk semua CPU `x86_64` (tanpa tuning CPU khusus) dan siap jalan lintas distro Linux modern (`glibc >= 2.38`) termasuk drop-in di HyperX-Launcher, Steam, Heroic, dan Lutris, lengkap dengan dokumentasi syarat sistem di `README.md`.

**Architecture:** Menghilangkan instruksi CPU spesifik (`-march=alderlake`) dan SONAME rapuh antar-distro (`--without-ffmpeg --without-unwind`) pada build Wine; menambahkan sanitasi `LD_LIBRARY_PATH` Steam Scout dan fallback sync 3-tingkat (`ntsync → fsync → esync`) di `src/runtime.rs`; mengganti pencarian library hardcoded `src/doctor.rs` dengan `dlopen`/`dlinfo` + gating `glibc >= 2.38`; menambahkan verb `runinprefix` di `src/main.rs`; menghasilkan `compatibilitytool.vdf` + `toolmanifest.vdf` di `scripts/package-runner.sh`; serta melengkapi `README.md`.

**Tech Stack:** Rust (edition 2024, `libc`, `serde`, `serde_json`, `tempfile`), Bash build scripts, GCC (`-march=x86-64 -mtune=generic`), `wine-cachyos`, Valve VDF metadata.

**Spec:** `docs/superpowers/specs/2026-11-26-hyperx-proton-universal-portability-design.md`

## Global Constraints

- Nol crate baru di `Cargo.toml` (tetap `libc`, `serde`, `serde_json`, dev-dep `tempfile`).
- Default CPU baseline wajib universal **`x86-64`** (`-march=x86-64 -mtune=generic`), tanpa `-march=alderlake` atau `-march=x86-64-v3` default.
- Nol ketergantungan `steam-runtime` (`toolmanifest.vdf` dilarang menyertakan `require_tool_appid`) dan nol `umu-launcher`.
- Binary Wine hasil rebuild wajib memiliki **0** `DT_NEEDED` ke `libavcodec*`, `libavformat*`, `libavutil*`, atau `libunwind*`.
- `hyperx-native.json` tetap `schema_version: 1`, `runner_mode: "native"`, `requires_steamrt: false`.
- Default `scripts/install-local.sh` (tanpa flag) tetap memasang ke `hyperx-launcher` seperti sebelumnya.
- Seluruh 28 test Rust existing + `tests/packaging_smoke.sh` wajib tetap lulus.

## Review Focus

1. **Steam Scout `LD_LIBRARY_PATH` saat `SYSTEM_LD_LIBRARY_PATH=""` (kosong):** Harus memanggil `env.remove("LD_LIBRARY_PATH")` — bukan menyisakan `LD_LIBRARY_PATH=""` (karena dynamic linker dapat menafsirkan string kosong sebagai direktori aktif `.`) (dites di Task 1).
2. **Verb `runinprefix` pada prefix baru maupun existing:** Harus tetap mengunci prefix dan menginisialisasi/sinkronisasi DLL bila prefix belum siap, namun **tidak boleh** menyisipkan `steam.exe` shim, membuat `usebitmap.flag`, atau menjalankan game rules walau argumen exe cocok dengan WuWa/HSR (dites di Task 2).
3. **Batas `check_glibc_floor` dan parsing string versi:** `"2.35"`, `"2.36"`, `"2.37"` wajib `Fail`; `"2.38"`, `"2.39"`, `"2.44"`, `"2.38.1"` wajib `Ok`; string tak valid wajib `Warn` (tidak boleh panic) (dites di Task 2).
4. **Matriks fallback sync 3-tingkat & opt-out:** `ntsync=false` + `fsync=false` (atau `PROTON_NO_FSYNC=1`) wajib mengaktifkan `WINEESYNC=1` dengan `PROTON_USE_NTSYNC=0` dan `WINEFSYNC=0`; jika `PROTON_NO_ESYNC=1` juga aktif, ketiganya harus `0` (dites di Task 1).
5. **Audit ELF nyata setelah rebuild Wine universal:** `readelf -d` di semua binary `files/bin/{wine,wineserver}` dan `files/lib/wine/x86_64-unix/*.so` wajib bebas `libav*` & `libunwind*`, dan `compatibilitytool.vdf`/`toolmanifest.vdf` bebas `require_tool_appid` (dites di Task 3 & Task 5).

---

### Task 1: 3-Tier Kernel Sync & Steam Scout `LD_LIBRARY_PATH` Sanitization (`src/runtime.rs`)

**Files:**
- Modify: `src/runtime.rs:1-200`
- Create: `tests/phase3_portability.rs`

**Interfaces:**
- Consumes: `libc::syscall` (`SYS_futex_waitv = 449` pada `x86_64`), `std::io::Error::last_os_error`, `crate::config::PrefixLayout`, `crate::gpu::GpuProbeResult`, `crate::rules::RuleEvaluation`.
- Produces:
  - `#[derive(Debug, Clone, Copy, PartialEq, Eq)] pub enum SyncMode { Ntsync, Fsync, Esync, Plain }` (plus `as_str(&self) -> &'static str`)
  - `pub fn is_fsync_available() -> bool`
  - `pub fn resolve_sync_mode(ntsync_available: bool, fsync_available: bool, no_ntsync: bool, no_fsync: bool, no_esync: bool) -> SyncMode`
  - `pub fn sanitize_steam_runtime_ld_library_path(ld_library_path: Option<&str>, system_ld_library_path: Option<&str>, keep_steam_runtime: bool) -> Option<String>`
  - `pub fn build_runtime_env_with_sync(runner_root: &Path, layout: &PrefixLayout, host_env: &BTreeMap<String, String>, ntsync_available: bool, fsync_available: bool, gpu_probe: &GpuProbeResult, rule_eval: &RuleEvaluation) -> RuntimeEnv`
  - Mempertahankan wrapper `build_runtime_env_with_context(...)` (meneruskan `fsync_available = true`) dan `build_runtime_env(...)` untuk backward compatibility.

- [ ] **Step 1: Tulis failing tests untuk 3-tier sync & sanitasi `LD_LIBRARY_PATH` Steam Scout di `tests/phase3_portability.rs`**

```rust
use std::collections::BTreeMap;
use std::path::Path;
use hyperx_proton::config::resolve_prefix_layout;
use hyperx_proton::gpu::GpuProbeResult;
use hyperx_proton::rules::RuleEvaluation;
use hyperx_proton::runtime::{
    SyncMode, build_runtime_env_with_sync, resolve_sync_mode,
    sanitize_steam_runtime_ld_library_path,
};

#[test]
fn resolve_sync_mode_falls_back_across_ntsync_fsync_esync_and_plain() {
    assert_eq!(resolve_sync_mode(true, true, false, false, false), SyncMode::Ntsync);
    assert_eq!(resolve_sync_mode(false, true, false, false, false), SyncMode::Fsync);
    assert_eq!(resolve_sync_mode(true, true, true, false, false), SyncMode::Fsync);
    assert_eq!(resolve_sync_mode(false, false, false, false, false), SyncMode::Esync);
    assert_eq!(resolve_sync_mode(false, true, false, true, false), SyncMode::Esync);
    assert_eq!(resolve_sync_mode(false, false, false, false, true), SyncMode::Plain);
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
    host_env.insert("LD_PRELOAD".to_string(), "/home/u/.steam/gameoverlayrenderer.so".to_string());

    let rt = build_runtime_env_with_sync(runner, &layout, &host_env, false, false, &probe, &RuleEvaluation::default());
    assert_eq!(rt.env.get("PROTON_USE_NTSYNC").map(String::as_str), Some("0"));
    assert_eq!(rt.env.get("WINEFSYNC").map(String::as_str), Some("0"));
    assert_eq!(rt.env.get("WINEESYNC").map(String::as_str), Some("1"));
    assert!(!rt.env.contains_key("LD_LIBRARY_PATH"), "empty SYSTEM_LD_LIBRARY_PATH must unset LD_LIBRARY_PATH");
    assert_eq!(
        rt.env.get("LD_PRELOAD").map(String::as_str),
        Some("/home/u/.steam/gameoverlayrenderer.so")
    );

    // 2. Non-empty SYSTEM_LD_LIBRARY_PATH is restored verbatim
    host_env.insert("SYSTEM_LD_LIBRARY_PATH".to_string(), "/opt/custom/lib:/usr/local/lib".to_string());
    let rt2 = build_runtime_env_with_sync(runner, &layout, &host_env, false, false, &probe, &RuleEvaluation::default());
    assert_eq!(rt2.env.get("LD_LIBRARY_PATH").map(String::as_str), Some("/opt/custom/lib:/usr/local/lib"));

    // 3. Without SYSTEM_LD_LIBRARY_PATH, only steam-runtime/ubuntu12_* segments are stripped
    let stripped = sanitize_steam_runtime_ld_library_path(
        Some("/opt/foo:/home/u/.steam/ubuntu12_32/steam-runtime/lib:/usr/local/lib"),
        None,
        false,
    );
    assert_eq!(stripped.as_deref(), Some("/opt/foo:/usr/local/lib"));

    // 4. Opt-out keeps Steam runtime LD_LIBRARY_PATH intact
    let kept = sanitize_steam_runtime_ld_library_path(
        Some("/home/u/.steam/ubuntu12_32/steam-runtime/lib"),
        Some(""),
        true,
    );
    assert_eq!(kept.as_deref(), Some("/home/u/.steam/ubuntu12_32/steam-runtime/lib"));
}
```

- [ ] **Step 2: Jalankan test untuk memastikan gagal**

Run: `cargo test --test phase3_portability`
Expected: FAIL karena `SyncMode`, `resolve_sync_mode`, `sanitize_steam_runtime_ld_library_path`, dan `build_runtime_env_with_sync` belum ada.

- [ ] **Step 3: Implementasikan 3-tier sync & sanitasi `LD_LIBRARY_PATH` di `src/runtime.rs`**

- `is_fsync_available()`: panggil `unsafe { libc::syscall(449, std::ptr::null::<libc::c_void>(), 0usize, 0u32, std::ptr::null::<libc::c_void>(), 0i32) }`; bila return `< 0` dan `std::io::Error::last_os_error().raw_os_error() == Some(libc::ENOSYS)` kembalikan `false`, selain itu `true`.
- `sanitize_steam_runtime_ld_library_path(...)`: cek apakah `ld_library_path` mengandung segmen dengan `"steam-runtime"`, `"ubuntu12_32"`, atau `"ubuntu12_64"`. Jika tidak (atau `keep_steam_runtime == true`), pertahankan apa adanya. Jika ya:
  - Bila `system_ld_library_path` adalah `Some(sys)` → kembalikan `Some(sys.to_string())` bila `!sys.trim().is_empty()`, selain itu `None`.
  - Bila `system_ld_library_path` adalah `None` → filter segmen `split(':')` yang tidak kosong dan tidak mengandung `"steam-runtime"`/`"ubuntu12_32"`/`"ubuntu12_64"`; bila hasil non-empty gabung dengan `":"`, selain itu `None`.
- Di `build_runtime_env_with_sync`: terapkan sanitasi `LD_LIBRARY_PATH` (jika `Some(v)` insert, jika `None` panggil `env.remove("LD_LIBRARY_PATH")`) dan set ketiga var `PROTON_USE_NTSYNC`, `WINEFSYNC`, `WINEESYNC` sesuai `SyncMode`.

- [ ] **Step 4: Jalankan seluruh test Rust**

Run: `cargo test`
Expected: PASS, 0 warning.

- [ ] **Step 5: Commit**

```bash
git add src/runtime.rs tests/phase3_portability.rs
git commit -m "feat(runtime): add 3-tier ntsync/fsync/esync fallback and Steam runtime LD_LIBRARY_PATH sanitization"
```

---

### Task 2: Cross-Distro `proton doctor` (`dlopen` + `dlinfo`, `glibc >= 2.38` Gating) & `runinprefix` Verb (`src/doctor.rs`, `src/main.rs`)

**Files:**
- Modify: `src/doctor.rs:1-295`
- Modify: `src/main.rs:47-290`
- Modify: `tests/phase3_portability.rs`

**Interfaces:**
- Consumes: `crate::runtime::{SyncMode, build_runtime_env_with_sync, is_fsync_available, is_ntsync_available, resolve_sync_mode}`, `libc::{dlopen, dlclose, RTLD_LAZY, RTLD_LOCAL}` + FFI `dlinfo` (`RTLD_DI_LINKMAP = 2`) + `gnu_get_libc_version`.
- Produces:
  - Di `src/doctor.rs`:
    - `pub const MIN_GLIBC_VERSION: (u32, u32) = (2, 38);`
    - `pub fn parse_glibc_version(raw: &str) -> Option<(u32, u32)>`
    - `pub fn check_glibc_floor(raw: &str) -> DoctorCheck` (`Fail` bila `< (2, 38)`, `Ok` bila `>= (2, 38)`, `Warn` bila gagal parse)
    - `pub fn find_host_lib(lib_name: &str) -> Option<String>` (pakai `dlopen` + `dlinfo(RTLD_DI_LINKMAP)` lalu `dlclose`, otomatis lintas multiarch Arch/Ubuntu/Debian/Fedora/openSUSE/NixOS)
    - Update `run_doctor`: gunakan `is_fsync_available()` untuk melaporkan sync tier (`ntsync` / `fsync` / `esync`), tambahkan check `host:glibc` menggunakan `gnu_get_libc_version()`.
  - Di `src/main.rs`:
    - Perbarui usage string agar mencantumkan `runinprefix`.
    - Teruskan `is_fsync_available()` ke `build_runtime_env_with_sync`.
    - Tambah handler `run_in_prefix(runner_root: &Path, args: &[String]) -> Result<i32, String>` untuk verb `"runinprefix"`: lock prefix, jalankan `wineboot -u` & `sync_prefix_dlls_with_options` jika prefix belum siap/berubah, lalu spawn langsung `wine <exe> [args...]` **tanpa** game rules (`RuleEvaluation::default()`), **tanpa** `steam.exe` shim, **tanpa** `ensure_files`, **tanpa** shader pre-warm, dan **tanpa** thread supervisor.

- [ ] **Step 1: Tulis failing tests di `tests/phase3_portability.rs` untuk `check_glibc_floor`, `find_host_lib` via `dlopen`, dan CLI `runinprefix`**

```rust
use hyperx_proton::doctor::{CheckSeverity, check_glibc_floor, find_host_lib, parse_glibc_version};
use std::os::unix::fs::PermissionsExt;
use std::process::Command;

#[test]
fn glibc_version_parser_and_floor_gating_work_across_distros() {
    assert_eq!(parse_glibc_version("2.35"), Some((2, 35)));
    assert_eq!(parse_glibc_version("2.38.1"), Some((2, 38)));
    assert_eq!(parse_glibc_version("2.44"), Some((2, 44)));
    assert_eq!(parse_glibc_version("invalid"), None);

    assert_eq!(check_glibc_floor("2.35").severity, CheckSeverity::Fail);
    assert_eq!(check_glibc_floor("2.36").severity, CheckSeverity::Fail);
    assert_eq!(check_glibc_floor("2.37").severity, CheckSeverity::Fail);
    assert_eq!(check_glibc_floor("2.38").severity, CheckSeverity::Ok);
    assert_eq!(check_glibc_floor("2.39").severity, CheckSeverity::Ok);
    assert_eq!(check_glibc_floor("2.44").severity, CheckSeverity::Ok);
    assert_eq!(check_glibc_floor("weird").severity, CheckSeverity::Warn);

    let libc_found = find_host_lib("libc.so.6");
    assert!(libc_found.is_some(), "dlopen must resolve libc.so.6 via dynamic linker");
    assert!(find_host_lib("libhyperx_definitely_missing_999.so").is_none());
}

#[test]
fn cli_runinprefix_bootstraps_prefix_and_skips_game_rules_and_steam_shim() {
    let tmp = tempfile::tempdir().unwrap();
    let runner = tmp.path().join("runner");
    let compat = tmp.path().join("SHARED");
    let log_file = tmp.path().join("calls.log");
    let rules_dir = tmp.path().join("empty-rules");
    std::fs::create_dir_all(&rules_dir).unwrap();

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

    let bin = env!("CARGO_BIN_EXE_hyperx-proton");
    // Even if invoked with StarRail.exe, runinprefix must NOT prepend steam.exe shim
    let st = Command::new(bin)
        .args(["runinprefix", "/tmp/HSR/StarRail.exe", "--helper"])
        .env("HYPERX_PROTON_ROOT", &runner)
        .env("STEAM_COMPAT_DATA_PATH", &compat)
        .env("HYPERX_PROTON_RULES_DIR", &rules_dir)
        .env("HYPERX_PROTON_NO_GPU_PROBE", "1")
        .env("TEST_CALL_LOG", &log_file)
        .status()
        .unwrap();
    assert!(st.success());

    let log = std::fs::read_to_string(&log_file).unwrap();
    assert!(log.contains("wine:wineboot -u"), "first runinprefix must initialize prefix:\n{log}");
    assert!(log.contains("wine:/tmp/HSR/StarRail.exe --helper"), "log:\n{log}");
    assert!(!log.contains("steam.exe"), "runinprefix must not invoke steam.exe shim:\n{log}");
    assert!(compat.join("pfx/drive_c/windows/system32/d3d11.dll").is_file());

    // Doctor reports host:glibc
    let doc = Command::new(bin)
        .arg("doctor")
        .env("HYPERX_PROTON_ROOT", &runner)
        .env("HYPERX_PROTON_RULES_DIR", &rules_dir)
        .output()
        .unwrap();
    let out = String::from_utf8_lossy(&doc.stdout);
    assert!(out.contains("host:glibc"), "doctor:\n{out}");
}
```

- [ ] **Step 2: Jalankan test untuk memastikan gagal**

Run: `cargo test --test phase3_portability`
Expected: FAIL karena simbol baru di `doctor` dan verb `runinprefix` belum ada.

- [ ] **Step 3: Implementasikan pembaruan `src/doctor.rs` dan `src/main.rs`**

Catatan FFI `src/doctor.rs`:
- Deklarasikan `#[repr(C)] struct LinkMap { l_addr: libc::uintptr_t, l_name: *const libc::c_char }` dan `unsafe extern "C" { fn dlinfo(handle: *mut libc::c_void, request: libc::c_int, info: *mut libc::c_void) -> libc::c_int; fn gnu_get_libc_version() -> *const libc::c_char; }`.
- Di `find_host_lib(lib_name)`: buat `CString::new(lib_name)`, panggil `libc::dlopen(c_name.as_ptr(), libc::RTLD_LAZY | libc::RTLD_LOCAL)`. Bila null return `None`. Bila non-null, coba `dlinfo(handle, 2 /* RTLD_DI_LINKMAP */, &mut lm as *mut _ as *mut c_void)` untuk membaca path `l_name` (bila kosong, pakai `lib_name.to_string()`), lalu selalu panggil `libc::dlclose(handle)`.

- [ ] **Step 4: Jalankan seluruh test suite Rust**

Run: `cargo test`
Expected: PASS, 0 warning.

- [ ] **Step 5: Commit**

```bash
git add src/doctor.rs src/main.rs tests/phase3_portability.rs
git commit -m "feat(portability): use dlopen/dlinfo and glibc floor in doctor, add runinprefix verb"
```

---

### Task 3: Universal x86-64 Build Flags, Clean Wine Configure, VDF Metadata, & Cross-Launcher Installer (`scripts/`, `packaging/`, `tests/packaging_smoke.sh`)

**Files:**
- Modify: `scripts/common.sh:12`
- Modify: `scripts/build-wine.sh:6-75`
- Modify: `packaging/arch/PKGBUILD:57-62`
- Modify: `scripts/package-runner.sh:50-134`
- Modify: `scripts/install-local.sh:1-22`
- Modify: `tests/packaging_smoke.sh:65-114`

**Interfaces:**
- Produces:
  - Default `HYPERX_MARCH="x86-64"` di `scripts/common.sh` dan `packaging/arch/PKGBUILD`.
  - `scripts/build-wine.sh`: `CFLAGS="-O2 -march=${HYPERX_MARCH} -mtune=generic -pipe"`, `CXXFLAGS` sama, tambah `--without-ffmpeg --without-unwind`, stamp key `wine:${WINE_REF}:${HYPERX_MARCH}:portable-v1:${PATCH_HASH}`.
  - `$RUNNER_DIR/compatibilitytool.vdf` dan `$RUNNER_DIR/toolmanifest.vdf` (dengan `commandline = "/proton %verb%"`, tanpa `require_tool_appid`).
  - `scripts/install-local.sh [--target hyperx|steam|heroic]` (default `hyperx`, `steam` → `${XDG_DATA_HOME:-$HOME/.local/share}/Steam/compatibilitytools.d/$HYPERX_PROTON_VERSION`, `heroic` → `${XDG_CONFIG_HOME:-$HOME/.config}/heroic/tools/proton/$HYPERX_PROTON_VERSION`).

- [ ] **Step 1: Perbarui `tests/packaging_smoke.sh` lebih dulu (TDD) agar memverifikasi `compatibilitytool.vdf`, `toolmanifest.vdf`, dan larangan `require_tool_appid`**

Tambahkan di `tests/packaging_smoke.sh`:
- Cek `$RUNNER_DIR/compatibilitytool.vdf` ada dan mengandung `$HYPERX_PROTON_VERSION`.
- Cek `$RUNNER_DIR/toolmanifest.vdf` ada, mengandung `"/proton %verb%"`, dan **tidak** mengandung `require_tool_appid`.

- [ ] **Step 2: Jalankan `bash tests/packaging_smoke.sh` untuk memastikan gagal**

Run: `bash tests/packaging_smoke.sh`
Expected: FAIL karena VDF belum dihasilkan `package-runner.sh`.

- [ ] **Step 3: Perbarui `scripts/common.sh`, `scripts/build-wine.sh`, `packaging/arch/PKGBUILD`, `scripts/package-runner.sh`, dan `scripts/install-local.sh`**

- Di `scripts/package-runner.sh`, tulis:
  - `compatibilitytool.vdf`:
    ```vdf
    "compatibilitytools"
    {
      "compat_tools"
      {
        "$HYPERX_PROTON_VERSION"
        {
          "install_path" "."
          "display_name" "HyperX-Proton ($HYPERX_PROTON_VERSION)"
          "from_oslist"  "windows"
          "to_oslist"    "linux"
        }
      }
    }
    ```
  - `toolmanifest.vdf`:
    ```vdf
    "manifest"
    {
      "version" "2"
      "commandline" "/proton %verb%"
      "use_sessions" "1"
    }
    ```
- Pastikan `bash -n scripts/*.sh tests/packaging_smoke.sh packaging/arch/PKGBUILD` bersih.

- [ ] **Step 4: Jalankan `cargo build --release && bash tests/packaging_smoke.sh`**

Run: `cargo build --release && bash tests/packaging_smoke.sh`
Expected: PASS (`packaging_smoke.sh: ALL CHECKS PASSED`).

- [ ] **Step 5: Commit**

```bash
git add scripts/common.sh scripts/build-wine.sh packaging/arch/PKGBUILD scripts/package-runner.sh scripts/install-local.sh tests/packaging_smoke.sh
git commit -m "build(portability): switch to universal x86-64, drop ffmpeg/unwind deps, add Steam/Heroic VDF"
```

---

### Task 4: Dokumentasi Syarat Minimum & Rekomendasi Sistem Lintas Distro (`README.md`)

**Files:**
- Modify: `README.md`

**Interfaces:**
- Consumes: seluruh kontrak aktual dari Fase 1–3 (`glibc >= 2.38`, CPU universal `x86-64`, Vulkan `1.3+` DXVK vs `1.1/1.2` DXVK-Sarek, sync 3-tingkat `ntsync`/`fsync`/`esync`, launcher install targets, env vars, dan schema custom JSON rules).

- [ ] **Step 1: Perbarui `README.md` dengan dokumentasi lengkap & akurat**

Sertakan secara rapi dan terstruktur:
1. Ringkasan fitur (pure native zero-SteamRT, universal `x86_64`, AMD/Intel/NVIDIA auto-detection + DXVK-Sarek `v1.14.1`, Rust native rule engine, 3-tier kernel sync).
2. **Syarat Minimum & Rekomendasi Sistem (Tabel):**
   - **CPU:** Minimum semua prosesor `x86_64` (Intel/AMD universal `-march=x86-64`, tidak membutuhkan AVX2/CPU generasi khusus); Rekomendasi 4+ physical cores (untuk kompilasi shader paralel).
   - **OS & `glibc`:** Minimum distro Linux 64-bit dengan **`glibc >= 2.38`** (Ubuntu 24.04+, Linux Mint 22+, Debian 13+, Fedora 39+, Nobara, Bazzite, Arch Linux, CachyOS, EndeavourOS, openSUSE Tumbleweed, SteamOS 3.6+); tidak mendukung distro LTS lawas dengan `glibc < 2.38` seperti Ubuntu 22.04 (`2.35`) atau Debian 12 (`2.36`) pada build host Arch Level 1 ini.
   - **Kernel & Sync:** Minimum Linux 5.x (`WINEESYNC`); Rekomendasi Linux `5.16+` (`WINEFSYNC` / `futex_waitv`) atau **`6.14+` (`/dev/ntsync`)**.
   - **GPU & Driver:**
     - **Rekomendasi (Vulkan 1.3+ → DXVK `v3.1.1` & VKD3D-Proton `v3.1`):** NVIDIA GTX 16xx/RTX 20xx+ (driver 550+), AMD RX 5000+ (Mesa RADV 23+), Intel Arc / Gen 11+ (Mesa ANV 23+).
     - **Minimum (Vulkan 1.1/1.2 → auto-fallback DXVK-Sarek `v1.14.1`):** GPU lawas AMD GCN/Polaris, Intel HD/UHD Gen 7–10, atau NVIDIA Kepler/Maxwell/Pascal dengan driver Vulkan 1.1/1.2.
   - **RAM & Storage:** Minimum 8 GB RAM; Rekomendasi 16 GB+ RAM & SSD/NVMe (Btrfs mendukung zero-copy `FICLONE` reflink prefix sync).
3. **Dependensi Library Host per Distro:** Perintah install paket ringkas untuk Arch, Fedora, Ubuntu/Debian, dan openSUSE (tanpa `steam-runtime` maupun `vulkaninfo`).
4. **Instalasi Lintas Launcher:** `HyperX-Launcher` (`./scripts/install-local.sh`), `Steam` (`--target steam`), `Heroic` (`--target heroic`), serta path manual `Lutris`.
5. **Environment Controls & Custom Game Rules JSON:** Daftar env opt-in/opt-out utama + contoh singkat `~/.config/hyperx-proton/rules/my-game.json`.

- [ ] **Step 2: Verifikasi markdown & commit**

Run: `git diff --stat README.md`
```bash
git add README.md
git commit -m "docs(readme): document universal system requirements, distro packages, and multi-launcher setup"
```

---

### Task 5: Live Universal Wine Rebuild, Packaging, Local Install, & Full ELF/Runtime Audit

**Files:**
- None (build & verification; hanya ubah `scripts/build-wine.sh` jika compiler melapor flag issue).

**Interfaces:**
- Consumes: seluruh perubahan Task 1–4.
- Produces: runner universal `x86-64` bebas `libav*`/`libunwind*` terpasang di `~/.local/share/hyperx-launcher/runners/HyperX-Proton/current`.

- [ ] **Step 1: Rebuild Rust release binary dan Wine universal (`x86-64`, `--without-ffmpeg --without-unwind`)**

Run:
```bash
./scripts/build.sh --component rust
./scripts/build.sh --component wine
```
Expected: `build/.stamps/wine.stamp` mencatat `x86-64:portable-v1` dan `build/stage/wine/usr/bin/wine` terbentuk bersih.

- [ ] **Step 2: Kemas ulang runner dan install ke lokal**

Run:
```bash
./scripts/package-runner.sh
./scripts/install-local.sh
```
Expected: `dist/hyperx-proton-11.0-20261125` memiliki `hyperx-native.json`, `compatibilitytool.vdf`, dan `toolmanifest.vdf`; terpasang di `~/.local/share/hyperx-launcher/runners/HyperX-Proton/current`.

- [ ] **Step 3: Verifikasi menyeluruh (ELF SONAME audit, `cargo test`, `packaging_smoke.sh`, dan live `proton doctor`)**

Run:
```bash
cargo test
bash tests/packaging_smoke.sh
RUNNER="$HOME/.local/share/hyperx-launcher/runners/HyperX-Proton/current"
# 1. Confirm ZERO fragile distro SONAMEs across all Wine binaries & Unix .so
for f in "$RUNNER/files/bin/wine" "$RUNNER/files/bin/wineserver" "$RUNNER/files/lib/wine/x86_64-unix/"*; do
    [[ -f "$f" ]] && readelf -d "$f" 2>/dev/null | grep '(NEEDED)' | sed -E 's/.*\[(.*)\]/\1/'
done | sort -u | tee /tmp/hx-wine-needed.txt
! grep -E 'libav(codec|format|util)|libunwind' /tmp/hx-wine-needed.txt
rm -f /tmp/hx-wine-needed.txt
# 2. Confirm live proton doctor & VDF metadata
"$RUNNER/proton" version
"$RUNNER/proton" doctor
ls -l "$RUNNER/compatibilitytool.vdf" "$RUNNER/toolmanifest.vdf" "$RUNNER/hyperx-native.json"
```
Expected:
- `cargo test`: 100% PASS, 0 warning.
- `packaging_smoke.sh`: `ALL CHECKS PASSED`.
- Zero `libavcodec`, `libavformat`, `libavutil`, `libunwind` di seluruh `DT_NEEDED` Wine.
- `proton doctor`: **0 `[FAIL]` / 0 `[WARN]`**, menampilkan `host:glibc` `[OK]`, `host:vulkan` terdeteksi via `dlopen`/`dlinfo`, `gpu:vulkan`, `gpu:dxvk-backend`, `rules:engine`.
