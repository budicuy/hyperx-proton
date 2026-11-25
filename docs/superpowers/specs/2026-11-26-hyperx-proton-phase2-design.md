# Desain HyperX-Proton Fase 2: Kompatibilitas Multi-Hardware (AMD / Intel / NVIDIA) & Rust Native Rule Engine

**Tanggal:** 2026-11-26  
**Repositori:** `/home/budicuy/Dokumen/PROJEK/hyperx-proton`  
**Status:** Disetujui

---

## 1. Tujuan & Alasan (Intent & Why)

Pada Fase 1, `HyperX-Proton` sudah menjadi runner native Arch Linux (nol SteamRT, nol `umu-launcher`) dengan Rust tuning engine (`PR_SET_TIMERSLACK`, `RLIMIT_NOFILE`, Btrfs `FICLONE`, async shader pre-warm, dan `pidfd` thread supervisor). Namun masih ada dua keterbatasan besar sebelum runner ini layak dipakai lintas mesin dan siap untuk rilis open-source:

1. **Bias single-GPU NVIDIA:** `src/runtime.rs` selalu menyuntikkan env shader cache NVIDIA (`__GL_SHADER_DISK_CACHE*`) tanpa memeriksa vendor GPU host, `PROTON_ENABLE_NVAPI=1` belum memiliki guard vendor untuk AMD/Intel, `src/shader_warm.rs` belum me-preload cache shader Mesa (`RADV`/`ANV`), dan GPU lawas dengan Vulkan 1.1/1.2 tidak bisa memakai DXVK `v3.1.1` (yang mensyaratkan Vulkan 1.3).
2. **Game fixes hardcoded & shim terlalu luas:** Fix Wuthering Waves (`SteamGameId=3513350` dan `usebitmap.flag`) masih ditulis langsung di `src/main.rs`, dan `steam.exe` shim masih dijalankan untuk semua game padahal hanya dibutuhkan anti-cheat HoYoProtect (HSR, Genshin, ZZZ).

Fase 2 menyelesaikan keduanya secara native di Rust agar `HyperX-Proton` bekerja optimal dan aman di **AMD, Intel, dan NVIDIA** (baik GPU modern Vulkan 1.3+ maupun GPU lawas Vulkan 1.1/1.2) serta memiliki pengganti `protonfixes` yang ringan, deterministik, dan dapat diperluas pengguna tanpa recompile.

---

## 2. Goals, Non-Goals, dan Anti-Goals

### Goals
- **Auto-detect GPU & Vulkan native di Rust (`src/gpu.rs`):** Deteksi vendor (`NVIDIA`, `AMD`, `Intel`, `Other`), tipe perangkat (`Discrete`, `Integrated`, `Virtual`, `Cpu`), versi Vulkan API, dan ekstensi kunci (`VK_EXT_graphics_pipeline_library`, `VK_KHR_present_wait`, `VK_EXT_shader_module_identifier`) langsung lewat `libvulkan.so.1` tanpa crate tambahan dan tanpa binary `vulkaninfo`.
- **Tuning vendor-aware lintas AMD, Intel, dan NVIDIA:**
  - NVIDIA: mempertahankan konfigurasi `__GL_SHADER_DISK_CACHE*` (cap 2 GiB), pre-warm `GLCache`, dan mengizinkan opt-in NVAPI.
  - AMD & Intel (Mesa RADV/ANV): tidak menyuntikkan env NVIDIA, memblokir NVAPI secara default, mengatur `MESA_SHADER_CACHE_MAX_SIZE=2G`, dan pre-warm cache shader Mesa.
