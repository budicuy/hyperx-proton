#!/usr/bin/env bash
set -euo pipefail

SCRIPTS_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "$SCRIPTS_DIR/common.sh"

ICU_SRC_DIR="$SRC_DIR/icu"
ICU_STAGE_64="$STAGE_DIR/icu/x86_64-windows"
ICU_STAGE_32="$STAGE_DIR/icu/i386-windows"
STAMP_KEY="${ICU_RELEASE}:${ICU_WIN64_SHA256}:${ICU_WIN32_SHA256}"

if stamp_matches icu "$STAMP_KEY" \
    && [[ -f "$ICU_STAGE_64/icuuc68.dll" && -f "$ICU_STAGE_64/icuin68.dll" && -f "$ICU_STAGE_64/icudt68.dll" ]] \
    && [[ -f "$ICU_STAGE_32/icuuc68.dll" && -f "$ICU_STAGE_32/icuin68.dll" && -f "$ICU_STAGE_32/icudt68.dll" ]]; then
    echo "[icu] up to date ($ICU_RELEASE)"
    exit 0
fi

mkdir -p "$ICU_SRC_DIR" "$ICU_STAGE_64" "$ICU_STAGE_32"

fetch_and_verify() {
    local url="$1"
    local expected_sha="$2"
    local dest="$3"

    if [[ ! -f "$dest" ]] || [[ "$(sha256sum "$dest" | awk '{print $1}')" != "$expected_sha" ]]; then
        echo "[icu] downloading $(basename "$dest")"
        curl -fL --retry 3 -o "$dest" "$url"
    fi

    local actual_sha
    actual_sha="$(sha256sum "$dest" | awk '{print $1}')"
    if [[ "$actual_sha" != "$expected_sha" ]]; then
        echo "ERROR: SHA-256 mismatch for $dest (expected $expected_sha, got $actual_sha)" >&2
        exit 1
    fi
}

WIN64_ZIP="$ICU_SRC_DIR/icu4c-68_2-Win64-MSVC2019.zip"
WIN32_ZIP="$ICU_SRC_DIR/icu4c-68_2-Win32-MSVC2019.zip"

fetch_and_verify "$ICU_WIN64_URL" "$ICU_WIN64_SHA256" "$WIN64_ZIP"
fetch_and_verify "$ICU_WIN32_URL" "$ICU_WIN32_SHA256" "$WIN32_ZIP"

echo "[icu] extracting icudt68/icuin68/icuuc68 DLLs"
unzip -o -j -q "$WIN64_ZIP" \
    bin64/icudt68.dll bin64/icuin68.dll bin64/icuuc68.dll \
    -d "$ICU_STAGE_64"
unzip -o -j -q "$WIN32_ZIP" \
    bin/icudt68.dll bin/icuin68.dll bin/icuuc68.dll \
    -d "$ICU_STAGE_32"

write_stamp icu "$STAMP_KEY"
echo "[icu] staged into $STAGE_DIR/icu"
