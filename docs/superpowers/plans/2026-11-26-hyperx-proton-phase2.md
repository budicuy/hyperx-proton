# HyperX-Proton Fase 2 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Menjadikan `HyperX-Proton` kompatibel lintas hardware (**AMD, Intel, NVIDIA**, Vulkan `1.1/1.2` hingga `1.4`) melalui probing Vulkan native di Rust, bundling & auto-fallback DXVK-Sarek `v1.14.1`, tuning runtime/cache shader per vendor, serta Rust native game rule engine dengan conditional `steam.exe` shim.

**Architecture:** Menambahkan modul murni Rust tanpa dependensi crate baru: `src/gpu.rs` (`dlopen("libvulkan.so.1")` + fungsi seleksi GPU/DXVK murni), `src/rules.rs` (built-in rules + drop-in custom JSON + komposisi multi-match), serta memperluas `src/dll_overrides.rs`, `src/runtime.rs`, `src/shader_warm.rs`, `src/prefix.rs`, `src/main.rs`, dan `src/doctor.rs` sambil menjaga wrapper backward-compatible. Di sisi build, menambahkan `scripts/build-dxvk-sarek.sh` dan mengemas `d3d11.dll` + `dxgi.dll` DXVK-Sarek `v1.14.1` ke `files/lib/dxvk-sarek/x86_64-windows/`.

**Tech Stack:** Rust (edition 2024, `libc`, `serde`, `serde_json`, `tempfile`), Bash build scripts, Meson + Ninja + `x86_64-w64-mingw32-gcc`, DXVK-Sarek `v1.14.1`, Vulkan C ABI (`libvulkan.so.1`).

**Spec:** `docs/superpowers/specs/2026-11-26-hyperx-proton-phase2-design.md`

## Global Constraints

- Nol crate baru di `Cargo.toml` (hanya `libc`, `serde`, `serde_json`, dan dev-dep `tempfile`).
- Nol ketergantungan binary `vulkaninfo`, Python, `steam-runtime`, atau `umu-launcher`.
- `hyperx-native.json` tetap `schema_version: 1`, `runner_mode: "native"`, `requires_steamrt: false`.
- Bundling grafis tetap ketat DX11 + DX12 (+ opt-in `nvapi64.dll`): **dilarang** mengemas `d3d8.dll`, `d3d9.dll`, `d3d10*.dll`, atau `ddraw.dll`.
- Seluruh fitur Fase 2 wajib **fail-soft** (tidak pernah menggagalkan launch karena `libvulkan.so.1` absen, host headless, atau file JSON custom korup).
- Prioritas konfigurasi wajib **User/Launcher Env > Custom Rule > Built-in Rule > Auto-Detected GPU Default**.
- Prefix existing (`user.reg`, `system.reg`, `drive_c/users/`) tidak boleh dirusak; ganti backend DXVK `standard ↔ sarek` tidak boleh memicu `wineboot -u`.
- Seluruh test lama (`config_and_overrides`, `prefix_manager`, `runtime_and_cli`, `phase1_performance`, `packaging_smoke.sh`) wajib tetap lulus.

## Review Focus

1. **Stale `nvapi64.dll` lintas ganti GPU:** Prefix yang pernah dipakai di GPU NVIDIA (`PROTON_ENABLE_NVAPI=1`) lalu dijalankan pada GPU AMD/Intel wajib menghapus `drive_c/windows/system32/nvapi64.dll` dan tidak menyertakan `nvapi64=n` di `WINEDLLOVERRIDES` (dites di Task 3 & Task 4).
2. **Custom JSON rule korup & path traversal pada `ensure_files`:** File JSON tidak valid di direktori rules wajib dilewati dengan warning tanpa menggagalkan launch, dan `ensure_files` dengan `..` atau path absolut wajib ditolak (dites di Task 2).
3. **Presedensi `User > Rule > Defaults` pada `DXVK_CONFIG` dan `WINE_DISABLE_VULKAN_OPWR`:** Rule yang menyetel `WINE_DISABLE_VULKAN_OPWR=0` dan `DXVK_CONFIG="dxgi.maxFrameRate = 60"` wajib mengalahkan default runtime dan tetap mendapat append `dxvk.numCompilerThreads = N`, namun tetap kalah dari env eksplisit user (dites di Task 3).
4. **Multi-GPU hybrid + `llvmpipe` + `MESA_VK_DEVICE_SELECT` dengan suffix `!`:** `select_primary_gpu` wajib mengabaikan `Cpu` (`llvmpipe`) saat GPU fisik ada, memprioritaskan `Discrete > Integrated`, dan mem-parse `MESA_VK_DEVICE_SELECT=1002:744c!` (case-insensitive, dengan/tanpa `!`) (dites di Task 1).
5. **Switch backend DXVK `standard ↔ sarek` tanpa `wineboot` ulang:** Menjalankan ulang pada prefix existing dengan backend berbeda wajib mengganti `d3d11.dll` + `dxgi.dll`, memperbarui `.hyperx-proton-dxvk-backend`, mempertahankan `user.reg`/`system.reg`/`drive_c/users/`, dan **tidak** memicu `wineboot -u` (dites di Task 4).

---

### Task 1: Native Vulkan GPU Prober & DXVK Backend Selection (`src/gpu.rs`)

**Files:**
- Create: `src/gpu.rs`
- Modify: `src/lib.rs`
- Create: `tests/phase2_multi_gpu_and_rules.rs`

**Interfaces:**
- Consumes: `libc` (`dlopen`, `dlsym`, `dlclose`, `RTLD_LAZY`, `RTLD_LOCAL`), `std::collections::BTreeMap`, `std::path::Path`.
- Produces:
  - `pub enum GpuVendor { Nvidia, Amd, Intel, Other(u32) }` (plus `from_vendor_id(u32) -> Self` dan `as_str(&self) -> &'static str`)
  - `pub enum GpuDeviceType { Discrete, Integrated, Virtual, Cpu, Other }` (plus `from_vk_type(u32) -> Self` dan `as_str(&self) -> &'static str`)
  - `pub struct GpuProfile { pub vendor: GpuVendor, pub vendor_id: u32, pub device_id: u32, pub device_name: String, pub device_type: GpuDeviceType, pub api_major: u32, pub api_minor: u32, pub api_patch: u32, pub has_gpl: bool, pub has_present_wait: bool, pub has_shader_module_id: bool }`
  - `pub enum GpuProbeResult { Detected(GpuProfile), Unavailable(String) }` (plus `vendor(&self) -> Option<GpuVendor>`)
  - `pub enum DxvkBackend { Standard, Sarek }` (plus `as_str(&self) -> &'static str`, `parse(s: &str) -> Option<Self>`, `rel_dll_dir(&self) -> &'static str`)
  - `pub struct DxvkSelection { pub backend: DxvkBackend, pub reason: String, pub warning: Option<String> }`
  - `pub fn is_sarek_bundled(runner_root: &Path) -> bool`
  - `pub fn select_primary_gpu(devices: &[GpuProfile], filter_name: Option<&str>, mesa_select: Option<&str>) -> Option<GpuProfile>`
  - `pub fn choose_dxvk_backend(probe: &GpuProbeResult, env_override: Option<&str>, rule_override: Option<DxvkBackend>, sarek_bundled: bool) -> DxvkSelection`
  - `pub fn probe_primary_gpu(host_env: &BTreeMap<String, String>) -> GpuProbeResult`