- **Bundling & auto-fallback DXVK-Sarek (`v1.14.1`):** Ikut mem-build `d3d11.dll` + `dxgi.dll` dari DXVK-Sarek ke `files/lib/dxvk-sarek/x86_64-windows/` dan memilihnya otomatis pada GPU Vulkan 1.1/1.2 (atau override manual), dengan stamp backend terpisah di prefix agar perpindahan backend aman dan idempoten.
- **Rust Native Game Rule Engine (`src/rules.rs`):** Memindahkan aturan WuWa dari `src/main.rs`, menyertakan built-in rules untuk WuWa, HSR, Genshin, dan ZZZ, mendukung drop-in custom JSON rules pengguna, dan menjadikan `steam.exe` shim **kondisional** per game.
- **Diagnostik lengkap di `proton doctor`:** Menampilkan GPU terpilih, versi Vulkan, ekstensi penting, backend DXVK terpilih, ketersediaan bundle `dxvk-sarek`, dan ringkasan rule engine.

### Non-Goals
- **Vulkan Layer in-process (`VK_KHR_present_wait` frame pacing / dynamic limiter):** Dicadangkan untuk Fase 3.
- **Dukungan DX8 / DX9 / DX10 / DDraw:** Runner tetap ketat hanya mengemas `d3d11.dll`, `dxgi.dll`, `d3d12.dll`, `d3d12core.dll` (plus `nvapi64.dll` opt-in).
- **Integrasi Python, `protonfixes`, `vulkan-tools`, `steam-runtime`, atau `umu-launcher`.**
- **Perubahan breaking pada `hyperx-native.json` `schema_version`:** Tetap `schema_version: 1`.

### Anti-Goals (Kegagalan Fungsional)
- Menggagalkan peluncuran game bila `libvulkan.so.1` tidak tersedia, host headless, atau file JSON rule pengguna korup — seluruh fitur Fase 2 **wajib fail-soft**.
- Menimpa env var atau `WINEDLLOVERRIDES` yang diset eksplisit oleh pengguna atau `hyperx-launcher`.
- Merusak prefix existing (`user.reg`, `system.reg`, `drive_c/users/`) atau memicu `wineboot -u` ulang hanya karena berpindah backend DXVK Standard ↔ Sarek.
- Meninggalkan `nvapi64.dll` aktif di dalam prefix ketika dijalankan pada GPU AMD atau Intel.

---

## 3. Pendekatan yang Dipertimbangkan (Approaches)

1. **Pendekatan A — Native Zero-Bloat (`dlopen` Vulkan Probe + Bundle DXVK-Sarek `v1.14.1` + Hybrid Built-in & JSON Rule Engine) *(Dipilih)***
   - Memanggil 6 simbol C ABI Vulkan dari `libvulkan.so.1` via `libc::dlopen`/`dlsym` (nol crate baru, nol `vulkaninfo`).
   - Mem-build dan mengemas DXVK-Sarek `v1.14.1` (`d3d11.dll` + `dxgi.dll`) bersama DXVK `v3.1.1` sehingga GPU Vulkan 1.1/1.2 langsung jalan *out-of-the-box*.
   - Rule engine hybrid: built-in rules di Rust + drop-in `*.json` di direktori config pengguna.
   - *Trade-off:* Tambahan ~1–2 menit pada build awal dan ~4–5 MB di tarball runner; ~180 baris FFI Vulkan terisolasi di `src/gpu.rs`.
2. **Pendekatan B — Crate `ash` + Slot DXVK-Sarek Kosong (Tanpa Bundle)**
   - *Trade-off:* Menarik crate Vulkan besar hanya untuk query awal dan membuat pengguna GPU Vulkan 1.1/1.2 harus compile DXVK-Sarek sendiri secara manual.
3. **Pendekatan C — Wrapper CLI `vulkaninfo --json` + Rule Hardcoded di Rust**
   - *Trade-off:* Bergantung pada paket `vulkan-tools`, menambah latency spawn proses tiap launch, dan tidak bisa diperluas pengguna tanpa recompile.

**Alasan Pemilihan:** Pengguna menginginkan rekomendasi terbaik demi kompatibilitas maksimal lintas AMD, Intel, dan NVIDIA tanpa memutus filosofi native zero-bloat `HyperX-Proton`.

---

## 4. Arsitektur & Spesifikasi Komponen

