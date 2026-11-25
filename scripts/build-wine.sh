#!/usr/bin/env bash
set -euo pipefail

source "$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/common.sh"

PATCH_HASH="$(hash_patches "$REPO_ROOT/patches/wine")"
STAMP_KEY="wine:${WINE_REF}:${HYPERX_MARCH}:portable-v1:${PATCH_HASH}"

if stamp_matches "wine" "$STAMP_KEY" && [[ -x "$STAGE_DIR/wine/usr/bin/wine" ]]; then
    echo "[wine] stamp matches ($STAMP_KEY), skipping build"
    exit 0
fi

WINE_SRC="$SRC_DIR/wine-cachyos"
WINE_OBJ="$BUILD_DIR/obj-wine"

ensure_git_checkout "$WINE_REPO" "$WINE_REF" "$WINE_SRC"

shopt -s nullglob
for patch_file in "$REPO_ROOT"/patches/wine/*.patch; do
    echo "[wine] applying patch: $(basename "$patch_file")"
    git -C "$WINE_SRC" apply "$patch_file"
done
shopt -u nullglob

rm -rf "$WINE_OBJ" "$STAGE_DIR/wine"
mkdir -p "$WINE_OBJ" "$STAGE_DIR/wine"

if [[ -n "$CC_WRAPPER" ]]; then
    export CC="ccache gcc"
    export CXX="ccache g++"
else
    export CC="gcc"
    export CXX="g++"
fi
export CFLAGS="-O2 -march=${HYPERX_MARCH} -mtune=generic -pipe"
export CXXFLAGS="-O2 -march=${HYPERX_MARCH} -mtune=generic -pipe"

if [[ ! -x "$WINE_SRC/configure" ]]; then
    echo "[wine] running autogen.sh"
    (cd "$WINE_SRC" && ./autogen.sh)
fi

cd "$WINE_OBJ"
"$WINE_SRC/configure" \
    --prefix=/usr \
    --enable-archs=i386,x86_64 \
    --with-x \
    --with-wayland \
    --with-vulkan \
    --with-gnutls \
    --with-gstreamer \
    --with-freetype \
    --with-fontconfig \
    --with-pulse \
    --with-udev \
    --with-sdl \
    --without-cups \
    --without-sane \
    --without-gphoto \
    --without-capi \
    --without-oss \
    --without-v4l2 \
    --without-pcsclite \
    --without-krb5 \
    --without-netapi \
    --without-opencl \
    --without-osmesa \
    --without-pcap \
    --without-ffmpeg \
    --without-unwind

make -j"$(nproc)"
make DESTDIR="$STAGE_DIR/wine" install

write_stamp "wine" "$STAMP_KEY"
echo "[wine] build complete"
