#!/usr/bin/env bash
set -euo pipefail

source "$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/common.sh"

STAMP_KEY="nvapi:${DXVK_NVAPI_REF}"
OUT_DLL_DIR="$STAGE_DIR/nvapi/x86_64-windows"

if stamp_matches "nvapi" "$STAMP_KEY" && [[ -f "$OUT_DLL_DIR/nvapi64.dll" ]]; then
    echo "[nvapi] stamp matches ($STAMP_KEY), skipping build"
    exit 0
fi

NVAPI_SRC="$SRC_DIR/dxvk-nvapi"
NVAPI_OBJ="$BUILD_DIR/obj-nvapi"
CROSS_FILE="$BUILD_DIR/mingw64-cross.txt"

ensure_git_checkout "$DXVK_NVAPI_REPO" "$DXVK_NVAPI_REF" "$NVAPI_SRC"
write_mingw64_cross_file "$CROSS_FILE"

rm -rf "$NVAPI_OBJ" "$STAGE_DIR/nvapi"
mkdir -p "$OUT_DLL_DIR"

meson setup "$NVAPI_OBJ" "$NVAPI_SRC" \
    --cross-file "$CROSS_FILE" \
    --buildtype=release \
    --prefix="$BUILD_DIR/install-nvapi" \
    -Denable_tests=false

ninja -C "$NVAPI_OBJ" -j"$(nproc)"

found="$(find "$NVAPI_OBJ" -type f -name 'nvapi64.dll' | head -n1)"
[[ -n "$found" ]] || { echo "[nvapi] ERROR: nvapi64.dll not produced"; exit 1; }
cp -f "$found" "$OUT_DLL_DIR/nvapi64.dll"

write_stamp "nvapi" "$STAMP_KEY"
echo "[nvapi] build complete (nvapi64.dll)"
