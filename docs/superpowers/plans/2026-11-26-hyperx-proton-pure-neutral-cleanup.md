# Rencana Implementasi Pembersihan Tuning Opsional HyperX-Proton

- **Spec:** [`2026-11-26-hyperx-proton-pure-neutral-cleanup-design.md`](../specs/2026-11-26-hyperx-proton-pure-neutral-cleanup-design.md)
- **Repositori:** `/home/budicuy/Dokumen/PROJEK/hyperx-proton`

### Task 1: Hapus modul & tes tuning opsional, pindahkan helper wajib
- Hapus file `src/supervisor.rs`, `src/shader_warm.rs`, `src/topology.rs`, `src/sys_tune.rs`, dan `tests/phase1_performance.rs`.
- Perbarui `src/lib.rs` untuk menghapus deklarasi `pub mod shader_warm`, `pub mod supervisor`, `pub mod sys_tune`, dan `pub mod topology`.
- Pindahkan `reflink_or_copy` (beserta `CopyMethod` & konstanta `FICLONE`) ke `src/prefix.rs`.
- Pindahkan `raise_nofile_limit` (konstanta `TARGET_NOFILE_SOFT = 524_288`) ke `src/main.rs`.

### Task 2: Cabut injeksi env performa otomatis & sederhanakan lifecycle proses
- Perbarui `src/runtime.rs`: hapus import `topology` dan seluruh blok auto-injeksi `DXVK_CONFIG` (`dxvk.numCompilerThreads`), `VKD3D_WORKER_THREADS`, `DXVK_STATE_CACHE`, `MESA_SHADER_CACHE_*`, `__GL_SHADER_DISK_CACHE*`, `DXVK_LOG_LEVEL`, dan `VKD3D_DEBUG`. Pastikan env user dan rule tetap diteruskan apa adanya (`User > Rule > Defaults`), termasuk NVAPI vendor guard, 3-tier sync, sanitasi `LD_LIBRARY_PATH`, dan `WINE_DISABLE_VULKAN_OPWR=1`.
- Perbarui `src/main.rs`: hapus `apply_process_tuning`, `spawn_shader_cache_prewarm`, dan `wait_with_supervisor`; panggil `raise_nofile_limit(TARGET_NOFILE_SOFT)` sebelum menjalankan Wine dan tunggu proses langsung via `child.wait()`.
- Perbarui `src/doctor.rs`: hapus import `supervisor` & `topology` serta cek `cpu:topology` dan `kernel:pidfd`.

### Task 3: Perbarui integration test & dokumentasi
- Perbarui `tests/phase2_multi_gpu_and_rules.rs`: hapus tes/import `shader_warm`, verifikasi bahwa `build_runtime_env_with_context` **tidak** menyuntikkan env shader cache / compiler threads / log level otomatis, bahwa `DXVK_CONFIG` dari rule diteruskan verbatim tanpa modifikasi, dan NVAPI vendor guard tetap aktif.
- Perbarui `README.md`: hapus baris env tuning yang dicabut serta rapikan narasi Overview agar mencerminkan runner Proton murni & netral.

### Task 4: Verifikasi penuh, repack, install lokal, dan commit
- Jalankan `cargo test`, `cargo build --release`, dan `bash tests/packaging_smoke.sh`.
- Jalankan `./scripts/package-runner.sh && ./scripts/install-local.sh`.
- Jalankan live `~/.local/share/hyperx-launcher/runners/HyperX-Proton/current/proton doctor` dan pastikan 0 `[FAIL]` / 0 `[WARN]` tanpa `cpu:topology` / `kernel:pidfd`.
- Commit bersih di branch `main` (tanpa push sebelum diminta pengguna).
