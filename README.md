# HyperX-Proton

Pure native, universal `x86-64` Proton runner built in Rust for Linux (`glibc >= 2.38`), running directly on the host without `steam-runtime` / `pressure-vessel` or `umu-launcher`.

## Overview

- **Universal `x86-64` & Clean Host Linkage:** Built with `-march=x86-64 -mtune=generic` (no AVX2 or generation-specific CPU instructions required) and Wine configured `--without-ffmpeg --without-unwind` so media uses host GStreamer (`libgstreamer-1.0.so.0`) and unwinding uses standard `libgcc_s.so.1`.
- **Multi-Vendor GPU Auto-Detection:** Native `libvulkan.so.1` probing for AMD, Intel, and NVIDIA with vendor-aware shader cache tuning (Mesa `RADV`/`ANV` and NVIDIA `GLCache`), automatic `PROTON_ENABLE_NVAPI` vendor guarding (`dxvk-nvapi` `v0.9.3`), and automatic fallback from DXVK `v3.1.1` (Vulkan 1.3+) to DXVK-Sarek `v1.14.1` (Vulkan 1.1/1.2).
- **DirectX 12 & Graphics:** `d3d12.dll` / `d3d12core.dll` via VKD3D-Proton `v3.1`, DX11/DXGI via DXVK `v3.1.1` / DXVK-Sarek `v1.14.1`, and opt-in NVAPI (`PROTON_ENABLE_NVAPI=1`).
- **3-Tier Kernel Sync:** Automatic runtime detection in priority order: `ntsync` (`/dev/ntsync`) → `fsync` (`futex_waitv`) → `esync` (`WINEESYNC=1`), with plain `wineserver` sync as final fallback.
- **Native Game Rule Engine:** Built-in rules (Wuthering Waves, Honkai: Star Rail, Genshin Impact, Zenless Zone Zero) with conditional `steam.exe` shim and drop-in user JSON rules (`$XDG_CONFIG_HOME/hyperx-proton/rules/*.json`).
- **Steam Scout & Prefix Compatibility:** Automatically sanitizes injected `steam-runtime` / `ubuntu12_*` `LD_LIBRARY_PATH` entries (preserving `LD_PRELOAD` for overlays/MangoHud), supports `proton runinprefix` for redistributables and prefix helpers (`protontricks`, Winecfg/Winetricks), and guards prefix DLL sync with `flock` (using Btrfs `FICLONE` reflink when available).
- **Upstream Pins:** Single source of truth in [`versions.env`](versions.env) (`hyperx-proton-11.0-20261125`).

## System Requirements

| Component | Minimum | Recommended |
| :--- | :--- | :--- |
| **CPU** | Any 64-bit `x86-64` processor (Intel/AMD, no AVX2 required) | 4+ physical cores (for parallel shader compilation) |
| **OS & `glibc`** | 64-bit Linux with **`glibc >= 2.38`** (see supported distros below) | Modern rolling or recent LTS distro (`glibc >= 2.38`) |
| **Kernel & Sync** | Linux 5.x (`WINEESYNC`) | Linux `5.16+` (`WINEFSYNC` / `futex_waitv`) or **`6.14+` (`/dev/ntsync`)** |
| **GPU & Vulkan** | Vulkan 1.1 / 1.2 (auto-fallback to **DXVK-Sarek `v1.14.1`**): AMD GCN/Polaris, Intel HD/UHD Gen 7–10, NVIDIA Kepler/Maxwell/Pascal | Vulkan 1.3+ (**DXVK `v3.1.1` & VKD3D-Proton `v3.1`**): NVIDIA GTX 16xx/RTX 20xx+ (driver 550+), AMD RX 5000+ (Mesa RADV 23+), Intel Arc / Gen 11+ (Mesa ANV 23+) |
| **RAM** | 8 GB | 16 GB+ |
| **Storage** | 2 GB free for runner + game prefixes | SSD / NVMe (Btrfs enables zero-copy `FICLONE` prefix sync) |

### Supported Distributions (`glibc >= 2.38`)

- **Supported:** Ubuntu 24.04+, Linux Mint 22+, Debian 13 (Trixie)+, Fedora 39+, Nobara, Bazzite, Arch Linux, CachyOS, EndeavourOS, openSUSE Tumbleweed, SteamOS 3.6+.
- **Not Supported (Level 1 Host Builds):** Older LTS distributions with `glibc < 2.38` such as Ubuntu 22.04 (`glibc 2.35`) or Debian 12 (`glibc 2.36`). `proton doctor` checks `host:glibc` and reports `[FAIL]` if `< 2.38`.

## Host Dependencies per Distro

