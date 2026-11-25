#!/usr/bin/env bash
set -euo pipefail

source "$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/common.sh"

STAMP_KEY="vkd3d:${VKD3D_PROTON_REF}"
OUT_DLL_DIR="$STAGE_DIR/vkd3d/x86_64-windows"

if stamp_matches "vkd3d" "$STAMP_KEY" && [[ -f "$OUT_DLL_DIR/d3d12.dll" ]] && [[ -f "$OUT_DLL_DIR/d3d12core.dll" ]]; then
    echo "[vkd3d] stamp matches ($STAMP_KEY), skipping build"
    exit 0
fi

VKD3D_SRC="$SRC_DIR/vkd3d-proton"
VKD3D_OBJ="$BUILD_DIR/obj-vkd3d"
CROSS_FILE="$BUILD_DIR/mingw64-cross.txt"

ensure_git_checkout "$VKD3D_PROTON_REPO" "$VKD3D_PROTON_REF" "$VKD3D_SRC"
write_mingw64_cross_file "$CROSS_FILE"

rm -rf "$VKD3D_OBJ" "$STAGE_DIR/vkd3d"
mkdir -p "$OUT_DLL_DIR"

export PATH="$STAGE_DIR/wine/usr/bin:$PATH"

# Ubuntu 24.04 ships mingw-w64 11 whose <pathcch.h> declares PathCchCombineEx
# without PATHCCH_NONE (added in mingw-w64 12). Probe cleanly so newer mingw-w64
# enum definitions are never broken by a macro override.
EXTRA_MESON_ARGS=()
if ! printf '#include <pathcch.h>\nunsigned long x = PATHCCH_NONE;\n' \
    | x86_64-w64-mingw32-gcc -fsyntax-only -x c - >/dev/null 2>&1; then
    echo "[vkd3d] mingw-w64 <pathcch.h> lacks PATHCCH_NONE; defining fallback 0"
    EXTRA_MESON_ARGS+=("-Dc_args=-DPATHCCH_NONE=0")
fi

meson setup "$VKD3D_OBJ" "$VKD3D_SRC" \
    --cross-file "$CROSS_FILE" \
    --buildtype=release \
    --prefix="$BUILD_DIR/install-vkd3d" \
    -Denable_tests=false \
    -Denable_extras=false \
    "${EXTRA_MESON_ARGS[@]}"

ninja -C "$VKD3D_OBJ" -j"$(nproc)"

for dll in d3d12.dll d3d12core.dll; do
    found="$(find "$VKD3D_OBJ" -type f -name "$dll" | head -n1)"
    [[ -n "$found" ]] || { echo "[vkd3d] ERROR: $dll not produced"; exit 1; }
    cp -f "$found" "$OUT_DLL_DIR/$dll"
done

write_stamp "vkd3d" "$STAMP_KEY"
echo "[vkd3d] build complete (d3d12.dll + d3d12core.dll)"
