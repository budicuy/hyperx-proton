# HyperX-Proton

Native Arch Linux Proton runner for `hyperx-launcher` (`hyperxos`), running directly on the host without `steam-runtime` / `pressure-vessel` or `umu-launcher`.

## Scope
- **Graphics:** DX11 (`d3d11.dll`, `dxgi.dll` via DXVK), DX12 (`d3d12.dll`, `d3d12core.dll` via VKD3D-Proton), and opt-in NVAPI (`nvapi64.dll` via `PROTON_ENABLE_NVAPI=1`).
- **Sync:** Auto-detects `/dev/ntsync` (opt-out with `PROTON_NO_NTSYNC=1` → fallback `WINEFSYNC=1`).
- **Prefix Safety:** `flock`-guarded non-destructive DLL sync preserving `user.reg`, `system.reg`, and `drive_c/users/`.
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
