# Desain HyperX-Proton Fase 3: Portabilitas Universal x86_64 & Lintas Distro (Level 1)

**Tanggal:** 2026-11-26  
**Repositori:** `/home/budicuy/Dokumen/PROJEK/hyperx-proton`  
**Status:** Disetujui

---

## 1. Tujuan & Alasan (Intent & Why)

Setelah Fase 1 (Rust runtime tuning) dan Fase 2 (multi-GPU AMD/Intel/NVIDIA + DXVK-Sarek + Rust rule engine), hasil audit binary menunjukkan empat penghalang portabilitas bila `HyperX-Proton` dijalankan di CPU atau distro Linux lain:
1. Wine masih di-compile dengan `-march=alderlake` (berisiko `SIGILL` di AMD Ryzen lama maupun Intel Gen < 12).
2. `winedmo.so` dan `ntdll.so` masih nge-link `libavcodec.so.63`/`libavformat.so.63`/`libavutil.so.61` (FFmpeg bleeding-edge Arch) dan `libunwind.so.8` yang SONAME/ketersediaannya berbeda di Ubuntu/Debian/Fedora/openSUSE.
3. `proton doctor` memeriksa library hanya di `/usr/lib` dan `/usr/lib64` sehingga menghasilkan *false FAIL* di distro multiarch (`/usr/lib/x86_64-linux-gnu` di Ubuntu/Debian/Mint, atau NixOS).
4. Kernel tanpa `/dev/ntsync` dan tanpa `futex_waitv` belum memiliki fallback `WINEESYNC=1`, belum ada metadata `compatibilitytool.vdf`/`toolmanifest.vdf` untuk Steam/Heroic/Lutris, dan `README.md` belum mencantumkan syarat minimum & rekomendasi sistem lintas distro.

Tujuan Fase 3 (Level 1) adalah menjadikan `HyperX-Proton` **universal untuk semua CPU `x86_64`** (tanpa tuning CPU khusus) dan **siap jalan lintas distro Linux modern (`glibc >= 2.38`)**, termasuk drop-in di **HyperX-Launcher, Steam, Heroic, dan Lutris** tanpa mengembalikan `steam-runtime` atau `umu-launcher`, serta mendokumentasikan syarat sistem secara jujur dan lengkap di `README.md`.

---

## 2. Goals, Non-Goals, dan Anti-Goals

### Goals
- **Universal CPU baseline `x86-64`:** Seluruh build default menggunakan `-march=x86-64 -mtune=generic` (di `scripts/common.sh`, `scripts/build-wine.sh`, dan `packaging/arch/PKGBUILD`), tanpa `-march=alderlake` atau `x86-64-v3` default.
- **Wine bebas SONAME rapuh antar-distro:** Rebuild `wine-cachyos` dengan `--without-ffmpeg --without-unwind` sehingga pemutaran media memakai GStreamer (`libgstreamer-1.0.so.0`) dan unwinder memakai `libgcc_s.so.1` standar (nol `libavcodec`/`libavformat`/`libavutil`/`libunwind` di `DT_NEEDED`).
- **Sanitasi `LD_LIBRARY_PATH` Steam Scout Runtime (`src/runtime.rs`):** Ketika Steam meluncurkan custom Proton tanpa `require_tool_appid`, Steam menyuntikkan `ubuntu12_32/steam-runtime/...` ke `LD_LIBRARY_PATH` (dan menyimpan nilai asli host di `SYSTEM_LD_LIBRARY_PATH`). `HyperX-Proton` otomatis memulihkan `SYSTEM_LD_LIBRARY_PATH` / menyaring entri `steam-runtime` agar Wine host-linked tidak crash ABI di Steam standar (tetap menjaga `LD_PRELOAD` Steam Overlay & opt-out `HYPERX_PROTON_KEEP_STEAM_RUNTIME_LD=1`).
- **Dukungan verb `runinprefix` (`src/main.rs`):** Mendukung `proton runinprefix <exe> [args...]` (dipakai Steamworks Common Redistributables, `protontricks`, serta Winecfg/Winetricks/"Run EXE on prefix" di Heroic) sebagai helper prefix ringan (tanpa game rules, `steam.exe` shim, shader pre-warm, maupun thread supervisor).
- **`proton doctor` lintas distro (`dlopen` + `dlinfo`) & gating `glibc >= 2.38`:** Mengganti pencarian path hardcoded `/usr/lib` dengan `libc::dlopen(..., RTLD_LAZY | RTLD_LOCAL)` + `dlinfo(RTLD_DI_LINKMAP)` agar otomatis mendukung Arch, Fedora, Ubuntu/Debian/Mint (`/usr/lib/x86_64-linux-gnu`), openSUSE, SteamOS, dan NixOS, melaporkan tier sync aktif, serta mem-parse versi `glibc` host dan mengeluarkan `[FAIL]` bila `< 2.38` (sesuai batas aktual `VERNEED` `GLIBC_2.38` pada `wineserver`/`ntdll.so`).
- **Fallback sync kernel 3-tingkat (`ntsync → fsync → esync`):** Mendeteksi `/dev/ntsync` dan syscall `futex_waitv` (`SYS_futex_waitv = 449` pada x86_64) sehingga kernel lama tanpa `futex_waitv` (atau saat `PROTON_NO_FSYNC=1`) otomatis jatuh ke `WINEESYNC=1` (dengan opt-out `PROTON_NO_ESYNC=1`).
- **Drop-in lintas launcher:** `scripts/package-runner.sh` menghasilkan `compatibilitytool.vdf` dan `toolmanifest.vdf` (di samping `hyperx-native.json`), dan `scripts/install-local.sh` mendukung `--target hyperx|steam|heroic` (default tetap `hyperx`).
- **`README.md` lengkap dengan Syarat Minimum & Rekomendasi Sistem:** Mencakup CPU, GPU (Vulkan 1.3+ vs 1.1/1.2 DXVK-Sarek), kernel/sync, `glibc >= 2.38`, daftar distro didukung, paket host per distro (Arch, Fedora, Ubuntu/Debian, openSUSE), cara install per launcher, dan contoh custom JSON rule.

