#!/usr/bin/env bash
set -euo pipefail

source "$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/common.sh"

STAMP_KEY="dxvk:${DXVK_REF}:dx11-only"
OUT_DLL_DIR="$STAGE_DIR/dxvk/x86_64-windows"

if stamp_matches "dxvk" "$STAMP_KEY" && [[ -f "$OUT_DLL_DIR/d3d11.dll" ]] && [[ -f "$OUT_DLL_DIR/dxgi.dll" ]]; then
    echo "[dxvk] stamp matches ($STAMP_KEY), skipping build"
    exit 0
fi

DXVK_SRC="$SRC_DIR/dxvk"
DXVK_OBJ="$BUILD_DIR/obj-dxvk"
CROSS_FILE="$BUILD_DIR/mingw64-cross.txt"

ensure_git_checkout "$DXVK_REPO" "$DXVK_REF" "$DXVK_SRC"
write_mingw64_cross_file "$CROSS_FILE"

rm -rf "$DXVK_OBJ" "$STAGE_DIR/dxvk"
mkdir -p "$OUT_DLL_DIR"

meson setup "$DXVK_OBJ" "$DXVK_SRC" \
    --cross-file "$CROSS_FILE" \
    --buildtype=release \
    --prefix="$BUILD_DIR/install-dxvk" \
    -Denable_d3d8=false \
    -Denable_d3d9=false \
    -Denable_d3d10=false \
    -Denable_d3d11=true \
    -Denable_dxgi=true

ninja -C "$DXVK_OBJ" -j"$(nproc)"

for dll in d3d11.dll dxgi.dll; do
    found="$(find "$DXVK_OBJ" -type f -name "$dll" | head -n1)"
    [[ -n "$found" ]] || { echo "[dxvk] ERROR: $dll not produced"; exit 1; }
    cp -f "$found" "$OUT_DLL_DIR/$dll"
done

write_stamp "dxvk" "$STAMP_KEY"
echo "[dxvk] build complete (d3d11.dll + dxgi.dll only)"