- [ ] **Step 1: Tulis failing tests untuk seleksi multi-GPU, fallback DXVK-Sarek, opt-out probe, dan layout ABI Vulkan di `tests/phase2_multi_gpu_and_rules.rs`**

```rust
use std::collections::BTreeMap;
use hyperx_proton::gpu::{
    DxvkBackend, GpuDeviceType, GpuProbeResult, GpuProfile, GpuVendor,
    choose_dxvk_backend, probe_primary_gpu, select_primary_gpu,
};

fn mock_gpu(
    vendor_id: u32,
    device_id: u32,
    name: &str,
    device_type: GpuDeviceType,
    major: u32,
    minor: u32,
) -> GpuProfile {
    GpuProfile {
        vendor: GpuVendor::from_vendor_id(vendor_id),
        vendor_id,
        device_id,
        device_name: name.to_string(),
        device_type,
        api_major: major,
        api_minor: minor,
        api_patch: 0,
        has_gpl: minor >= 3,
        has_present_wait: minor >= 3,
        has_shader_module_id: vendor_id == 0x10DE && minor >= 3,
    }
}

#[test]
fn select_primary_gpu_prefers_discrete_ignores_llvmpipe_and_honors_filters() {
    let devices = vec![
        mock_gpu(0x10005, 0x0000, "llvmpipe (LLVM 23)", GpuDeviceType::Cpu, 1, 4),
        mock_gpu(0x8086, 0x4680, "Intel UHD Graphics 730", GpuDeviceType::Integrated, 1, 3),
        mock_gpu(0x10DE, 0x21C4, "NVIDIA GeForce GTX 1660 SUPER", GpuDeviceType::Discrete, 1, 4),
        mock_gpu(0x1002, 0x744C, "AMD Radeon RX 7900 XTX", GpuDeviceType::Discrete, 1, 3),
    ];

    // Default: Discrete with highest Vulkan API wins (1.4 > 1.3), never Cpu
    let picked = select_primary_gpu(&devices, None, None).unwrap();
    assert_eq!(picked.vendor, GpuVendor::Nvidia);
    assert_eq!(picked.device_id, 0x21C4);

    // DXVK_FILTER_DEVICE_NAME selects Intel iGPU explicitly
    let intel = select_primary_gpu(&devices, Some("uhd graphics"), None).unwrap();
    assert_eq!(intel.vendor, GpuVendor::Intel);

    // MESA_VK_DEVICE_SELECT with trailing '!' selects AMD RX 7900 XTX
    let amd = select_primary_gpu(&devices, None, Some("1002:744c!")).unwrap();
    assert_eq!(amd.vendor, GpuVendor::Amd);
    assert_eq!(amd.device_id, 0x744C);

    // Only Cpu available still falls back to Cpu instead of None
    let cpu_only = select_primary_gpu(&devices[..1], None, None).unwrap();
    assert_eq!(cpu_only.device_type, GpuDeviceType::Cpu);
}

#[test]
fn choose_dxvk_backend_auto_selects_sarek_for_vulkan_1_2_and_handles_overrides() {
    let vk12_intel = GpuProbeResult::Detected(mock_gpu(
        0x8086, 0x1912, "Intel HD 530", GpuDeviceType::Integrated, 1, 2,
    ));
    let vk14_nvidia = GpuProbeResult::Detected(mock_gpu(
        0x10DE, 0x21C4, "GTX 1660 SUPER", GpuDeviceType::Discrete, 1, 4,
    ));
    let unavail = GpuProbeResult::Unavailable("no vulkan".to_string());

    // Vulkan 1.2 + bundled -> Sarek
    let sel = choose_dxvk_backend(&vk12_intel, None, None, true);
    assert_eq!(sel.backend, DxvkBackend::Sarek);
    assert!(sel.warning.is_none());

    // Vulkan 1.2 + NOT bundled -> fallback Standard with warning
    let sel_missing = choose_dxvk_backend(&vk12_intel, None, None, false);
    assert_eq!(sel_missing.backend, DxvkBackend::Standard);
    assert!(sel_missing.warning.is_some());

    // Vulkan 1.4 + auto -> Standard
    assert_eq!(choose_dxvk_backend(&vk14_nvidia, None, None, true).backend, DxvkBackend::Standard);

    // Rule override sarek wins over auto on Vulkan 1.4, env override standard beats rule
    assert_eq!(
        choose_dxvk_backend(&vk14_nvidia, None, Some(DxvkBackend::Sarek), true).backend,
        DxvkBackend::Sarek
    );
    assert_eq!(
        choose_dxvk_backend(&vk14_nvidia, Some("standard"), Some(DxvkBackend::Sarek), true).backend,
        DxvkBackend::Standard
    );

    // Unavailable GPU -> fail-soft Standard
    assert_eq!(choose_dxvk_backend(&unavail, None, None, true).backend, DxvkBackend::Standard);
}

#[test]
fn probe_primary_gpu_respects_no_probe_env_and_is_fail_soft() {
    let mut env = BTreeMap::new();
    env.insert("HYPERX_PROTON_NO_GPU_PROBE".to_string(), "1".to_string());
    assert!(matches!(probe_primary_gpu(&env), GpuProbeResult::Unavailable(_)));
}
```

- [ ] **Step 2: Jalankan test untuk memastikan gagal kompilasi (`gpu` belum ada)**

Run: `cargo test --test phase2_multi_gpu_and_rules`
Expected: FAIL karena `hyperx_proton::gpu` belum terdefinisi.

- [ ] **Step 3: Implementasikan `src/gpu.rs` dan ekspor `pub mod gpu;` di `src/lib.rs`**

Implementasikan sesuai antarmuka di atas dengan detail ABI x86_64 yang telah diverifikasi dari `/usr/include/vulkan/vulkan_core.h`:
- `#[repr(C)] struct VkApplicationInfo` (`size_of == 48`, `s_type = 0`).
- `#[repr(C)] struct VkInstanceCreateInfo` (`size_of == 64`, `s_type = 1`).
- `#[repr(C, align(8))] struct VkPhysicalDeviceProperties` (`size_of == 824`, `api_version @ 0`, `driver_version @ 4`, `vendor_id @ 8`, `device_id @ 12`, `device_type @ 16`, `device_name: [u8; 256] @ 20`, `_tail: [u8; 548]`).
- `#[repr(C)] struct VkExtensionProperties` (`size_of == 260`, `extension_name: [u8; 256] @ 0`, `spec_version: u32 @ 256`).
- Tambahkan unit test internal `#[cfg(test)]` di `src/gpu.rs` yang meng-assert keempat `std::mem::size_of` dan offset `device_name`/`vendor_id` agar regresi ABI mustahil lolos.
- Gunakan RAII wrapper (`VulkanLibGuard` untuk `dlclose` dan `VkInstanceGuard` untuk `vkDestroyInstance`), buat instance dengan `apiVersion = VK_MAKE_API_VERSION(0, 1, 1, 0)` lalu fallback `1.0.0` bila `VK_ERROR_INCOMPATIBLE_DRIVER` (`-9`), dan decode `apiVersion` (`(v >> 22) & 0x7f`, `(v >> 12) & 0x3ff`, `v & 0xfff`).
- Cek ekstensi: `"VK_EXT_graphics_pipeline_library"`, `"VK_KHR_present_wait"`, `"VK_EXT_shader_module_identifier"`.
- Parse `MESA_VK_DEVICE_SELECT`: ambil prefix sebelum `!` (atau `,`), split `:`, parse hex 16-bit/32-bit untuk `vendor_id:device_id`.