No `steam-runtime` or `vulkaninfo` is required. Install the host Vulkan loader, GStreamer media plugins, GnuTLS, Fontconfig, and FreeType packages for your distribution (plus your GPU's Vulkan ICD driver):

- **Arch Linux / CachyOS / EndeavourOS:**
  ```bash
  sudo pacman -S --needed vulkan-icd-loader gnutls gstreamer gst-plugins-base gst-plugins-good gst-libav fontconfig freetype2
  # Plus GPU Vulkan driver: vulkan-radeon (AMD), vulkan-intel (Intel), or nvidia-utils (NVIDIA)
  ```

- **Fedora / Nobara / Bazzite:**
  ```bash
  sudo dnf install vulkan-loader gnutls gstreamer1 gstreamer1-plugins-base gstreamer1-plugins-good gstreamer1-plugin-libav fontconfig freetype
  # Plus mesa-vulkan-drivers (AMD/Intel) or NVIDIA driver
  ```

- **Ubuntu 24.04+ / Debian 13+ / Linux Mint 22+:**
  ```bash
  sudo apt install libvulkan1 libgnutls30 libgstreamer1.0-0 gstreamer1.0-plugins-base gstreamer1.0-plugins-good gstreamer1.0-libav libfontconfig1 libfreetype6
  # Plus mesa-vulkan-drivers (AMD/Intel) or NVIDIA driver
  ```

- **openSUSE Tumbleweed:**
  ```bash
  sudo zypper install libvulkan1 libgnutls30 gstreamer gstreamer-plugins-base gstreamer-plugins-good gstreamer-plugins-libav fontconfig libfreetype6
  # Plus Mesa-vulkan-drivers (AMD/Intel) or NVIDIA driver
  ```

## Multi-Launcher Installation

After building and packaging (`./scripts/build.sh && ./scripts/package-runner.sh`), install to your preferred launcher:

- **HyperX-Launcher (default):**
  ```bash
  ./scripts/install-local.sh
  # Installs to ~/.local/share/hyperx-launcher/runners/HyperX-Proton/current
  ```

- **Steam:**
  ```bash
  ./scripts/install-local.sh --target steam
  # Or extract the release tarball into ~/.local/share/Steam/compatibilitytools.d/
  ```
  `package-runner.sh` generates `compatibilitytool.vdf` and `toolmanifest.vdf` (without `require_tool_appid`) so Steam can launch games directly with HyperX-Proton and run redistributables via `proton runinprefix`.

- **Heroic Games Launcher:**
  ```bash
  ./scripts/install-local.sh --target heroic
  # Or extract the release tarball into ~/.config/heroic/tools/proton/
  ```

- **Lutris:**
  Extract the packaged runner directory (`dist/hyperx-proton-11.0-20261125`) into `~/.local/share/lutris/runners/wine/`.

## Environment Variables & Custom JSON Rules

### Key Environment Overrides

| Variable | Effect |
| :--- | :--- |
| `PROTON_NO_NTSYNC=1` | Disable `/dev/ntsync` and fall back to `fsync` (or `esync`) |
| `PROTON_NO_FSYNC=1` | Disable `WINEFSYNC` (`futex_waitv`) and fall back to `esync` |
| `PROTON_NO_ESYNC=1` | Disable `WINEESYNC` (falls back to plain `wineserver` sync if `ntsync`/`fsync` are also disabled) |
| `PROTON_ENABLE_NVAPI=1` | Enable `dxvk-nvapi` (`nvapi64.dll`) on NVIDIA GPUs (`HYPERX_PROTON_FORCE_NVAPI=1` overrides vendor guard) |
| `PROTON_ENABLE_WAYLAND=1` | Enable native Wine Wayland driver (`DISPLAY` unset) |
| `HYPERX_PROTON_DXVK_BACKEND` | Override DXVK backend: `auto` (default), `standard` (`v3.1.1`), or `sarek` (`v1.14.1`) |
| `HYPERX_PROTON_NO_GPU_PROBE=1` | Skip native `libvulkan.so.1` GPU probing |
| `HYPERX_PROTON_NO_RULES=1` | Disable built-in and custom game rules |
| `HYPERX_PROTON_RULES_DIR` | Override custom rules directory (default: `$XDG_CONFIG_HOME/hyperx-proton/rules`) |
| `HYPERX_PROTON_NO_STEAM_SHIM=1` / `HYPERX_PROTON_FORCE_STEAM_SHIM=1` | Force-disable or force-enable the conditional `steam.exe` shim |
| `HYPERX_PROTON_KEEP_STEAM_RUNTIME_LD=1` | Opt out of `steam-runtime` `LD_LIBRARY_PATH` sanitization |
| `HYPERX_PROTON_SHADER_THREADS` | Override parallel shader compiler thread count |
| `HYPERX_PROTON_NO_TUNE=1` / `HYPERX_PROTON_TIMERSLACK_NS` | Disable runtime process tuning or override `PR_SET_TIMERSLACK` |
| `HYPERX_PROTON_NO_PREWARM=1` / `HYPERX_PROTON_NO_SUPERVISOR=1` | Disable background shader/prefix pre-warming or `pidfd` thread supervisor |

### Custom Game Rules (`~/.config/hyperx-proton/rules/my-game.json`)

Drop a single rule object or a JSON array of rule objects into `~/.config/hyperx-proton/rules/*.json`:

```json
{
  "id": "my-game",
  "name": "My Custom Game",
  "match_criteria": {
    "exe_all": ["mygame-win64-shipping.exe"],
    "exe_any": ["mygame"],
    "steam_app_ids": ["1234560"]
  },
  "steam_game_id": "1234560",
  "env": {
    "DXVK_HUD": "fps"
  },
  "dll_overrides": ["example_ext=n,b"],
  "use_steam_shim": false,
  "dxvk_backend": "auto",
  "ensure_files": [
    {
      "if_exists_rel": "Engine/Binaries/Win64/Helper.exe",
      "create_rel": "Engine/Binaries/Win64/usebitmap.flag"
    }
  ]
}
```

## Build & Diagnostics

```bash
# Run unit & integration tests
cargo test

# Build all components locally (with ccache & stamp cache)
./scripts/build.sh

# Package runner tarball + hyperx-native.json + Steam/Heroic VDF metadata
./scripts/package-runner.sh

# Install locally (hyperx | steam | heroic)
./scripts/install-local.sh

# Run cross-distro host & runner diagnostics
~/.local/share/hyperx-launcher/runners/HyperX-Proton/current/proton doctor
```
