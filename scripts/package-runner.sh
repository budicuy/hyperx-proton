#!/usr/bin/env bash
set -euo pipefail

SCRIPTS_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "$SCRIPTS_DIR/common.sh"

ARG_STAGE_DIR="$STAGE_DIR"
ARG_OUT_DIR="$DIST_DIR"
PROTON_BIN="$REPO_ROOT/target/release/hyperx-proton"

while [[ $# -gt 0 ]]; do
    case "$1" in
        --stage-dir)
            ARG_STAGE_DIR="${2:?missing value for --stage-dir}"
            shift 2
            ;;
        --out-dir)
            ARG_OUT_DIR="${2:?missing value for --out-dir}"
            shift 2
            ;;
        --proton-bin)
            PROTON_BIN="${2:?missing value for --proton-bin}"
            shift 2
            ;;
        *)
            echo "Unknown argument: $1" >&2
            exit 2
            ;;
    esac
done

[[ -x "$PROTON_BIN" ]] || { echo "Missing executable proton binary: $PROTON_BIN" >&2; exit 1; }

WINE_STAGE="$ARG_STAGE_DIR/wine/usr"
[[ -x "$WINE_STAGE/bin/wine" ]] || { echo "Missing staged wine binary in $WINE_STAGE/bin" >&2; exit 1; }
[[ -x "$WINE_STAGE/bin/wineserver" ]] || { echo "Missing staged wineserver binary in $WINE_STAGE/bin" >&2; exit 1; }

RUNNER_DIR="$ARG_OUT_DIR/$HYPERX_PROTON_VERSION"
rm -rf "$RUNNER_DIR"
mkdir -p \
    "$RUNNER_DIR/files/bin" \
    "$RUNNER_DIR/files/lib/wine" \
    "$RUNNER_DIR/files/lib/wine/icu/x86_64-windows" \
    "$RUNNER_DIR/files/lib/wine/icu/i386-windows" \
    "$RUNNER_DIR/files/lib/dxvk/x86_64-windows" \
    "$RUNNER_DIR/files/lib/vkd3d-proton/x86_64-windows" \
    "$RUNNER_DIR/files/lib/nvapi/x86_64-windows" \
    "$RUNNER_DIR/files/share/wine"

install -Dm755 "$PROTON_BIN" "$RUNNER_DIR/proton"
printf '%s\n' "$HYPERX_PROTON_VERSION" > "$RUNNER_DIR/version"

WINE_SHORT="${WINE_REF:0:8}"
cat > "$RUNNER_DIR/hyperx-native.json" <<EOF
{
  "schema_version": 1,
  "runner_mode": "native",
  "requires_steamrt": false,
  "name": "HyperX-Proton",
  "version": "$HYPERX_PROTON_VERSION",
  "arch": "x86_64",
  "components": {
    "wine": "wine-cachyos@${WINE_SHORT}",
    "dxvk": "${DXVK_REF} (d3d11,dxgi)",
    "vkd3d_proton": "${VKD3D_PROTON_REF} (d3d12,d3d12core)",
    "dxvk_nvapi": "${DXVK_NVAPI_REF}"
  }
}
EOF

cat > "$RUNNER_DIR/compatibilitytool.vdf" <<EOF
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
EOF

cat > "$RUNNER_DIR/toolmanifest.vdf" <<EOF
"manifest"
{
  "version" "2"
  "commandline" "/proton %verb%"
  "use_sessions" "1"
}
EOF

cp -a "$WINE_STAGE/bin/wine" "$RUNNER_DIR/files/bin/wine"
cp -a "$WINE_STAGE/bin/wineserver" "$RUNNER_DIR/files/bin/wineserver"
for sub in x86_64-unix x86_64-windows i386-windows; do
    if [[ -d "$WINE_STAGE/lib/wine/$sub" ]]; then
        cp -a "$WINE_STAGE/lib/wine/$sub" "$RUNNER_DIR/files/lib/wine/"
    fi
done

# wine-cachyos preloader_exec() rewrites x86_64-unix/wine to i386-unix/wine64{-preloader}
# when spawning AMD64 child processes (e.g. wineboot, explorer, game subprocesses).
# In a pure WoW64 build (no 32-bit Unix build), mirror wine64 + wine64-preloader into i386-unix/.
for bin in wine64 wine64-preloader; do
    if [[ ! -x "$RUNNER_DIR/files/lib/wine/x86_64-unix/$bin" ]]; then
        echo "Missing staged Wine loader binary: files/lib/wine/x86_64-unix/$bin" >&2
        exit 1
    fi
done
mkdir -p "$RUNNER_DIR/files/lib/wine/i386-unix"
ln -sfn ../x86_64-unix/wine64 "$RUNNER_DIR/files/lib/wine/i386-unix/wine64"
ln -sfn ../x86_64-unix/wine64-preloader "$RUNNER_DIR/files/lib/wine/i386-unix/wine64-preloader"
if [[ -d "$WINE_STAGE/share/wine" ]]; then
    cp -a "$WINE_STAGE/share/wine/." "$RUNNER_DIR/files/share/wine/"