### 4.1. Native Vulkan GPU Prober (`src/gpu.rs`)

#### Tipe Data
```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GpuVendor {
    Nvidia,      // 0x10DE
    Amd,         // 0x1002
    Intel,       // 0x8086
    Other(u32),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GpuDeviceType {
    Discrete,    // VK_PHYSICAL_DEVICE_TYPE_DISCRETE_GPU (2)
    Integrated,  // VK_PHYSICAL_DEVICE_TYPE_INTEGRATED_GPU (1)
    Virtual,     // VK_PHYSICAL_DEVICE_TYPE_VIRTUAL_GPU (3)
    Cpu,         // VK_PHYSICAL_DEVICE_TYPE_CPU (4)
    Other,       // 0 / unknown
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GpuProfile {
    pub vendor: GpuVendor,
    pub vendor_id: u32,
    pub device_id: u32,
    pub device_name: String,
    pub device_type: GpuDeviceType,
    pub api_major: u32,
    pub api_minor: u32,
    pub api_patch: u32,
    pub has_gpl: bool,               // VK_EXT_graphics_pipeline_library
    pub has_present_wait: bool,      // VK_KHR_present_wait
    pub has_shader_module_id: bool,  // VK_EXT_shader_module_identifier
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GpuProbeResult {
    Detected(GpuProfile),
    Unavailable(String),
}
```

#### Mekanisme Probing (`probe_primary_gpu(host_env)`)
1. Jika `HYPERX_PROTON_NO_GPU_PROBE=1`, langsung kembalikan `GpuProbeResult::Unavailable("disabled by HYPERX_PROTON_NO_GPU_PROBE=1")`.
2. Muat `libvulkan.so.1` via `libc::dlopen(c"libvulkan.so.1", RTLD_LAZY | RTLD_LOCAL)`. Bungkus handle dengan RAII guard agar selalu di-`dlclose`.
3. Ambil simbol via `libc::dlsym`:
   - `vkCreateInstance`
   - `vkDestroyInstance`
   - `vkEnumeratePhysicalDevices`
   - `vkGetPhysicalDeviceProperties`
   - `vkEnumerateDeviceExtensionProperties`
4. Buat `VkInstance` minimal (nol layer, nol ekstensi, `pApplicationInfo.apiVersion = VK_MAKE_API_VERSION(0, 1, 1, 0)`). Jika mengembalikan `VK_ERROR_INCOMPATIBLE_DRIVER` (`-9`), coba ulang sekali dengan `VK_MAKE_API_VERSION(0, 1, 0, 0)`. Bungkus `VkInstance` dengan RAII guard untuk `vkDestroyInstance`.
5. Enumerasi seluruh `VkPhysicalDevice`. Untuk setiap perangkat, baca `VkPhysicalDeviceProperties` (`apiVersion`, `vendorID`, `deviceID`, `deviceType`, `deviceName`) dan `vkEnumerateDeviceExtensionProperties(dev, NULL, ...)` untuk menandai `has_gpl`, `has_present_wait`, dan `has_shader_module_id`.
6. Pilih GPU utama lewat fungsi murni `select_primary_gpu(&devices, dxvk_filter_device_name, mesa_vk_device_select)`:
   - **Filter eksplisit 1 (`DXVK_FILTER_DEVICE_NAME`):** Jika diset dan tidak kosong, pilih kandidat yang `device_name` mengandung substring tersebut (case-insensitive).
   - **Filter eksplisit 2 (`MESA_VK_DEVICE_SELECT`):** Jika berformat `<hex_vendor>:<hex_device>` (abaikan suffix `!` atau indeks setelahnya), pilih kandidat dengan `vendor_id` dan `device_id` yang cocok.
   - **Skor default (multi-GPU / laptop hybrid):** `Discrete (300) > Integrated (200) > Virtual (100) > Other (50) > Cpu (10)`. Tie-breaker: `(api_major, api_minor, api_patch)` tertinggi, lalu urutan indeks terkecil.