### Non-Goals
- **Tidak** membangun Rust Vulkan frame-pacing/limiter layer (pengaturan frame cap/HUD diserahkan ke user).
- **Tidak** membangun build-time container Ubuntu 22.04 / Debian 12 (`glibc 2.35`, Level 2) pada fase ini.
- **Tidak** membundle library sistem (`libc`, `libvulkan`, `libGL`, `libdrm`, `libavcodec`) ke dalam tarball.
- **Tidak** mengimplementasikan verb destruktif `destroydefaultpfx` (prefix user tetap dijaga non-destruktif).

### Anti-Goals
- Menyisakan satu pun binary yang memakai instruksi khusus `alderlake` / `AVX2` wajib atau `DT_NEEDED` ke `libavcodec*`/`libavformat*`/`libavutil*`/`libunwind*`.
- Merusak perilaku existing `hyperx-launcher` (`hyperx-native.json` tetap `schema_version: 1`, default `install-local.sh` tetap memasang ke `hyperx-launcher`, seluruh 28 test Rust + `packaging_smoke.sh` tetap hijau).

---

## 3. Pendekatan yang Dipertimbangkan (Approaches)

1. **Pendekatan A — Clean Universal Configure (`--without-ffmpeg --without-unwind` + `-march=x86-64 -mtune=generic` + `dlopen` Doctor + Sync 3-Tingkat + VDF) *(Dipilih)***
   - Menghilangkan dependency FFmpeg/libunwind langsung pada configure Wine (media tetap berjalan melalui GStreamer, unwind melalui `libgcc_s.so.1`) dan menggunakan baseline `x86-64` murni.
   - *Trade-off:* Memerlukan 1× rebuild Wine (~5–8 menit dengan `ccache`).
2. **Pendekatan B — Bundle `.so` FFmpeg & libunwind dari host ke `files/lib/compat/`**
   - *Trade-off:* Tetap harus rebuild Wine untuk menghilangkan `-march=alderlake`, dan membundle `libavcodec` menarik puluhan dependency transitif yang rawan bentrok ABI di distro lain.

---

## 4. Ringkasan Perubahan Teknis

1. **Build Universal & Clean SONAME (`scripts/common.sh`, `scripts/build-wine.sh`, `packaging/arch/PKGBUILD`):**
   - `HYPERX_MARCH="${HYPERX_MARCH:-x86-64}"` di `scripts/common.sh` dan `packaging/arch/PKGBUILD`.
   - `CFLAGS="-O2 -march=${HYPERX_MARCH} -mtune=generic -pipe"` (sama untuk `CXXFLAGS`) di `scripts/build-wine.sh`, dan tambahkan `--without-ffmpeg --without-unwind` ke argumen `configure` (stamp key menyertakan `portable-v1` agar rebuild pasti terpicu).
