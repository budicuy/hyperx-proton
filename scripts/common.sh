#!/usr/bin/env bash
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
source "$REPO_ROOT/versions.env"

BUILD_DIR="${BUILD_DIR:-$REPO_ROOT/build}"
SRC_DIR="${SRC_DIR:-$BUILD_DIR/src}"
STAGE_DIR="${STAGE_DIR:-$BUILD_DIR/stage}"
STAMP_DIR="${STAMP_DIR:-$BUILD_DIR/.stamps}"
DIST_DIR="${DIST_DIR:-$REPO_ROOT/dist}"
HYPERX_MARCH="${HYPERX_MARCH:-x86-64}"

mkdir -p "$BUILD_DIR" "$SRC_DIR" "$STAGE_DIR" "$STAMP_DIR" "$DIST_DIR"

if command -v ccache >/dev/null 2>&1; then
    export CCACHE_DIR="${CCACHE_DIR:-$HOME/.cache/ccache}"
    CC_WRAPPER="ccache"
else
    CC_WRAPPER=""
fi

ensure_git_checkout() {
    local url="$1"
    local ref="$2"
    local dest="$3"

    if [[ ! -d "$dest/.git" ]]; then
        git clone "$url" "$dest"
    fi

    git -C "$dest" fetch --tags --force origin
    git -C "$dest" checkout -f "$ref"
    git -C "$dest" reset --hard "$ref"
    git -C "$dest" clean -fdx
    if [[ -f "$dest/.gitmodules" ]]; then
        git -C "$dest" submodule update --init --recursive
    fi
}

stamp_matches() {
    local component="$1"
    local expected="$2"
    local stamp_file="$STAMP_DIR/${component}.stamp"
    [[ -f "$stamp_file" ]] && [[ "$(cat "$stamp_file")" == "$expected" ]]
}

write_stamp() {
    local component="$1"
    local value="$2"
    printf '%s\n' "$value" > "$STAMP_DIR/${component}.stamp"
}

hash_patches() {
    local dir="$1"
    if [[ ! -d "$dir" ]]; then
        echo "none"
        return
    fi
    local files
    files="$(find "$dir" -maxdepth 1 -type f -name '*.patch' | sort)"
    if [[ -z "$files" ]]; then
        echo "none"
    else
        sha256sum $files | sha256sum | awk '{print $1}'
    fi
}

write_mingw64_cross_file() {
    local out_file="$1"
    local c_cmd="x86_64-w64-mingw32-gcc"
    local cpp_cmd="x86_64-w64-mingw32-g++"
    if [[ -n "$CC_WRAPPER" ]]; then
        c_cmd="['ccache', 'x86_64-w64-mingw32-gcc']"
        cpp_cmd="['ccache', 'x86_64-w64-mingw32-g++']"
    else
        c_cmd="'x86_64-w64-mingw32-gcc'"
        cpp_cmd="'x86_64-w64-mingw32-g++'"
    fi

    cat > "$out_file" <<EOF
[binaries]
c = $c_cmd
cpp = $cpp_cmd
ar = 'x86_64-w64-mingw32-ar'
strip = 'x86_64-w64-mingw32-strip'
windres = 'x86_64-w64-mingw32-windres'

[host_machine]
system = 'windows'
cpu_family = 'x86_64'
cpu = 'x86_64'
endian = 'little'
EOF
}
