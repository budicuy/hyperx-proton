#!/usr/bin/env bash
set -euo pipefail

SCRIPTS_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "$SCRIPTS_DIR/common.sh"

SRC_RUNNER="$DIST_DIR/$HYPERX_PROTON_VERSION"
[[ -x "$SRC_RUNNER/proton" ]] || {
    echo "Packaged runner not found at $SRC_RUNNER. Run scripts/package-runner.sh first." >&2
    exit 1
}

INSTALL_BASE="${XDG_DATA_HOME:-$HOME/.local/share}/hyperx-launcher/runners/HyperX-Proton"
DEST_RUNNER="$INSTALL_BASE/$HYPERX_PROTON_VERSION"

mkdir -p "$INSTALL_BASE"
rm -rf "$DEST_RUNNER"
cp -a "$SRC_RUNNER" "$DEST_RUNNER"
ln -sfn "$HYPERX_PROTON_VERSION" "$INSTALL_BASE/current"

echo "[install-local] installed $DEST_RUNNER"
echo "[install-local] updated symlink $INSTALL_BASE/current -> $HYPERX_PROTON_VERSION"
