# Desain Pembersihan Tuning Opsional HyperX-Proton (Pure Neutral Runner)

- **Tanggal:** 2026-11-26
- **Status:** Disetujui
- **Repositori:** `/home/budicuy/Dokumen/PROJEK/hyperx-proton`

## Latar Belakang & Tujuan

`HyperX-Proton` sebelumnya menyertakan modul optimasi/tuning performa agresif Fase 1 (`supervisor`, `shader_warm`, `topology`, `sys_tune`, serta injeksi otomatis env shader cache/thread/logging) yang membuat perilaku runner berbeda dari Proton murni dan berpotensi tumpang tindih dengan kebijakan scheduler kernel, driver GPU, atau launcher/container masa depan.

Tujuan perubahan ini adalah **mencabut seluruh tuning performa opsional** agar `HyperX-Proton` kembali menjadi **fork Proton yang murni, netral, dan universal**, sambil **mempertahankan 100% fitur wajib** untuk kompatibilitas game dan portabilitas lintas-distro/hardware.

## Ruang Lingkup

### 1. Fitur Opsional yang Dicabut
- **Modul & tes yang dihapus:**
  - `src/supervisor.rs` (`pidfd_open` thread supervisor & auto-nice thread game)
  - `src/shader_warm.rs` (background `posix_fadvise` shader cache pre-warmer)
  - `src/topology.rs` (deteksi physical core `/sys/devices/system/cpu` & kalkulasi thread compiler)
  - `src/sys_tune.rs` (`PR_SET_TIMERSLACK`, `PR_SET_CHILD_SUBREAPER`; dua helper non-tuning yang wajib tetap dipindahkan, lihat bagian 2)
  - `tests/phase1_performance.rs`
- **Injeksi env otomatis yang dicabut dari `src/runtime.rs`:**
  - `HYPERX_PROTON_SHADER_THREADS`
  - `DXVK_CONFIG` (`dxvk.numCompilerThreads = ...` tidak lagi di-append otomatis)
  - `VKD3D_WORKER_THREADS`
  - `DXVK_STATE_CACHE`
  - `MESA_SHADER_CACHE_DISABLE`, `MESA_SHADER_CACHE_MAX_SIZE`
  - `__GL_SHADER_DISK_CACHE`, `__GL_SHADER_DISK_CACHE_SKIP_CLEANUP`, `__GL_SHADER_DISK_CACHE_SIZE`
  - `DXVK_LOG_LEVEL`, `VKD3D_DEBUG`
- **Env override yang dihapus dari `src/main.rs` & `README.md`:**
  - `HYPERX_PROTON_NO_TUNE`, `HYPERX_PROTON_TIMERSLACK_NS`, `HYPERX_PROTON_NO_PREWARM`, `HYPERX_PROTON_NO_SUPERVISOR`, `HYPERX_PROTON_SHADER_THREADS`
- **Diagnostik `doctor` yang dihapus dari `src/doctor.rs`:**
  - `cpu:topology`
  - `kernel:pidfd`

### 2. Fitur Wajib yang Dipertahankan
- **Kompatibilitas Game & HoYoVerse/WuWa:**
  - Rule engine (`src/rules.rs`: WuWa, HSR, Genshin, ZZZ + custom JSON rules + `User > Rule > Defaults`)
  - Conditional `steam.exe` shim (`files/lib/wine/x86_64-windows/steam.exe`)
  - ICU 68.2 DLLs (`icudt68.dll`, `icuin68.dll`, `icuuc68.dll`)
  - `usebitmap.flag` (`apply_ensure_files`)
  - `WINE_DISABLE_VULKAN_OPWR=1` (kecuali di-override rule/user)
- **Portabilitas Multi-GPU & Lintas-Distro:**
  - Native `dlopen` Vulkan GPU probe (`src/gpu.rs`) untuk fallback DXVK-Sarek `v1.14.1` (Vulkan 1.1/1.2) dan NVAPI vendor guard (blokir `nvapi64.dll` pada AMD/Intel)
  - 3-tier kernel sync (`ntsync` → `fsync` → `esync` → `plain`)
  - Sanitasi `LD_LIBRARY_PATH` Steam Scout Runtime (`sanitize_steam_runtime_ld_library_path`)
  - Verb `proton runinprefix`
  - `proton doctor` lintas-distro (`dlopen` + `dlinfo`, floor `glibc >= 2.38`)
  - Helper wajib yang dipindahkan secara rapi:
    - `reflink_or_copy` (Btrfs/XFS `FICLONE` CoW fallback `io::copy`) → dipindah ke `src/prefix.rs`
    - `raise_nofile_limit(524_288)` (pelindung agar `WINEESYNC=1` tidak crash `EMFILE` pada distro dengan soft limit 1024) → dipindah ke `src/main.rs` (dipanggil sebelum menjalankan Wine pada `run_launch` & `run_in_prefix`)

## Risiko & Verifikasi

- **Zero Wine Rebuild:** Semua artefak native (`wine`, `dxvk`, `dxvk-sarek`, `vkd3d`, `nvapi`, `icu`, `steam.exe`) sudah di-stage di `build/stage/`, sehingga hanya launcher Rust `proton` yang di-rebuild dan di-repack.
- **Kriteria Selesai:**
  1. `cargo test` 100% lulus tanpa warning kompilasi.
  2. `cargo build --release` sukses tanpa warning.
  3. `bash tests/packaging_smoke.sh` lulus semua cek.
  4. `./scripts/package-runner.sh && ./scripts/install-local.sh` sukses.
  5. `proton doctor` live mengembalikan exit code 0 (0 `[FAIL]`, 0 `[WARN]`) tanpa baris `cpu:topology` atau `kernel:pidfd`.