7. Jika tidak ada device yang ditemukan atau simbol gagal dimuat, kembalikan `GpuProbeResult::Unavailable(reason)` secara *fail-soft*.

---

### 4.2. Seleksi Backend DXVK & Bundling DXVK-Sarek (`src/gpu.rs`, `src/prefix.rs`, `scripts/`)

#### Aturan Pemilihan Backend (`choose_dxvk_backend`)
```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DxvkBackend {
    Standard, // files/lib/dxvk/x86_64-windows
    Sarek,    // files/lib/dxvk-sarek/x86_64-windows
}
```
Prioritas penentuan:
1. **`HYPERX_PROTON_DXVK_BACKEND`** (env eksplisit pengguna):
   - `"standard"` → `Standard`.
   - `"sarek"` → `Sarek` jika `files/lib/dxvk-sarek/x86_64-windows/{d3d11.dll,dxgi.dll}` ada; jika tidak ada, fallback `Standard` + warning ke `stderr`.
2. **Rule override (`rule.dxvk_backend`)** (bila ada dan bundle tersedia).
3. **Mode `auto` (default):**
   - Jika `GpuProbeResult::Detected(gpu)` dengan `(gpu.api_major, gpu.api_minor) < (1, 3)` **dan** bundle `dxvk-sarek` tersedia → pilih `Sarek`.
   - Jika `Detected(gpu)` dengan `(api_major, api_minor) < (1, 3)` tetapi bundle `dxvk-sarek` tidak ada → fallback `Standard` + warning jelas ke `stderr`.
   - Selain itu (Vulkan `1.3+`, atau `Unavailable`) → pilih `Standard`.

#### Sinkronisasi Prefix Aman (`src/prefix.rs`)
- Tambah stamp file `.hyperx-proton-dxvk-backend` di dalam `layout.wineprefix` berisi `"standard\n"` atau `"sarek\n"`.
- `prefix_needs_wineboot()` tetap tidak terpengaruh oleh perubahan backend DXVK sehingga berpindah `Standard ↔ Sarek` **tidak** menjalankan `wineboot -u`.
- Fungsi baru `sync_prefix_dlls_with_options(runner_root, layout, runner_version, enable_nvapi, dxvk_backend)` (mempertahankan wrapper `sync_prefix_dlls(...)` untuk kompatibilitas backward dengan pemanggil/test yang ada):
  - Memicu re-copy DLL bila salah satu terpenuhi: versi runner berubah, backend DXVK pada stamp berubah, DLL target belum lengkap, atau `enable_nvapi` berubah.
  - Menyalin `d3d11.dll` + `dxgi.dll` dari `files/lib/dxvk/x86_64-windows` (`Standard`) atau `files/lib/dxvk-sarek/x86_64-windows` (`Sarek`).
  - Jika `enable_nvapi == false` dan `drive_c/windows/system32/nvapi64.dll` sudah ada di prefix (mis. bekas sesi GPU NVIDIA sebelumnya), hapus `nvapi64.dll` tersebut agar tidak aktif di GPU AMD/Intel.
  - Tidak pernah menyentuh `user.reg`, `system.reg`, atau `drive_c/users/`.

#### Build & Packaging DXVK-Sarek
- `versions.env`:
  ```bash
  DXVK_SAREK_REPO=https://github.com/pythonlover02/DXVK-Sarek.git
  DXVK_SAREK_REF=v1.14.1
  ```
- `scripts/build-dxvk-sarek.sh`: mengikuti pola `scripts/build-dxvk.sh` dengan stamp `dxvk-sarek:${DXVK_SAREK_REF}:dx11-only`, meson cross-compile MinGW64:
  `-Denable_ddraw=false -Denable_d3d8=false -Denable_d3d9=false -Denable_d3d10=false -Denable_d3d11=true -Denable_dxgi=true`, lalu menyalin hanya `d3d11.dll` dan `dxgi.dll` ke `$STAGE_DIR/dxvk-sarek/x86_64-windows/`.