- [ ] **Step 4: Jalankan unit & integration tests untuk memastikan lulus**

Run: `cargo test`
Expected: PASS (seluruh test lama + test `gpu` baru lulus; tidak ada compiler warning).

- [ ] **Step 5: Commit**

```bash
git add src/lib.rs src/gpu.rs tests/phase2_multi_gpu_and_rules.rs
git commit -m "feat(gpu): add native dlopen Vulkan GPU prober and DXVK backend selector"
```

---

### Task 2: Rust Native Game Rule Engine (`src/rules.rs`) & Extended DLL Overrides (`src/dll_overrides.rs`)

**Files:**
- Create: `src/rules.rs`
- Modify: `src/dll_overrides.rs:22-59`
- Modify: `src/lib.rs`
- Modify: `tests/phase2_multi_gpu_and_rules.rs`

**Interfaces:**
- Consumes: `crate::gpu::DxvkBackend`, `serde::Deserialize`, `serde_json`, `std::collections::BTreeMap`.
- Produces:
  - Di `src/dll_overrides.rs`:
    - `pub fn merge_dll_overrides_with_extras(existing: Option<&str>, extra_overrides: &[String], enable_nvapi: bool) -> String` (urutan prioritas: `existing` > `extra_overrides` > `BASE_OVERRIDES` > `nvapi64=n`)
    - Mempertahankan `pub fn merge_dll_overrides(existing: Option<&str>, enable_nvapi: bool) -> String` sebagai wrapper memanggil `merge_dll_overrides_with_extras(existing, &[], enable_nvapi)`.
  - Di `src/rules.rs`:
    - `pub struct GameRule`, `pub struct RuleMatch`, `pub struct EnsureFileRule` (sesuai schema Section 4.4 spec)
    - `pub struct LoadedRules { pub rules: Vec<GameRule>, pub builtin_count: usize, pub custom_count: usize, pub warnings: Vec<String> }`
    - `pub struct RuleEvaluation { pub matched_rule_ids: Vec<String>, pub steam_game_id: Option<String>, pub env: BTreeMap<String, String>, pub dll_overrides: Vec<String>, pub use_steam_shim: Option<bool>, pub dxvk_backend: Option<DxvkBackend>, pub ensure_files: Vec<EnsureFileRule> }`
    - `pub fn builtin_rules() -> Vec<GameRule>` (memuat `wuthering-waves`, `honkai-star-rail`, `genshin-impact`, `zenless-zone-zero`)
    - `pub fn resolve_custom_rules_dir(host_env: &BTreeMap<String, String>) -> Option<PathBuf>` (single active dir fallback: `HYPERX_PROTON_RULES_DIR` → `XDG_CONFIG_HOME/hyperx-proton/rules` → `HOME/.config/hyperx-proton/rules`)
    - `pub fn load_rules(host_env: &BTreeMap<String, String>) -> LoadedRules`
    - `pub fn evaluate_rules(rules: &[GameRule], exe_path: &str, host_env: &BTreeMap<String, String>) -> RuleEvaluation`
    - `pub fn should_use_steam_shim(eval: &RuleEvaluation, host_env: &BTreeMap<String, String>, has_steam_shim_file: bool) -> bool`
    - `pub fn apply_ensure_files(exe_dir: &Path, rules: &[EnsureFileRule]) -> usize` (fail-soft, menolak `..` dan path absolut)

- [ ] **Step 1: Tulis failing tests di `tests/phase2_multi_gpu_and_rules.rs` untuk built-in rules, multi-match merge, same-ID replace, corrupt JSON, path traversal, dan conditional `steam.exe` shim**