2. **3-Tier Kernel Sync & Sanitasi `LD_LIBRARY_PATH` Steam (`src/runtime.rs`, `src/main.rs`):**
   - Tambah `pub fn is_fsync_available() -> bool`: probe `libc::syscall(449, ptr::null::<c_void>(), 0, 0, ptr::null::<c_void>(), 0)`; bila gagal dengan `ENOSYS`, return `false`, selain itu (`EINVAL`/`EFAULT`/0) return `true`.
   - Urutan resolusi sync via `pub fn resolve_sync_mode(ntsync_avail: bool, fsync_avail: bool, no_ntsync: bool, no_fsync: bool, no_esync: bool) -> SyncMode`:
     1. `ntsync_avail && !no_ntsync` → `PROTON_USE_NTSYNC=1`, `WINEFSYNC=0`, `WINEESYNC=0`
     2. `else if fsync_avail && !no_fsync` → `PROTON_USE_NTSYNC=0`, `WINEFSYNC=1`, `WINEESYNC=0`
     3. `else if !no_esync` → `PROTON_USE_NTSYNC=0`, `WINEFSYNC=0`, `WINEESYNC=1`
     4. `else` → semua `0` (plain wineserver sync).
   - Wrapper lama `build_runtime_env(...)` dan `build_runtime_env_with_context(...)` tetap kompatibel (meneruskan `fsync_available = true`), plus `build_runtime_env_with_sync(...)` untuk menerima `fsync_available` eksplisit.
   - **Sanitasi `LD_LIBRARY_PATH` Steam Scout Runtime (keputusan desain dari builder review):**
     - Fungsi murni `sanitize_steam_runtime_ld_library_path(ld_library_path: Option<&str>, system_ld_library_path: Option<&str>, keep_steam_runtime: bool) -> Option<String>`:
       - Bila `keep_steam_runtime` (`HYPERX_PROTON_KEEP_STEAM_RUNTIME_LD=1`), biarkan `LD_LIBRARY_PATH` apa adanya.
       - Bila `LD_LIBRARY_PATH` mengandung segmen `steam-runtime` (atau `ubuntu12_32` / `ubuntu12_64`):
         - Jika `SYSTEM_LD_LIBRARY_PATH` ada di `host_env`, pulihkan nilainya (hapus `LD_LIBRARY_PATH` bila `SYSTEM_LD_LIBRARY_PATH` kosong).
         - Jika `SYSTEM_LD_LIBRARY_PATH` tidak ada, buang hanya segmen path yang mengandung `steam-runtime`, `ubuntu12_32`, atau `ubuntu12_64`, dan pertahankan segmen host lain (hapus `LD_LIBRARY_PATH` bila hasilnya kosong).
       - `LD_PRELOAD` tidak disentuh agar Steam Overlay / MangoHud tetap berjalan normal.
   - **Verb `runinprefix` (`src/main.rs` — keputusan desain dari builder review):**
     - `proton runinprefix <exe> [args...]` menjalankan helper prefix ringan: mengambil `acquire_prefix_lock`, menjalankan `wineboot -u` + `sync_prefix_dlls_with_options` hanya bila prefix belum siap/berubah, lalu langsung mengeksekusi `wine <exe> [args...]` **tanpa** game rules, `steam.exe` shim, `ensure_files`, shader pre-warm, maupun thread supervisor, dan mengembalikan exit code proses tersebut.
3. **Cross-Distro `proton doctor` & Gating `glibc >= 2.38` (`src/doctor.rs`):**
   - `find_host_lib(lib_name)` menggunakan `libc::dlopen(lib_name, RTLD_LAZY | RTLD_LOCAL)` + `dlinfo(RTLD_DI_LINKMAP)` untuk mendapatkan path aktual (fallback ke nama library bila `l_name` kosong), lalu selalu `dlclose`.
   - Tambah `parse_glibc_version(raw: &str) -> Option<(u32, u32)>` dan `check_glibc_floor(raw: &str) -> DoctorCheck` (keputusan desain dari builder review): membaca `gnu_get_libc_version()`, menghasilkan `[OK]` bila `>= (2, 38)` dan `[FAIL]` bila `< (2, 38)` dengan pesan jelas bahwa Wine membutuhkan `glibc >= 2.38`.
   - Perbarui check `sync` agar menampilkan tier aktual (`ntsync` / `fsync` / `esync`).
4. **Cross-Launcher Packaging & Installer (`scripts/package-runner.sh`, `scripts/install-local.sh`, `tests/packaging_smoke.sh`):**
   - `package-runner.sh` menulis `compatibilitytool.vdf` dan `toolmanifest.vdf` di root `$RUNNER_DIR` (menggunakan `HYPERX_PROTON_VERSION`, `commandline = "/proton %verb%"`, `require_tool_appid` **tidak** diset karena zero-SteamRT).
   - `install-local.sh` mendukung `--target hyperx|steam|heroic` (default `hyperx`).
   - `tests/packaging_smoke.sh` memverifikasi `compatibilitytool.vdf` dan `toolmanifest.vdf` ada dan valid.
5. **Dokumentasi Syarat Sistem (`README.md`):**
   - Tambah bagian **Syarat Minimum & Rekomendasi Sistem** (CPU universal x86_64, RAM, GPU Vulkan 1.3+ vs 1.1/1.2 DXVK-Sarek, Kernel/Sync 3-tingkat, `glibc >= 2.38`, daftar distro modern yang didukung termasuk Ubuntu 24.04+, Fedora 39+, Debian 13+, Arch/CachyOS, openSUSE Tumbleweed, Bazzite/Nobara, SteamOS 3.6+, daftar paket host per distro, panduan install HyperX-Launcher/Steam/Heroic/Lutris, dan cara pakai custom JSON rules).
6. **Rebuild & Verifikasi Live:**
   - Jalankan `./scripts/build.sh --component wine`, `--component rust`, `package-runner.sh`, `install-local.sh`.
   - Verifikasi `readelf -d` di seluruh `files/lib/wine/x86_64-unix/*.so` memiliki **0** `libavcodec`/`libavformat`/`libavutil`/`libunwind`, `cargo test` PASS, `packaging_smoke.sh` PASS, dan live `proton doctor` 0 `[FAIL]` / 0 `[WARN]`.
