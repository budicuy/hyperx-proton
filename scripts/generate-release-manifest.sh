#!/usr/bin/env bash
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
source "$REPO_ROOT/versions.env"

TARBALL=""
TAG=""
REPO="budicuy/hyperx-proton"
OUT_DIR=""
RELEASE_NOTES=""

while [[ $# -gt 0 ]]; do
    case "$1" in
        --tarball)
            TARBALL="${2:?missing value for --tarball}"
            shift 2
            ;;
        --tag)
            TAG="${2:?missing value for --tag}"
            shift 2
            ;;
        --repo)
            REPO="${2:?missing value for --repo}"
            shift 2
            ;;
        --out-dir)
            OUT_DIR="${2:?missing value for --out-dir}"
            shift 2
            ;;
        --release-notes)
            RELEASE_NOTES="${2:?missing value for --release-notes}"
            shift 2
            ;;
        -h|--help)
            echo "Usage: $0 --tarball <path> --tag <v...> [--repo budicuy/hyperx-proton] [--out-dir <dir>] [--release-notes <text>]"
            exit 0
            ;;
        *)
            echo "Unknown argument: $1" >&2
            exit 2
            ;;
    esac
done

[[ -n "$TARBALL" ]] || { echo "ERROR: --tarball is required" >&2; exit 2; }
[[ -f "$TARBALL" ]] || { echo "ERROR: tarball not found: $TARBALL" >&2; exit 1; }
[[ -n "$TAG" ]] || { echo "ERROR: --tag is required" >&2; exit 2; }

if [[ -z "$OUT_DIR" ]]; then
    OUT_DIR="$(dirname "$TARBALL")"
fi
mkdir -p "$OUT_DIR"

if [[ -z "$RELEASE_NOTES" ]]; then
    RELEASE_NOTES="Universal x86-64 build with DXVK ${DXVK_REF}, VKD3D-Proton ${VKD3D_PROTON_REF}."
fi

ASSET_NAME="$(basename "$TARBALL")"
SHA256="$(sha256sum "$TARBALL" | awk '{print $1}')"
SIZE_BYTES="$(stat -c%s "$TARBALL")"
DOWNLOAD_URL="https://github.com/${REPO}/releases/download/${TAG}/${ASSET_NAME}"

printf '%s  %s\n' "$SHA256" "$ASSET_NAME" > "${OUT_DIR}/${ASSET_NAME}.sha256"

python3 - "${OUT_DIR}/latest.json" "$HYPERX_PROTON_VERSION" "$TAG" "$ASSET_NAME" "$DOWNLOAD_URL" "$SHA256" "$SIZE_BYTES" "$RELEASE_NOTES" <<'PY'
from datetime import datetime, timezone
import json
import sys

(
    out_path,
    version,
    tag,
    asset_name,
    download_url,
    sha256,
    size_bytes_raw,
    release_notes,
) = sys.argv[1:9]

manifest = {
    "schema_version": 1,
    "version": version,
    "tag": tag,
    "asset_name": asset_name,
    "download_url": download_url,
    "sha256": sha256,
    "size_bytes": int(size_bytes_raw),
    "min_glibc": "2.38",
    "published_at": datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ"),
    "release_notes": release_notes,
}

with open(out_path, "w", encoding="utf-8") as f:
    json.dump(manifest, f, indent=2)
    f.write("\n")
PY

echo "[manifest] wrote ${OUT_DIR}/${ASSET_NAME}.sha256 and ${OUT_DIR}/latest.json"