```rust
use hyperx_proton::dll_overrides::merge_dll_overrides_with_extras;
use hyperx_proton::rules::{
    EnsureFileRule, apply_ensure_files, builtin_rules, evaluate_rules, load_rules,
    should_use_steam_shim,
};

#[test]
fn builtin_rules_match_wuwa_and_hoyoverse_with_conditional_steam_shim() {
    let rules = builtin_rules();
    let empty_env = BTreeMap::new();

    let wuwa = evaluate_rules(&rules, "/games/wuwa/Client/Binaries/Win64/Client-Win64-Shipping.exe", &empty_env);
    assert_eq!(wuwa.matched_rule_ids, vec!["wuthering-waves"]);
    assert_eq!(wuwa.steam_game_id.as_deref(), Some("3513350"));
    assert!(wuwa.dll_overrides.contains(&"KRSDKExternal.exe=d".to_string()));
    assert_eq!(wuwa.use_steam_shim, Some(false));
    assert!(!should_use_steam_shim(&wuwa, &empty_env, true));
    assert_eq!(wuwa.ensure_files.len(), 2);

    let hsr = evaluate_rules(&rules, "/games/HSR/StarRail.exe", &empty_env);
    assert_eq!(hsr.matched_rule_ids, vec!["honkai-star-rail"]);
    assert!(should_use_steam_shim(&hsr, &empty_env, true));

    let mut no_shim = BTreeMap::new();
    no_shim.insert("HYPERX_PROTON_NO_STEAM_SHIM".to_string(), "1".to_string());
    assert!(!should_use_steam_shim(&hsr, &no_shim, true));

    let mut force_shim = BTreeMap::new();
    force_shim.insert("HYPERX_PROTON_FORCE_STEAM_SHIM".to_string(), "1".to_string());
    assert!(should_use_steam_shim(&wuwa, &force_shim, true));

    // Unknown game does NOT trigger steam shim by default
    let other = evaluate_rules(&rules, "/games/Cyberpunk/bin/x64/Cyberpunk2077.exe", &empty_env);
    assert!(other.matched_rule_ids.is_empty());
    assert!(!should_use_steam_shim(&other, &empty_env, true));
}

#[test]
fn custom_rules_support_multi_match_merge_same_id_replace_and_corrupt_json_fail_soft() {
    let tmp = tempfile::tempdir().unwrap();
    let rules_dir = tmp.path().join("rules");
    std::fs::create_dir_all(&rules_dir).unwrap();

    // 1. Valid custom rule with NEW id matching WuWa -> merges with builtin without dropping usebitmap.flag
    std::fs::write(
        rules_dir.join("01-wuwa-tweak.json"),
        r#"{
            "id": "my-wuwa-tweak",
            "name": "My WuWa Tweak",
            "match_criteria": { "exe_any": ["client-win64-shipping.exe"] },
            "env": { "DXVK_CONFIG": "dxgi.maxFrameRate = 60" },
            "dll_overrides": ["KRSDKExternal.exe=n", "customfoo=d"],
            "dxvk_backend": "sarek"
        }"#,
    ).unwrap();

    // 2. Valid array file replacing builtin genshin-impact by same id
    std::fs::write(
        rules_dir.join("02-override-genshin.json"),
        r#"[{
            "id": "genshin-impact",
            "name": "Custom Genshin",
            "match_criteria": { "exe_any": ["genshinimpact.exe"] },
            "use_steam_shim": false
        }]"#,
    ).unwrap();

    // 3. Corrupt JSON must emit warning and NOT break loading
    std::fs::write(rules_dir.join("03-broken.json"), "{ invalid json").unwrap();

    let mut env = BTreeMap::new();
    env.insert("HYPERX_PROTON_RULES_DIR".to_string(), rules_dir.display().to_string());

    let loaded = load_rules(&env);
    assert_eq!(loaded.builtin_count, 4);
    assert_eq!(loaded.custom_count, 2);
    assert_eq!(loaded.warnings.len(), 1);

    let eval_wuwa = evaluate_rules(&loaded.rules, "/games/wuwa/Client-Win64-Shipping.exe", &env);
    assert_eq!(eval_wuwa.matched_rule_ids, vec!["wuthering-waves", "my-wuwa-tweak"]);
    assert_eq!(eval_wuwa.steam_game_id.as_deref(), Some("3513350"));
    assert_eq!(eval_wuwa.ensure_files.len(), 2);
    assert_eq!(eval_wuwa.env.get("DXVK_CONFIG").map(String::as_str), Some("dxgi.maxFrameRate = 60"));
    assert_eq!(eval_wuwa.dxvk_backend, Some(DxvkBackend::Sarek));

    // Custom rule dll_override wins over built-in, user wins over both
    let merged = merge_dll_overrides_with_extras(Some("customfoo=b"), &eval_wuwa.dll_overrides, false);
    assert!(merged.starts_with("customfoo=b;"));
    assert!(merged.contains("KRSDKExternal.exe=n"));
    assert!(!merged.contains("KRSDKExternal.exe=d"));

    // Replaced genshin-impact rule now has use_steam_shim = false
    let eval_gi = evaluate_rules(&loaded.rules, "/games/GenshinImpact.exe", &env);
    assert_eq!(eval_gi.use_steam_shim, Some(false));
}

#[test]
fn apply_ensure_files_creates_valid_flag_and_rejects_path_traversal() {
    let tmp = tempfile::tempdir().unwrap();
    let exe_dir = tmp.path().join("game");
    let sdk = exe_dir.join("ThirdParty/KrPcSdk_Global/KRSDKRes/KRSDKWebViewOSR");
    std::fs::create_dir_all(&sdk).unwrap();
    std::fs::write(sdk.join("KRWebClient.exe"), b"bin").unwrap();

    let actions = vec![
        EnsureFileRule {
            if_exists_rel: "ThirdParty/KrPcSdk_Global/KRSDKRes/KRSDKWebViewOSR/KRWebClient.exe".to_string(),
            create_rel: "ThirdParty/KrPcSdk_Global/KRSDKRes/KRSDKWebViewOSR/usebitmap.flag".to_string(),
        },
        EnsureFileRule {
            if_exists_rel: "ThirdParty/KrPcSdk_Global/KRSDKRes/KRSDKWebViewOSR/KRWebClient.exe".to_string(),
            create_rel: "../escaped.flag".to_string(),
        },
        EnsureFileRule {
            if_exists_rel: "ThirdParty/KrPcSdk_Global/KRSDKRes/KRSDKWebViewOSR/KRWebClient.exe".to_string(),
            create_rel: tmp.path().join("abs.flag").display().to_string(),
        },
    ];

    let created = apply_ensure_files(&exe_dir, &actions);
    assert_eq!(created, 1);
    assert!(sdk.join("usebitmap.flag").is_file());
    assert!(!tmp.path().join("escaped.flag").exists());
    assert!(!tmp.path().join("abs.flag").exists());
}
```

- [ ] **Step 2: Jalankan test untuk memastikan gagal**

Run: `cargo test --test phase2_multi_gpu_and_rules`
Expected: FAIL karena `src/rules.rs` dan `merge_dll_overrides_with_extras` belum ada.

- [ ] **Step 3: Implementasikan `merge_dll_overrides_with_extras` di `src/dll_overrides.rs` dan buat `src/rules.rs` + ekspor di `src/lib.rs`**

Catatan detail implementasi:
- Di `src/rules.rs`, saat mengomposisi `dll_overrides` dari beberapa rule yang cocok (built-in lebih dulu, custom lebih akhir), susun hasil `RuleEvaluation.dll_overrides` dengan **urutan prioritas tertinggi dulu** (custom sebelum built-in) sehingga `merge_dll_overrides_with_extras` (first-seen-wins) selalu memenangkan `user > custom rule > built-in rule > BASE_OVERRIDES`.
- Jika `HYPERX_PROTON_NO_RULES=1`, `evaluate_rules` langsung mengembalikan `RuleEvaluation::default()`.
- Untuk validasi path di `apply_ensure_files`, tolak path apa pun yang `is_absolute()` atau mengandung komponen `Component::ParentDir` atau `Component::RootDir`.

- [ ] **Step 4: Jalankan seluruh test untuk memastikan lulus**

Run: `cargo test`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/lib.rs src/dll_overrides.rs src/rules.rs tests/phase2_multi_gpu_and_rules.rs
git commit -m "feat(rules): add native Rust game rule engine and layered DLL override merge"
```

---

### Task 3: Vendor-Aware Runtime Env & Cross-Vendor Shader Pre-Warming (`src/runtime.rs`, `src/shader_warm.rs`)

**Files:**
- Modify: `src/runtime.rs:1-146`
- Modify: `src/shader_warm.rs:1-82`
- Modify: `tests/phase2_multi_gpu_and_rules.rs`

**Interfaces:**
- Consumes: `crate::gpu::{GpuProbeResult, GpuVendor}`, `crate::rules::RuleEvaluation`, `crate::dll_overrides::merge_dll_overrides_with_extras`.
- Produces:
  - Di `src/runtime.rs`:
    - `pub fn build_runtime_env_with_context(runner_root: &Path, layout: &PrefixLayout, host_env: &BTreeMap<String, String>, ntsync_available: bool, gpu_probe: &GpuProbeResult, rule_eval: &RuleEvaluation) -> RuntimeEnv`
    - Mempertahankan `pub fn build_runtime_env(runner_root: &Path, layout: &PrefixLayout, host_env: &BTreeMap<String, String>, ntsync_available: bool) -> RuntimeEnv` sebagai wrapper dengan `GpuProbeResult::Unavailable` + `RuleEvaluation::default()`.
  - Di `src/shader_warm.rs`:
    - `pub fn collect_shader_cache_dirs_for_vendor(layout: &PrefixLayout, env: &BTreeMap<String, String>, exe_dir: Option<&Path>, vendor: Option<GpuVendor>) -> Vec<PathBuf>`
    - Mempertahankan `pub fn collect_shader_cache_dirs(layout: &PrefixLayout, env: &BTreeMap<String, String>, exe_dir: Option<&Path>) -> Vec<PathBuf>` sebagai wrapper dengan `vendor = None`.

- [ ] **Step 1: Tulis failing tests di `tests/phase2_multi_gpu_and_rules.rs` untuk AMD/Intel/NVIDIA env tuning, NVAPI guard, `User > Rule > Defaults`, dan Mesa shader pre-warm**

```rust
use hyperx_proton::config::resolve_prefix_layout;
use hyperx_proton::rules::RuleEvaluation;
use hyperx_proton::runtime::build_runtime_env_with_context;
use hyperx_proton::shader_warm::{collect_shader_cache_dirs_for_vendor, prewarm_shader_cache_dirs};