- `scripts/build.sh`: mendukung `--component dxvk-sarek` dan memanggil `build-dxvk-sarek.sh` pada target `all`.
- `scripts/package-runner.sh`: menyalin `d3d11.dll` + `dxgi.dll` dari `$ARG_STAGE_DIR/dxvk-sarek/x86_64-windows/` ke `$RUNNER_DIR/files/lib/dxvk-sarek/x86_64-windows/` dan menambahkan `"dxvk_sarek": "${DXVK_SAREK_REF} (d3d11,dxgi)"` ke `components` di `hyperx-native.json`.
- `tests/packaging_smoke.sh`: menambah dummy stage `dxvk-sarek` (termasuk `ddraw.dll` / `d3d9.dll` terlarang untuk memastikan filtering ketat) dan memverifikasi hasil paketnya.

---

### 4.3. Vendor-Aware Runtime Env & Shader Pre-Warming (`src/runtime.rs`, `src/shader_warm.rs`)

#### Prioritas Env & Guard Vendor di `src/runtime.rs`
Tambah `build_runtime_env_with_context(runner_root, layout, host_env, ntsync_available, gpu_probe, rule_eval)` sambil mempertahankan wrapper `build_runtime_env(...)` (menggunakan `GpuProbeResult::Unavailable` + tanpa rule) agar kompatibilitas API & unit test lama terjaga.

**Urutan Pipeline (`User > Rule > Defaults` — keputusan desain):**
1. Mulai dari salinan `host_env` (nilai eksplisit user / `hyperx-launcher`).
2. Gabungkan `rule_eval.env` ke `effective_env` **hanya untuk key yang belum ada di `host_env`** (khusus `SteamGameId`, nilai kosong atau `"0"` di `host_env` dianggap belum diset sehingga boleh diisi oleh `rule_eval.steam_game_id`).
3. Jalankan seluruh evaluasi flag kontrol dan default runtime terhadap `effective_env` (termasuk `PROTON_NO_NTSYNC`, `PROTON_ENABLE_WAYLAND`, `PROTON_ENABLE_NVAPI` yang tetap melewati guard vendor, `WINE_DISABLE_VULKAN_OPWR`, penggabungan `dxvk.numCompilerThreads` ke `DXVK_CONFIG` yang mungkin berasal dari rule, `VKD3D_WORKER_THREADS`, serta default cache shader per vendor). Dengan urutan ini, rule game bisa meng-override default runtime (mis. `WINE_DISABLE_VULKAN_OPWR=0` atau `DXVK_CONFIG` khusus game), sementara env eksplisit user selalu menang mutlak.

Aturan per vendor:
1. **NVAPI Guard:**
   - User meminta NVAPI bila `PROTON_ENABLE_NVAPI=1` atau `HYPERX_PROTON_NVAPI=1` (atau `HYPERX_PROTON_FORCE_NVAPI=1`).
   - `nvapi_enabled` bernilai `true` hanya bila:
     - `HYPERX_PROTON_FORCE_NVAPI=1`, **atau**
     - user meminta NVAPI **dan** GPU adalah `Detected(GpuVendor::Nvidia)` atau `Unavailable`.
   - Jika GPU adalah `Amd` atau `Intel` (tanpa `FORCE_NVAPI`), `nvapi_enabled = false` dan `nvapi64=n` tidak ditambahkan ke `WINEDLLOVERRIDES`.
2. **Shader Cache Env per Vendor (selalu gunakan `.entry(...).or_insert_with(...)` agar tidak menimpa setting user):**
   - **`Nvidia` atau `Unavailable` (fallback aman):**
     - `__GL_SHADER_DISK_CACHE=1`
     - `__GL_SHADER_DISK_CACHE_SKIP_CLEANUP=1`
     - `__GL_SHADER_DISK_CACHE_SIZE=2147483647`
   - **`Amd` atau `Intel` (Mesa RADV / ANV):**
     - Tidak menyuntikkan `__GL_SHADER_DISK_CACHE*`.
     - `MESA_SHADER_CACHE_DISABLE=false`
     - `MESA_SHADER_CACHE_MAX_SIZE=2G`
