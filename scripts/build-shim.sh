#!/usr/bin/env bash
set -euo pipefail

SCRIPTS_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "$SCRIPTS_DIR/common.sh"

SRC_FILE="$REPO_ROOT/helpers/steam_shim.c"
OUT_DIR="$STAGE_DIR/shim/x86_64-windows"
OUT_BIN="$OUT_DIR/steam.exe"
STAMP_KEY="$(sha256sum "$SRC_FILE" | awk '{print $1}')"

if stamp_matches shim "$STAMP_KEY" && [[ -f "$OUT_BIN" ]]; then
    echo "[shim] up to date"
    exit 0
fi

mkdir -p "$OUT_DIR"
echo "[shim] compiling native steam.exe shim"
x86_64-w64-mingw32-gcc -O2 -Wall -Wextra -municode -mconsole -s \
    -o "$OUT_BIN" "$SRC_FILE"

write_stamp shim "$STAMP_KEY"
echo "[shim] staged $OUT_BIN"