#[test]
fn runtime_env_applies_vendor_specific_cache_and_blocks_nvapi_on_amd_intel() {
    let runner = std::path::Path::new("/opt/hyperx-proton");
    let layout = resolve_prefix_layout(Some(std::path::Path::new("/tmp/SHARED")), None).unwrap();

    let mut host_env = BTreeMap::new();
    host_env.insert("PROTON_ENABLE_NVAPI".to_string(), "1".to_string());

    let amd = GpuProbeResult::Detected(mock_gpu(0x1002, 0x744C, "AMD RX 7900", GpuDeviceType::Discrete, 1, 3));
    let rt_amd = build_runtime_env_with_context(runner, &layout, &host_env, true, &amd, &RuleEvaluation::default());
    assert!(!rt_amd.nvapi_enabled);
    assert!(!rt_amd.env.get("WINEDLLOVERRIDES").unwrap().contains("nvapi64=n"));
    assert!(!rt_amd.env.contains_key("__GL_SHADER_DISK_CACHE"));
    assert_eq!(rt_amd.env.get("MESA_SHADER_CACHE_DISABLE").map(String::as_str), Some("false"));
    assert_eq!(rt_amd.env.get("MESA_SHADER_CACHE_MAX_SIZE").map(String::as_str), Some("2G"));

    let intel = GpuProbeResult::Detected(mock_gpu(0x8086, 0x4680, "Intel UHD 730", GpuDeviceType::Integrated, 1, 3));
    let rt_intel = build_runtime_env_with_context(runner, &layout, &host_env, true, &intel, &RuleEvaluation::default());
    assert!(!rt_intel.nvapi_enabled);
    assert_eq!(rt_intel.env.get("MESA_SHADER_CACHE_MAX_SIZE").map(String::as_str), Some("2G"));

    // HYPERX_PROTON_FORCE_NVAPI=1 overrides vendor guard
    let mut force_env = host_env.clone();
    force_env.insert("HYPERX_PROTON_FORCE_NVAPI".to_string(), "1".to_string());
    let rt_forced = build_runtime_env_with_context(runner, &layout, &force_env, true, &amd, &RuleEvaluation::default());
    assert!(rt_forced.nvapi_enabled);
    assert!(rt_forced.env.get("WINEDLLOVERRIDES").unwrap().contains("nvapi64=n"));

    // NVIDIA allows NVAPI and sets NVIDIA 2 GiB cache, not Mesa
    let nv = GpuProbeResult::Detected(mock_gpu(0x10DE, 0x21C4, "GTX 1660 SUPER", GpuDeviceType::Discrete, 1, 4));
    let rt_nv = build_runtime_env_with_context(runner, &layout, &host_env, true, &nv, &RuleEvaluation::default());
    assert!(rt_nv.nvapi_enabled);
    assert!(rt_nv.env.get("WINEDLLOVERRIDES").unwrap().contains("nvapi64=n"));
    assert_eq!(rt_nv.env.get("__GL_SHADER_DISK_CACHE_SIZE").map(String::as_str), Some("2147483647"));
    assert!(!rt_nv.env.contains_key("MESA_SHADER_CACHE_MAX_SIZE"));
}

#[test]
fn runtime_env_respects_user_over_rule_over_defaults_for_dxvk_config_and_opwr() {
    let runner = std::path::Path::new("/opt/hyperx-proton");
    let layout = resolve_prefix_layout(Some(std::path::Path::new("/tmp/SHARED")), None).unwrap();
    let nv = GpuProbeResult::Detected(mock_gpu(0x10DE, 0x21C4, "GTX 1660 SUPER", GpuDeviceType::Discrete, 1, 4));

    let mut rule_eval = RuleEvaluation::default();
    rule_eval.steam_game_id = Some("3513350".to_string());
    rule_eval.env.insert("WINE_DISABLE_VULKAN_OPWR".to_string(), "0".to_string());
    rule_eval.env.insert("DXVK_CONFIG".to_string(), "dxgi.maxFrameRate = 60".to_string());

    // Without user override: rule wins over runtime defaults AND dxvk.numCompilerThreads appends cleanly
    let mut host_env = BTreeMap::new();
    host_env.insert("SteamGameId".to_string(), "0".to_string());
    let rt = build_runtime_env_with_context(runner, &layout, &host_env, true, &nv, &rule_eval);
    assert_eq!(rt.env.get("SteamGameId").map(String::as_str), Some("3513350"));
    assert_eq!(rt.env.get("WINE_DISABLE_VULKAN_OPWR").map(String::as_str), Some("0"));
    let dxvk_cfg = rt.env.get("DXVK_CONFIG").unwrap();
    assert!(dxvk_cfg.contains("dxgi.maxFrameRate = 60"), "got: {dxvk_cfg}");
    assert!(dxvk_cfg.contains("dxvk.numCompilerThreads = "), "got: {dxvk_cfg}");

    // With explicit user override: user beats rule
    host_env.insert("WINE_DISABLE_VULKAN_OPWR".to_string(), "1".to_string());
    host_env.insert("SteamGameId".to_string(), "999999".to_string());
    let rt_user = build_runtime_env_with_context(runner, &layout, &host_env, true, &nv, &rule_eval);
    assert_eq!(rt_user.env.get("WINE_DISABLE_VULKAN_OPWR").map(String::as_str), Some("1"));
    assert_eq!(rt_user.env.get("SteamGameId").map(String::as_str), Some("999999"));
}

