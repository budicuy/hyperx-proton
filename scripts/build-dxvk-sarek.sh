#!/usr/bin/env bash
set -euo pipefail

source "$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/common.sh"

STAMP_KEY="dxvk-sarek:${DXVK_SAREK_REF}:dx11-only"
OUT_DLL_DIR="$STAGE_DIR/dxvk-sarek/x86_64-windows"

if stamp_matches "dxvk-sarek" "$STAMP_KEY" && [[ -f "$OUT_DLL_DIR/d3d11.dll" ]] && [[ -f "$OUT_DLL_DIR/dxgi.dll" ]]; then
    echo "[dxvk-sarek] stamp matches ($STAMP_KEY), skipping build"
    exit 0
fi

DXVK_SAREK_SRC="$SRC_DIR/dxvk-sarek"
DXVK_SAREK_OBJ="$BUILD_DIR/obj-dxvk-sarek"
CROSS_FILE="$BUILD_DIR/mingw64-cross.txt"

ensure_git_checkout "$DXVK_SAREK_REPO" "$DXVK_SAREK_REF" "$DXVK_SAREK_SRC"
write_mingw64_cross_file "$CROSS_FILE"

rm -rf "$DXVK_SAREK_OBJ" "$STAGE_DIR/dxvk-sarek"
mkdir -p "$OUT_DLL_DIR"

meson setup "$DXVK_SAREK_OBJ" "$DXVK_SAREK_SRC" \
    --cross-file "$CROSS_FILE" \
    --buildtype=release \
    --prefix="$BUILD_DIR/install-dxvk-sarek" \
    -Denable_ddraw=false \
    -Denable_d3d8=false \
    -Denable_d3d9=false \
    -Denable_d3d10=false \
    -Denable_d3d11=true \
    -Denable_dxgi=true

ninja -C "$DXVK_SAREK_OBJ" -j"$(nproc)"

for dll in d3d11.dll dxgi.dll; do
    found="$(find "$DXVK_SAREK_OBJ" -type f -name "$dll" | head -n1)"
    [[ -n "$found" ]] || { echo "[dxvk-sarek] ERROR: $dll not produced"; exit 1; }
    cp -f "$found" "$OUT_DLL_DIR/$dll"
done

write_stamp "dxvk-sarek" "$STAMP_KEY"
echo "[dxvk-sarek] build complete (d3d11.dll + dxgi.dll only)"
