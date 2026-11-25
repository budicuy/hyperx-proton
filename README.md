# HyperX-Proton

Native Arch Linux Proton runner for `hyperx-launcher` (`hyperxos`), running directly on the host without `steam-runtime` / `pressure-vessel` or `umu-launcher`.

## Scope
- **Multi-Vendor GPU Support (AMD / Intel / NVIDIA):** Native `libvulkan.so.1` GPU probing for vendor-aware shader cache tuning (Mesa `RADV`/`ANV` and NVIDIA `GLCache`) and automatic `PROTON_ENABLE_NVAPI` vendor guard.
- **Graphics:** DX11 (`d3d11.dll`, `dxgi.dll` via DXVK `v3.1.1`, with automatic DXVK-Sarek `v1.14.1` fallback for Vulkan 1.1/1.2 GPUs), DX12 (`d3d12.dll`, `d3d12core.dll` via VKD3D-Proton), and opt-in NVAPI (`nvapi64.dll` via `PROTON_ENABLE_NVAPI=1`).
- **Native Game Rule Engine:** Built-in game rules (Wuthering Waves, Honkai: Star Rail, Genshin Impact, Zenless Zone Zero) with conditional `steam.exe` shim and extensible drop-in JSON rules (`$XDG_CONFIG_HOME/hyperx-proton/rules/*.json`).
- **Sync:** Auto-detects `/dev/ntsync` (opt-out with `PROTON_NO_NTSYNC=1` → fallback `WINEFSYNC=1`).
- **Prefix Safety:** `flock`-guarded non-destructive DLL sync preserving `user.reg`, `system.reg`, and `drive_c/users/` across DXVK backend switches.
- **Upstream Pins:** Single source of truth in `versions.env`.

## Usage

```bash
# Run unit & integration tests
cargo test

# Build all components locally (with ccache & stamp cache)
./scripts/build.sh
./scripts/package-runner.sh
./scripts/install-local.sh

# Run diagnostics
~/.local/share/hyperx-launcher/runners/HyperX-Proton/current/proton doctor
```