#[test]
fn shader_warm_collects_and_advises_mesa_cache_for_amd_and_intel() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join("home");
    let mesa_sf = home.join(".cache/mesa_shader_cache_sf");
    let nv_gl = home.join(".cache/nvidia/GLCache");
    std::fs::create_dir_all(&mesa_sf).unwrap();
    std::fs::create_dir_all(&nv_gl).unwrap();
    std::fs::write(mesa_sf.join("radv.foz"), vec![1u8; 4096]).unwrap();
    std::fs::write(mesa_sf.join("index.idx"), vec![2u8; 512]).unwrap();
    std::fs::write(nv_gl.join("nv.bin"), vec![3u8; 4096]).unwrap();

    let layout = resolve_prefix_layout(Some(&tmp.path().join("SHARED")), None).unwrap();
    let mut env = BTreeMap::new();
    env.insert("HOME".to_string(), home.display().to_string());

    let amd_dirs = collect_shader_cache_dirs_for_vendor(&layout, &env, None, Some(GpuVendor::Amd));
    assert!(amd_dirs.contains(&mesa_sf));
    assert!(!amd_dirs.contains(&nv_gl));
    let stats = prewarm_shader_cache_dirs(&amd_dirs);
    assert_eq!(stats.files_advised, 2);
    assert_eq!(stats.bytes_advised, 4608);
}
```

- [ ] **Step 2: Jalankan test untuk memastikan gagal**

Run: `cargo test --test phase2_multi_gpu_and_rules`
Expected: FAIL karena `build_runtime_env_with_context` dan `collect_shader_cache_dirs_for_vendor` belum ada.

- [ ] **Step 3: Implementasikan perubahan di `src/runtime.rs` dan `src/shader_warm.rs`**

- Di `src/runtime.rs`: ikuti persis urutan pipeline Section 4.3 spec (`host_env` → merge `rule_eval.env` & `rule_eval.steam_game_id` bila kosong/`"0"` → evaluasi control flags & `nvapi_enabled` dengan vendor guard → `merge_dll_overrides_with_extras` → `DXVK_CONFIG` append `dxvk.numCompilerThreads` → vendor-specific shader cache defaults).
- Di `src/shader_warm.rs`: tambahkan `.foz` dan `.idx` ke `is_shader_cache_file`, lalu pilah direktori NVIDIA (`__GL_SHADER_DISK_CACHE_PATH`, `$HOME/.cache/nvidia/GLCache`) untuk `Some(GpuVendor::Nvidia) | None` dan direktori Mesa (`MESA_SHADER_CACHE_DIR`, `$XDG_CACHE_HOME/mesa_shader_cache{,_sf}`, `$HOME/.cache/mesa_shader_cache{,_sf}`) untuk `Some(GpuVendor::Amd | GpuVendor::Intel)`.

- [ ] **Step 4: Jalankan seluruh test**

Run: `cargo test`
Expected: PASS (semua test lama + Fase 2 lulus).

- [ ] **Step 5: Commit**

```bash
git add src/runtime.rs src/shader_warm.rs tests/phase2_multi_gpu_and_rules.rs
git commit -m "feat(runtime): add vendor-aware AMD/Intel/NVIDIA tuning, NVAPI guard, and Mesa shader prewarm"
```

---

### Task 4: Backend-Aware Prefix Sync, CLI Wiring, & Expanded `proton doctor` (`src/prefix.rs`, `src/main.rs`, `src/doctor.rs`)

**Files:**
- Modify: `src/prefix.rs:66-304`
- Modify: `src/main.rs:1-196`
- Modify: `src/doctor.rs:1-200`
- Modify: `tests/phase2_multi_gpu_and_rules.rs`

**Interfaces:**
- Consumes: `crate::gpu::{DxvkBackend, choose_dxvk_backend, is_sarek_bundled, probe_primary_gpu}`, `crate::rules::{apply_ensure_files, evaluate_rules, load_rules, should_use_steam_shim}`, `crate::runtime::build_runtime_env_with_context`, `crate::shader_warm::collect_shader_cache_dirs_for_vendor`.
- Produces:
  - Di `src/prefix.rs`:
    - `pub fn sync_prefix_dlls_with_options(runner_root: &Path, layout: &PrefixLayout, runner_version: &str, enable_nvapi: bool, dxvk_backend: DxvkBackend) -> Result<PrefixSyncReport, String>`
    - Wrapper `pub fn sync_prefix_dlls(...)` tetap memanggil `sync_prefix_dlls_with_options(..., DxvkBackend::Standard)`.
  - Di `src/main.rs`: menghapus blok hardcoded WuWa, mengganti seluruh launch pipeline menjadi: `probe_primary_gpu` → `load_rules` + `evaluate_rules` → `choose_dxvk_backend` (cetak `warning` ke stderr bila ada) → `build_runtime_env_with_context` → `sync_prefix_dlls_with_options` → `apply_ensure_files` → `collect_shader_cache_dirs_for_vendor` → conditional `steam.exe` shim via `should_use_steam_shim`.
  - Di `src/doctor.rs`: menambahkan check `gpu:vulkan`, `gpu:dxvk-backend`, `dxvk-sarek:d3d11.dll`, `dxvk-sarek:dxgi.dll`, `rules:engine`.

- [ ] **Step 1: Tulis failing tests di `tests/phase2_multi_gpu_and_rules.rs` untuk backend switch tanpa `wineboot`, pembersihan stale `nvapi64.dll`, CLI WuWa vs HSR shim, dan `proton doctor`**

```rust
use hyperx_proton::prefix::{prefix_needs_wineboot, sync_prefix_dlls_with_options};
use std::os::unix::fs::PermissionsExt;
use std::process::Command;

#[test]
fn prefix_switches_dxvk_backend_without_wineboot_and_removes_stale_nvapi() {
    let tmp = tempfile::tempdir().unwrap();
    let runner = tmp.path().join("runner");
    let compat = tmp.path().join("SHARED");
    let pfx = compat.join("pfx");
    std::fs::create_dir_all(pfx.join("drive_c/windows/system32")).unwrap();
    std::fs::write(pfx.join("system.reg"), "KEEP_SYS").unwrap();
    std::fs::write(pfx.join("user.reg"), "KEEP_USER").unwrap();

    for (rel, content) in [
        ("files/lib/dxvk/x86_64-windows/d3d11.dll", b"STD_D3D11".as_slice()),
        ("files/lib/dxvk/x86_64-windows/dxgi.dll", b"STD_DXGI".as_slice()),
        ("files/lib/dxvk-sarek/x86_64-windows/d3d11.dll", b"SAREK_D3D11".as_slice()),
        ("files/lib/dxvk-sarek/x86_64-windows/dxgi.dll", b"SAREK_DXGI".as_slice()),
        ("files/lib/vkd3d-proton/x86_64-windows/d3d12.dll", b"VKD3D12".as_slice()),
        ("files/lib/vkd3d-proton/x86_64-windows/d3d12core.dll", b"VKD3DCORE".as_slice()),
        ("files/lib/nvapi/x86_64-windows/nvapi64.dll", b"NVAPI64".as_slice()),
    ] {
        let p = runner.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, content).unwrap();
    }

    let layout = resolve_prefix_layout(Some(&compat), None).unwrap();
    let r1 = sync_prefix_dlls_with_options(&runner, &layout, "v1", true, DxvkBackend::Standard).unwrap();
    assert!(r1.dlls_updated);
    assert_eq!(std::fs::read(pfx.join("drive_c/windows/system32/d3d11.dll")).unwrap(), b"STD_D3D11");
    assert!(pfx.join("drive_c/windows/system32/nvapi64.dll").is_file());

    // Switch to Sarek & disable NVAPI (e.g. AMD/Intel): NO wineboot needed, DLLs swapped, stale nvapi64.dll deleted!
    assert!(!prefix_needs_wineboot(&runner, &layout, "v1"));
    let r2 = sync_prefix_dlls_with_options(&runner, &layout, "v1", false, DxvkBackend::Sarek).unwrap();
    assert!(r2.dlls_updated);
    assert!(!r2.needs_wineboot);
    assert_eq!(std::fs::read(pfx.join("drive_c/windows/system32/d3d11.dll")).unwrap(), b"SAREK_D3D11");
    assert!(!pfx.join("drive_c/windows/system32/nvapi64.dll").exists());
    assert_eq!(std::fs::read_to_string(pfx.join(".hyperx-proton-dxvk-backend")).unwrap().trim(), "sarek");
    assert_eq!(std::fs::read_to_string(pfx.join("user.reg")).unwrap(), "KEEP_USER");

    // Repeating with same backend is idempotent
    let r3 = sync_prefix_dlls_with_options(&runner, &layout, "v1", false, DxvkBackend::Sarek).unwrap();
    assert!(!r3.dlls_updated);
}

