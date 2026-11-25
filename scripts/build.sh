#!/usr/bin/env bash
set -euo pipefail

SCRIPTS_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "$SCRIPTS_DIR/common.sh"

COMPONENT="all"
while [[ $# -gt 0 ]]; do
    case "$1" in
        --component)
            COMPONENT="${2:?missing value for --component}"
            shift 2
            ;;
        -h|--help)
            echo "Usage: $0 [--component all|rust|wine|dxvk|vkd3d|nvapi|icu|shim]"
            exit 0
            ;;
        *)
            echo "Unknown argument: $1" >&2
            exit 2
            ;;
    esac
done

check_tools() {
    local missing=()
    for cmd in "$@"; do
        if ! command -v "$cmd" >/dev/null 2>&1; then
            missing+=("$cmd")
        fi
    done
    if [[ ${#missing[@]} -gt 0 ]]; then
        echo "ERROR: missing required build tools: ${missing[*]}" >&2
        exit 1
    fi
}

build_rust() {
    check_tools cargo
    echo "[rust] building hyperx-proton (release)"
    (cd "$REPO_ROOT" && cargo build --release)
}

case "$COMPONENT" in
    all)
        check_tools cargo x86_64-w64-mingw32-gcc meson ninja glslangValidator bison flex curl unzip
        build_rust
        bash "$SCRIPTS_DIR/build-wine.sh"
        bash "$SCRIPTS_DIR/build-dxvk.sh"
        bash "$SCRIPTS_DIR/build-vkd3d.sh"
        bash "$SCRIPTS_DIR/build-nvapi.sh"
        bash "$SCRIPTS_DIR/build-icu.sh"
        bash "$SCRIPTS_DIR/build-shim.sh"
        ;;
    rust)
        build_rust
        ;;
    wine)
        check_tools bison flex
        bash "$SCRIPTS_DIR/build-wine.sh"
        ;;
    dxvk)
        check_tools x86_64-w64-mingw32-gcc meson ninja glslangValidator
        bash "$SCRIPTS_DIR/build-dxvk.sh"
        ;;
    vkd3d)
        check_tools x86_64-w64-mingw32-gcc meson ninja glslangValidator
        bash "$SCRIPTS_DIR/build-vkd3d.sh"
        ;;
    nvapi)
        check_tools x86_64-w64-mingw32-gcc meson ninja
        bash "$SCRIPTS_DIR/build-nvapi.sh"
        ;;
    icu)
        check_tools curl unzip
        bash "$SCRIPTS_DIR/build-icu.sh"
        ;;
    shim)
        check_tools x86_64-w64-mingw32-gcc
        bash "$SCRIPTS_DIR/build-shim.sh"
        ;;
    *)
        echo "Unknown component: $COMPONENT" >&2
        exit 2
        ;;
esac