3. **Compiler Threads & Logging:**
   - Tetap sama untuk semua vendor: `dxvk.numCompilerThreads = N` di `DXVK_CONFIG`, `VKD3D_WORKER_THREADS = N`, `DXVK_STATE_CACHE=1`, `DXVK_LOG_LEVEL=error`, `VKD3D_DEBUG=none`, `WINE_DISABLE_VULKAN_OPWR=1`.

#### Shader Cache Pre-Warming Lintas Vendor (`src/shader_warm.rs`)
- Tambah `collect_shader_cache_dirs_for_vendor(layout, env, exe_dir, vendor)` (dan pertahankan `collect_shader_cache_dirs` sebagai wrapper untuk backward compatibility):
  - **Umum (semua vendor):** `DXVK_STATE_CACHE_PATH`, `VKD3D_SHADER_CACHE_PATH`, `exe_dir`, `drive_c/users/steamuser/AppData/Local/dxvk`, `<compat>/shadercache`.
  - **NVIDIA (`Nvidia` / `None`):** `__GL_SHADER_DISK_CACHE_PATH`, `$HOME/.cache/nvidia/GLCache`.
  - **AMD / Intel (`Amd` / `Intel`):** `MESA_SHADER_CACHE_DIR`, `$XDG_CACHE_HOME/mesa_shader_cache`, `$XDG_CACHE_HOME/mesa_shader_cache_sf`, `$HOME/.cache/mesa_shader_cache`, `$HOME/.cache/mesa_shader_cache_sf`.
- Tambah ekstensi file cache Mesa (`.foz`, `.idx`) pada `is_shader_cache_file` dengan batas aman `MAX_PREWARM_FILES` dan `MAX_PREWARM_BYTES` yang sudah ada.

---

### 4.4. Rust Native Game Rule Engine (`src/rules.rs`)

#### Schema Rule (`GameRule`)
```rust
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct GameRule {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub match_criteria: RuleMatch,
    #[serde(default)]
    pub steam_game_id: Option<String>,
    #[serde(default)]
    pub env: BTreeMap<String, String>,
    #[serde(default)]
    pub dll_overrides: Vec<String>,
    #[serde(default)]
    pub use_steam_shim: Option<bool>,
    #[serde(default)]
    pub dxvk_backend: Option<String>, // "standard" | "sarek"
    #[serde(default)]
    pub ensure_files: Vec<EnsureFileRule>,
}

#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
pub struct RuleMatch {
    /// Semua substring harus cocok (case-insensitive) dengan path executable.
    #[serde(default)]
    pub exe_all: Vec<String>,
    /// Minimal satu substring cocok (case-insensitive) dengan path executable.
    #[serde(default)]
    pub exe_any: Vec<String>,
    /// Cocok jika SteamGameId atau SteamAppId termasuk dalam daftar ini.
    #[serde(default)]
    pub steam_app_ids: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct EnsureFileRule {
    /// Relatif terhadap direktori executable; file hanya dibuat bila marker ini ada.
    pub if_exists_rel: String,
    /// Relatif terhadap direktori executable; file kosong yang dipastikan ada.
    pub create_rel: String,
}
```