#[test]
fn cli_launches_wuwa_without_steam_shim_and_hsr_with_steam_shim_and_doctor_reports_phase2() {
    let tmp = tempfile::tempdir().unwrap();
    let runner = tmp.path().join("runner");
    let compat = tmp.path().join("SHARED");
    let log_file = tmp.path().join("calls.log");
    let rules_dir = tmp.path().join("empty-rules");
    std::fs::create_dir_all(&rules_dir).unwrap();

    // Minimal stub runner with dxvk, dxvk-sarek, vkd3d, steam.exe shim
    std::fs::create_dir_all(runner.join("files/bin")).unwrap();
    std::fs::write(
        runner.join("hyperx-native.json"),
        r#"{"schema_version":1,"runner_mode":"native","requires_steamrt":false,"name":"HyperX-Proton","version":"v1","arch":"x86_64"}"#,
    ).unwrap();
    for (rel, content) in [
        ("files/lib/dxvk/x86_64-windows/d3d11.dll", b"STD11".as_slice()),
        ("files/lib/dxvk/x86_64-windows/dxgi.dll", b"STDX".as_slice()),
        ("files/lib/dxvk-sarek/x86_64-windows/d3d11.dll", b"SAR11".as_slice()),
        ("files/lib/dxvk-sarek/x86_64-windows/dxgi.dll", b"SARX".as_slice()),
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

    // WuWa fake directory with KRWebClient.exe
    let wuwa_dir = tmp.path().join("wuwa/Client/Binaries/Win64");
    let webview = wuwa_dir.join("ThirdParty/KrPcSdk_Global/KRSDKRes/KRSDKWebViewOSR");
    std::fs::create_dir_all(&webview).unwrap();
    std::fs::write(webview.join("KRWebClient.exe"), b"cef").unwrap();
    let wuwa_exe = wuwa_dir.join("Client-Win64-Shipping.exe");
    std::fs::write(&wuwa_exe, b"pe").unwrap();

    let bin = env!("CARGO_BIN_EXE_hyperx-proton");
    let st1 = Command::new(bin)
        .args(["run", wuwa_exe.to_str().unwrap()])
        .env("HYPERX_PROTON_ROOT", &runner)
        .env("STEAM_COMPAT_DATA_PATH", &compat)
        .env("HYPERX_PROTON_RULES_DIR", &rules_dir)
        .env("HYPERX_PROTON_NO_GPU_PROBE", "1")
        .env("TEST_CALL_LOG", &log_file)
        .status()
        .unwrap();
    assert!(st1.success());
    assert!(webview.join("usebitmap.flag").is_file());

    // Launch HSR -> MUST prepend C:\windows\system32\steam.exe
    let st2 = Command::new(bin)
        .args(["run", "/tmp/HSR/StarRail.exe"])
        .env("HYPERX_PROTON_ROOT", &runner)
        .env("STEAM_COMPAT_DATA_PATH", &compat)
        .env("HYPERX_PROTON_RULES_DIR", &rules_dir)
        .env("HYPERX_PROTON_NO_GPU_PROBE", "1")
        .env("TEST_CALL_LOG", &log_file)
        .status()
        .unwrap();
    assert!(st2.success());

    let log = std::fs::read_to_string(&log_file).unwrap();
    assert!(log.contains(&format!("wine:{}", wuwa_exe.display())), "log:\n{log}");
    assert!(!log.contains(&format!("steam.exe {}", wuwa_exe.display())), "WuWa must not use steam shim:\n{log}");
    assert!(log.contains("wine:C:\\windows\\system32\\steam.exe /tmp/HSR/StarRail.exe"), "HSR must use steam shim:\n{log}");

    // Doctor reports Phase 2 checks
    let doc = Command::new(bin)
        .arg("doctor")
        .env("HYPERX_PROTON_ROOT", &runner)
        .env("HYPERX_PROTON_RULES_DIR", &rules_dir)
        .output()
        .unwrap();
    let out = String::from_utf8_lossy(&doc.stdout);
    assert!(out.contains("gpu:vulkan"), "doctor:\n{out}");
    assert!(out.contains("gpu:dxvk-backend"), "doctor:\n{out}");
    assert!(out.contains("dxvk-sarek:d3d11.dll"), "doctor:\n{out}");
    assert!(out.contains("rules:engine"), "doctor:\n{out}");
}
```

- [ ] **Step 2: Jalankan test untuk memastikan gagal**

Run: `cargo test --test phase2_multi_gpu_and_rules`
Expected: FAIL.

- [ ] **Step 3: Implementasikan pembaruan di `src/prefix.rs`, `src/main.rs`, dan `src/doctor.rs`**

- Di `src/prefix.rs`:
  - `target_prefix_files(runner_root, enable_nvapi, dxvk_backend)` mengambil `d3d11.dll` + `dxgi.dll` dari `dxvk_backend.rel_dll_dir()`.
  - Cek stamp `.hyperx-proton-dxvk-backend`; bila berbeda dari `dxvk_backend.as_str()`, lakukan re-copy DLL dan tulis stamp baru.
  - Bila `!enable_nvapi` dan `drive_c/windows/system32/nvapi64.dll` ada, hapus file tersebut (`remove_existing_if_present`).
- Di `src/main.rs`:
  - Panggil `probe_primary_gpu(&host_env)`, `load_rules(&host_env)`, `evaluate_rules(&loaded.rules, exe, &host_env)`, `choose_dxvk_backend(&gpu_probe, host_env.get("HYPERX_PROTON_DXVK_BACKEND").map(String::as_str), rule_eval.dxvk_backend, is_sarek_bundled(runner_root))`.
  - Cetak warning dari `loaded.warnings` dan `dxvk_sel.warning` ke `eprintln!` (fail-soft).
  - Gunakan `build_runtime_env_with_context`, `sync_prefix_dlls_with_options`, `apply_ensure_files`, `collect_shader_cache_dirs_for_vendor`, dan `should_use_steam_shim`.
- Di `src/doctor.rs`:
  - Tambahkan check `gpu:vulkan`, `gpu:dxvk-backend`, `dxvk-sarek:d3d11.dll`, `dxvk-sarek:dxgi.dll`, dan `rules:engine`.

- [ ] **Step 4: Jalankan seluruh test suite Rust**

Run: `cargo test`
Expected: PASS (seluruh unit & integration tests lulus, 0 warnings).

- [ ] **Step 5: Commit**

```bash
git add src/prefix.rs src/main.rs src/doctor.rs tests/phase2_multi_gpu_and_rules.rs
git commit -m "feat(proton): wire multi-GPU detection, Sarek backend prefix sync, rules engine, and doctor checks"
```

---

### Task 5: Build & Bundle DXVK-Sarek `v1.14.1` (`versions.env`, `scripts/`, `tests/packaging_smoke.sh`)

**Files:**
- Modify: `versions.env`
- Create: `scripts/build-dxvk-sarek.sh`
- Modify: `scripts/build.sh`
- Modify: `scripts/package-runner.sh`
- Modify: `tests/packaging_smoke.sh`
- Modify: `README.md`

**Interfaces:**
- Consumes: `scripts/common.sh` (`ensure_git_checkout`, `write_mingw64_cross_file`, `stamp_matches`, `write_stamp`), `versions.env` (`DXVK_SAREK_REPO`, `DXVK_SAREK_REF`).
- Produces:
  - `$STAGE_DIR/dxvk-sarek/x86_64-windows/{d3d11.dll,dxgi.dll}`
  - `$RUNNER_DIR/files/lib/dxvk-sarek/x86_64-windows/{d3d11.dll,dxgi.dll}`
  - `"dxvk_sarek": "${DXVK_SAREK_REF} (d3d11,dxgi)"` di `hyperx-native.json`

- [ ] **Step 1: Perbarui `tests/packaging_smoke.sh` lebih dulu (TDD) agar memverifikasi `dxvk-sarek` + menolak `ddraw.dll`/`d3d9.dll`**

Tambahkan di `tests/packaging_smoke.sh`:
- Buat dummy `$STAGE_DIR/dxvk-sarek/x86_64-windows/{d3d11.dll,dxgi.dll,ddraw.dll,d3d9.dll}`.
- Assert `hyperx-native.json` mengandung `data["components"]["dxvk_sarek"]`.
- Assert `files/lib/dxvk-sarek/x86_64-windows/{d3d11.dll,dxgi.dll}` ada di `$RUNNER_DIR`.
- Assert tidak ada `d3d9.dll` maupun `ddraw.dll` di seluruh `$RUNNER_DIR`.

- [ ] **Step 2: Jalankan `bash tests/packaging_smoke.sh` untuk memastikan gagal**

Run: `bash tests/packaging_smoke.sh`
Expected: FAIL karena `package-runner.sh` belum menyalin `dxvk-sarek`.

- [ ] **Step 3: Tambahkan pin DXVK-Sarek di `versions.env`, buat `scripts/build-dxvk-sarek.sh`, dan perbarui `scripts/build.sh`, `scripts/package-runner.sh`, dan `README.md`**

- `versions.env`:
  ```bash
  DXVK_SAREK_REPO=https://github.com/pythonlover02/DXVK-Sarek.git
  DXVK_SAREK_REF=v1.14.1
  ```
- `scripts/build-dxvk-sarek.sh`: mengikuti `scripts/build-dxvk.sh` (`chmod +x`) dengan `-Denable_ddraw=false -Denable_d3d8=false -Denable_d3d9=false -Denable_d3d10=false -Denable_d3d11=true -Denable_dxgi=true`.
- `scripts/build.sh`: tambahkan `dxvk-sarek` ke `--help`, target `all`, dan case `dxvk-sarek`.
- `scripts/package-runner.sh`: buat direktori `files/lib/dxvk-sarek/x86_64-windows`, salin hanya `d3d11.dll` + `dxgi.dll`, dan cantumkan `"dxvk_sarek"` di `hyperx-native.json`.
- `README.md`: perbarui ringkasan singkat Scope (multi-vendor AMD/Intel/NVIDIA, DXVK-Sarek fallback, Rust rule engine).

- [ ] **Step 4: Jalankan `cargo build --release && bash tests/packaging_smoke.sh`**

Run: `cargo build --release && bash tests/packaging_smoke.sh`
Expected: PASS (`packaging_smoke.sh: ALL CHECKS PASSED`).

- [ ] **Step 5: Commit**

```bash
git add versions.env scripts/build-dxvk-sarek.sh scripts/build.sh scripts/package-runner.sh tests/packaging_smoke.sh README.md
git commit -m "build(dxvk-sarek): bundle DXVK-Sarek v1.14.1 DX11/DXGI fallback in runner package"
```

---

### Task 6: Real Host Build DXVK-Sarek, Local Install, & End-to-End Verification

**Files:**
- Modify (only if `mingw-w64-gcc` 16 needs a documented build flag): `scripts/build-dxvk-sarek.sh`

**Interfaces:**
- Consumes: seluruh artefak Task 1–5.
- Produces: runner terpasang mutakhir di `~/.local/share/hyperx-launcher/runners/HyperX-Proton/current` dengan DXVK-Sarek nyata tercompile + seluruh verifikasi lulus.

- [ ] **Step 1: Compile DXVK-Sarek `v1.14.1` nyata + rebuild release binary Rust**

Run:
```bash
./scripts/build.sh --component rust
./scripts/build.sh --component dxvk-sarek
```
Expected: `build/stage/dxvk-sarek/x86_64-windows/{d3d11.dll,dxgi.dll}` terbentuk (valid PE32+ DLLs). Bila `mingw-w64-gcc` memerlukan flag kompatibilitas standar, perbaiki di `scripts/build-dxvk-sarek.sh`, re-run, dan commit fix-nya.

- [ ] **Step 2: Kemas ulang runner dan install ke lokal**

Run:
```bash
./scripts/package-runner.sh
./scripts/install-local.sh
```
Expected: `~/.local/share/hyperx-launcher/runners/HyperX-Proton/current` terpasang utuh dengan `dxvk-sarek` + binary `proton` Fase 2.

- [ ] **Step 3: Verifikasi menyeluruh (`cargo test`, `packaging_smoke.sh`, `proton doctor`, inspeksi PE DLL, dan cek prefix existing aman)**

Run:
```bash
cargo test
bash tests/packaging_smoke.sh
RUNNER="$HOME/.local/share/hyperx-launcher/runners/HyperX-Proton/current"
"$RUNNER/proton" version
"$RUNNER/proton" doctor
file "$RUNNER/files/lib/dxvk-sarek/x86_64-windows/d3d11.dll" "$RUNNER/files/lib/dxvk-sarek/x86_64-windows/dxgi.dll"
find "$RUNNER/files/lib/dxvk" "$RUNNER/files/lib/dxvk-sarek" "$RUNNER/files/lib/vkd3d-proton" "$RUNNER/files/lib/nvapi" -type f | sort
```
Expected:
- `cargo test`: 100% PASS, 0 failures.
- `packaging_smoke.sh`: `ALL CHECKS PASSED`.
- `proton doctor`: **0 `[FAIL]`**, menampilkan `gpu:vulkan` (`NVIDIA GeForce GTX 1660 SUPER`, Vulkan 1.4.x, `GPL=yes`), `gpu:dxvk-backend` (`standard`), `dxvk-sarek:d3d11.dll` `[OK]`, `dxvk-sarek:dxgi.dll` `[OK]`, `rules:engine` `[OK]` (4 built-in rules).
- Hanya DLL DX11/DX12/NVAPI yang terkemas (nol `d3d9.dll`/`ddraw.dll`).
