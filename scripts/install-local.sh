#!/usr/bin/env bash
set -euo pipefail

SCRIPTS_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "$SCRIPTS_DIR/common.sh"

TARGET="hyperx"
while [[ $# -gt 0 ]]; do
    case "$1" in
        --target)
            TARGET="${2:?missing value for --target}"
            shift 2
            ;;
        -h|--help)
            echo "Usage: $0 [--target hyperx|steam|heroic]"
            exit 0
            ;;
        *)
            echo "Unknown argument: $1" >&2
            echo "Usage: $0 [--target hyperx|steam|heroic]" >&2
            exit 2
            ;;
    esac
done

SRC_RUNNER="$DIST_DIR/$HYPERX_PROTON_VERSION"
[[ -x "$SRC_RUNNER/proton" ]] || {
    echo "Packaged runner not found at $SRC_RUNNER. Run scripts/package-runner.sh first." >&2
    exit 1
}

case "$TARGET" in
    hyperx)
        INSTALL_BASE="${XDG_DATA_HOME:-$HOME/.local/share}/hyperx-launcher/runners/HyperX-Proton"
        ;;
    steam)
        INSTALL_BASE="${XDG_DATA_HOME:-$HOME/.local/share}/Steam/compatibilitytools.d"
        ;;
    heroic)
        INSTALL_BASE="${XDG_CONFIG_HOME:-$HOME/.config}/heroic/tools/proton"
        ;;
    *)
        echo "Invalid --target '$TARGET' (expected: hyperx, steam, or heroic)" >&2
        exit 2
        ;;
esac

DEST_RUNNER="$INSTALL_BASE/$HYPERX_PROTON_VERSION"

mkdir -p "$INSTALL_BASE"
rm -rf "$DEST_RUNNER"
cp -a "$SRC_RUNNER" "$DEST_RUNNER"

echo "[install-local] installed $DEST_RUNNER (target: $TARGET)"
if [[ "$TARGET" == "hyperx" ]]; then
    ln -sfn "$HYPERX_PROTON_VERSION" "$INSTALL_BASE/current"
    echo "[install-local] updated symlink $INSTALL_BASE/current -> $HYPERX_PROTON_VERSION"
fi