#### Aturan Matching, Komposisi Multi-Match, & Precedence
1. **Opt-out:** Jika `HYPERX_PROTON_NO_RULES=1`, tidak ada rule yang dijalankan (`use_steam_shim = false` kecuali `HYPERX_PROTON_FORCE_STEAM_SHIM=1`).
2. **Sumber & Resolusi Direktori Custom Rules (keputusan desain — single active dir fallback):**
   - Hanya **satu** direktori custom rules aktif yang dipilih berdasarkan rantai fallback standar:
     1. `$HYPERX_PROTON_RULES_DIR` (bila diset & tidak kosong — sekaligus berfungsi sebagai override penuh untuk isolasi `doctor` dan integration tests agar tidak tercemar `~/.config` host)
     2. `$XDG_CONFIG_HOME/hyperx-proton/rules` (bila `XDG_CONFIG_HOME` diset & tidak kosong)
     3. `$HOME/.config/hyperx-proton/rules`
   - Mulai dari daftar `builtin_rules()`, lalu muat semua file `*.json` di direktori aktif tersebut secara deterministik (urut alfabetis nama file). Setiap file JSON menerima baik satu objek `GameRule` maupun array `[GameRule, ...]`.
   - Jika custom rule memakai `id` yang sama dengan rule sebelumnya (mis. built-in rule), custom rule menggantikan rule ber-`id` sama tersebut di posisinya (sehingga user bisa mengganti penuh built-in rule jika memang diinginkan). File yang gagal dibaca/parse dicatat ke warning (dan tampil di `doctor`), tidak menggagalkan launch.
3. **Evaluasi Match:** Suatu rule cocok jika:
   - `steam_app_ids` cocok dengan `SteamGameId` atau `SteamAppId` dari host env, **ATAU**
   - `exe_all` tidak kosong atau `exe_any` tidak kosong, dan: setiap entri `exe_all` ada di path executable (case-insensitive) **serta** (bila `exe_any` tidak kosong) minimal satu entri `exe_any` ada di path executable (case-insensitive).
   - Rule dengan seluruh kriteria kosong tidak pernah cocok.
4. **Komposisi Multi-Match (keputusan desain — merge berlapis, bukan single-match):**
   - Jika beberapa rule dengan `id` berbeda sama-sama cocok pada satu launch (mis. built-in `wuthering-waves` + custom rule `my-wuwa-tweak`), **semua rule yang cocok digabung berurutan** (built-in lebih dulu, diikuti custom rules sesuai urutan pemuatan) menjadi satu `RuleEvaluation`:
     - `matched_rule_ids`: daftar semua `id` yang cocok.
     - `env`: digabung berurutan; rule yang lebih akhir menimpa key yang sama dari rule sebelumnya.
     - `dll_overrides`: dikumpulkan dengan prioritas rule yang lebih akhir menang atas rule sebelumnya (ketika diserahkan ke `merge_dll_overrides_with_extras`).
     - `ensure_files`: diakumulasikan berurutan (deduplikasi berdasarkan `create_rel`).
     - Field skalar `Option<T>` (`steam_game_id`, `use_steam_shim`, `dxvk_backend`): nilai `Some(v)` dari rule yang lebih akhir menimpa nilai sebelumnya; `None` mempertahankan nilai dari rule sebelumnya.
5. **Prioritas Nilai Akhir (Tertinggi → Terendah):**
   - **Env eksplisit user / `hyperx-launcher`** selalu menang atas `RuleEvaluation`, dan `RuleEvaluation` menang atas default `runtime.rs`.
   - **`WINEDLLOVERRIDES`:** User overrides → custom rule overrides → built-in rule overrides → `BASE_OVERRIDES` (dan `nvapi64=n` bila aktif), melalui `merge_dll_overrides_with_extras(...)`.
   - **Conditional `steam.exe` Shim:**
     - `HYPERX_PROTON_NO_STEAM_SHIM=1` → selalu `false`.
     - `HYPERX_PROTON_FORCE_STEAM_SHIM=1` → `true` (bila `steam.exe` ada di prefix).
     - Selain itu → mengikuti `rule_eval.use_steam_shim.unwrap_or(false)` (bila `steam.exe` ada di prefix).
   - **`ensure_files`:** Dieksekusi sebelum spawn game, fail-soft (error I/O diabaikan dengan aman), dan mencegah path traversal (`..` atau path absolut ditolak).