fi

# Strictly copy only DX11 + DX12 (+ opt-in NVAPI) DLLs
for dll in d3d11.dll dxgi.dll; do
    src="$ARG_STAGE_DIR/dxvk/x86_64-windows/$dll"
    [[ -f "$src" ]] || { echo "Missing staged DXVK DLL: $src" >&2; exit 1; }
    cp -f "$src" "$RUNNER_DIR/files/lib/dxvk/x86_64-windows/$dll"
done

for dll in d3d12.dll d3d12core.dll; do
    src="$ARG_STAGE_DIR/vkd3d/x86_64-windows/$dll"
    [[ -f "$src" ]] || { echo "Missing staged VKD3D DLL: $src" >&2; exit 1; }
    cp -f "$src" "$RUNNER_DIR/files/lib/vkd3d-proton/x86_64-windows/$dll"
done

NVAPI_SRC="$ARG_STAGE_DIR/nvapi/x86_64-windows/nvapi64.dll"
[[ -f "$NVAPI_SRC" ]] || { echo "Missing staged NVAPI DLL: $NVAPI_SRC" >&2; exit 1; }
cp -f "$NVAPI_SRC" "$RUNNER_DIR/files/lib/nvapi/x86_64-windows/nvapi64.dll"

# ICU 68.2 DLLs required by wine-cachyos icu.dll forwarders (e.g. .NET/Sharphereal in WuWa)
for arch in x86_64-windows i386-windows; do
    for dll in icudt68.dll icuin68.dll icuuc68.dll; do
        src="$ARG_STAGE_DIR/icu/$arch/$dll"
        [[ -f "$src" ]] || { echo "Missing staged ICU DLL: $src" >&2; exit 1; }
        cp -f "$src" "$RUNNER_DIR/files/lib/wine/icu/$arch/$dll"
    done
done

# Minimal native steam.exe child-launcher shim (required by HoYoProtect AC on Linux)
SHIM_SRC="$ARG_STAGE_DIR/shim/x86_64-windows/steam.exe"
[[ -f "$SHIM_SRC" ]] || { echo "Missing staged steam.exe shim: $SHIM_SRC" >&2; exit 1; }
cp -f "$SHIM_SRC" "$RUNNER_DIR/files/lib/wine/x86_64-windows/steam.exe"

# Drop developer-only static import archives (*.a) that are never used at runtime
find "$RUNNER_DIR/files/lib/wine" -type f -name '*.a' -delete

# Strip DWARF debug sections from real ELF and PE binaries (opt-out: HYPERX_PROTON_NO_STRIP=1)
if [[ "${HYPERX_PROTON_NO_STRIP:-0}" != "1" ]]; then
    if command -v strip >/dev/null 2>&1; then
        strip --strip-unneeded "$RUNNER_DIR/proton" 2>/dev/null || true
        find "$RUNNER_DIR/files/bin" "$RUNNER_DIR/files/lib/wine/x86_64-unix" \
            -type f ! -name 'wine64-preloader' -print0 \
            | xargs -0 -r -n 16 -P "$(nproc)" bash -c '
                for f in "$@"; do
                    if [[ "$(od -An -tx1 -N4 "$f" 2>/dev/null | tr -d " \n")" == "7f454c46" ]]; then
                        strip --strip-debug "$f" 2>/dev/null || true
                    fi
                done
            ' _
    fi

    if command -v x86_64-w64-mingw32-strip >/dev/null 2>&1; then
        find \
            "$RUNNER_DIR/files/lib/wine/x86_64-windows" \
            "$RUNNER_DIR/files/lib/wine/i386-windows" \
            "$RUNNER_DIR/files/lib/dxvk" \
            "$RUNNER_DIR/files/lib/vkd3d-proton" \
            "$RUNNER_DIR/files/lib/nvapi" \
            -type f -print0 \
            | xargs -0 -r -n 32 -P "$(nproc)" bash -c '
                for f in "$@"; do
                    if [[ "$(od -An -tx1 -N2 "$f" 2>/dev/null | tr -d " \n")" == "4d5a" ]]; then
                        x86_64-w64-mingw32-strip --strip-debug "$f" 2>/dev/null || true
                    fi
                done
            ' _
    fi
fi

TARBALL="$ARG_OUT_DIR/${HYPERX_PROTON_VERSION}.tar.zst"
if command -v zstd >/dev/null 2>&1; then
    tar -C "$ARG_OUT_DIR" --zstd -cf "$TARBALL" "$HYPERX_PROTON_VERSION"
    echo "[package] created $TARBALL"
else
    echo "[package] zstd not found; skipped tarball creation"
fi

echo "[package] assembled $RUNNER_DIR"