#### Built-in Rules Fase 2
1. **`wuthering-waves`**
   - Match: `exe_all: ["client-win64-shipping.exe"]`, `exe_any: ["wuwa", "wuthering"]`, `steam_app_ids: ["3513350"]`
   - `steam_game_id: Some("3513350")`
   - `dll_overrides: ["KRSDKExternal.exe=d"]`
   - `use_steam_shim: Some(false)`
   - `ensure_files`:
     - `ThirdParty/KrPcSdk_Global/KRSDKRes/KRSDKWebViewOSR/KRWebClient.exe` → `ThirdParty/KrPcSdk_Global/KRSDKRes/KRSDKWebViewOSR/usebitmap.flag`
     - `ThirdParty/KrPcSdk_CN/KRSDKRes/KRSDKWebViewOSR/KRWebClient.exe` → `ThirdParty/KrPcSdk_CN/KRSDKRes/KRSDKWebViewOSR/usebitmap.flag`
2. **`honkai-star-rail`**
   - Match: `exe_any: ["starrail.exe"]`
   - `use_steam_shim: Some(true)`
3. **`genshin-impact`**
   - Match: `exe_any: ["genshinimpact.exe", "yuanshen.exe"]`
   - `use_steam_shim: Some(true)`
4. **`zenless-zone-zero`**
   - Match: `exe_any: ["zenlesszonezero.exe"]`
   - `use_steam_shim: Some(true)`

---

### 4.5. Pembaruan `proton doctor` (`src/doctor.rs`)

Menambahkan 4 pemeriksaan baru ke output `proton doctor`:
- `gpu:vulkan`: `[OK]` dengan detail `<device_name> [<Vendor>, <Type>, Vulkan <maj.min.patch>, GPL=<yes|no>, present_wait=<yes|no>]`, atau `[WARN]` bila `Unavailable`.
- `gpu:dxvk-backend`: `[OK]` dengan detail `<standard|sarek> (<alasan>)`, atau `[WARN]` bila GPU hanya Vulkan `<1.3` tetapi `dxvk-sarek` tidak tersedia di bundle.
- `dxvk-sarek:d3d11.dll` dan `dxvk-sarek:dxgi.dll`: `[OK]` bila ada, `[WARN]` bila tidak ada (agar kompatibel dengan runner parsial/test fixture lama, sementara `packaging_smoke.sh` memverifikasi keduanya secara ketat).
- `rules:engine`: `[OK]` jumlah built-in rules + custom rules yang termuat, atau `[WARN]` bila ada file JSON custom yang gagal di-parse.

---

## 5. Batasan & Hal yang Diserahkan ke Builder

### Batasan Wajib
- Jangan menambah crate dependency baru di `Cargo.toml` (tetap `libc`, `serde`, `serde_json`, dan dev-dep `tempfile`).
- Jangan merusak test yang sudah ada (`tests/config_and_overrides.rs`, `tests/prefix_manager.rs`, `tests/runtime_and_cli.rs`, `tests/phase1_performance.rs`, `tests/packaging_smoke.sh`).
- Seluruh logika probing GPU, seleksi backend, vendor env, dan rule engine wajib memiliki unit & integration tests deterministik (tanpa bergantung pada GPU fisik tertentu).
- Tetap bangun dan verifikasi langsung pada host saat ini (termasuk build nyata `build-dxvk-sarek.sh`, `package-runner.sh`, `install-local.sh`, `cargo test`, `tests/packaging_smoke.sh`, dan `proton doctor`).

### Diserahkan ke Builder
- Nama persis struct internal FFI Vulkan (`VkInstance`, `VkPhysicalDeviceProperties`, layout struct C ABI x86_64) sepanjang diverifikasi terhadap `/usr/include/vulkan/vulkan_core.h` pada host.
- Organisasi file test baru (mis. `tests/phase2_multi_gpu_and_rules.rs`) dan detail pesan warning stderr.
- Penyesuaian flag meson DXVK-Sarek bila `mingw-w64-gcc` 16 di Arch Linux 2026 memerlukan flag kompatibilitas standar (mis. C++ standard/warning) — tetap terdokumentasi di `scripts/build-dxvk-sarek.sh`.
