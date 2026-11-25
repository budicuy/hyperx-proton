#!/usr/bin/env bash
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$REPO_ROOT"

# 1. Syntax check all shell scripts and PKGBUILD
shopt -s nullglob
for f in scripts/*.sh packaging/arch/PKGBUILD; do
    bash -n "$f"
done

# 2. Ensure release binary exists for packaging smoke test
if [[ ! -x "$REPO_ROOT/target/release/hyperx-proton" ]]; then
    cargo build --release
fi

TMP_DIR="$(mktemp -d)"
trap 'rm -rf "$TMP_DIR"' EXIT

STAGE_DIR="$TMP_DIR/stage"
OUT_DIR="$TMP_DIR/dist"

mkdir -p \
    "$STAGE_DIR/wine/usr/bin" \
    "$STAGE_DIR/wine/usr/lib/wine/x86_64-unix" \
    "$STAGE_DIR/wine/usr/lib/wine/x86_64-windows" \
    "$STAGE_DIR/wine/usr/lib/wine/i386-windows" \
    "$STAGE_DIR/wine/usr/share/wine" \
    "$STAGE_DIR/dxvk/x86_64-windows" \
    "$STAGE_DIR/dxvk-sarek/x86_64-windows" \
    "$STAGE_DIR/vkd3d/x86_64-windows" \
    "$STAGE_DIR/nvapi/x86_64-windows" \
    "$STAGE_DIR/icu/x86_64-windows" \
    "$STAGE_DIR/icu/i386-windows" \
    "$STAGE_DIR/shim/x86_64-windows"

printf '#!/usr/bin/env bash\nexit 0\n' > "$STAGE_DIR/wine/usr/bin/wine"
printf '#!/usr/bin/env bash\nexit 0\n' > "$STAGE_DIR/wine/usr/bin/wineserver"
printf '#!/usr/bin/env bash\nexit 0\n' > "$STAGE_DIR/wine/usr/lib/wine/x86_64-unix/wine64"
printf '#!/usr/bin/env bash\nexit 0\n' > "$STAGE_DIR/wine/usr/lib/wine/x86_64-unix/wine64-preloader"
chmod +x \
    "$STAGE_DIR/wine/usr/bin/wine" \
    "$STAGE_DIR/wine/usr/bin/wineserver" \
    "$STAGE_DIR/wine/usr/lib/wine/x86_64-unix/wine64" \
    "$STAGE_DIR/wine/usr/lib/wine/x86_64-unix/wine64-preloader"
touch "$STAGE_DIR/wine/usr/lib/wine/x86_64-unix/ntdll.so"
touch "$STAGE_DIR/wine/usr/lib/wine/x86_64-windows/kernel32.dll"
touch "$STAGE_DIR/wine/usr/lib/wine/x86_64-windows/libkernel32.a"
touch "$STAGE_DIR/wine/usr/lib/wine/i386-windows/kernel32.dll"
touch "$STAGE_DIR/wine/usr/lib/wine/i386-windows/libkernel32.a"
touch "$STAGE_DIR/wine/usr/share/wine/wine.inf"

# Dummy DX11/DX12/NVAPI DLLs + forbidden d3d9.dll/ddraw.dll to verify strict filtering
printf 'd3d11' > "$STAGE_DIR/dxvk/x86_64-windows/d3d11.dll"
printf 'dxgi' > "$STAGE_DIR/dxvk/x86_64-windows/dxgi.dll"
printf 'forbidden' > "$STAGE_DIR/dxvk/x86_64-windows/d3d9.dll"
printf 'sarek-d3d11' > "$STAGE_DIR/dxvk-sarek/x86_64-windows/d3d11.dll"
printf 'sarek-dxgi' > "$STAGE_DIR/dxvk-sarek/x86_64-windows/dxgi.dll"
printf 'forbidden' > "$STAGE_DIR/dxvk-sarek/x86_64-windows/d3d9.dll"
printf 'forbidden' > "$STAGE_DIR/dxvk-sarek/x86_64-windows/ddraw.dll"
printf 'd3d12' > "$STAGE_DIR/vkd3d/x86_64-windows/d3d12.dll"
printf 'd3d12core' > "$STAGE_DIR/vkd3d/x86_64-windows/d3d12core.dll"
printf 'nvapi64' > "$STAGE_DIR/nvapi/x86_64-windows/nvapi64.dll"
for arch in x86_64-windows i386-windows; do
    for dll in icudt68.dll icuin68.dll icuuc68.dll; do
        printf '%s' "$dll" > "$STAGE_DIR/icu/$arch/$dll"
    done
done
printf 'steam-shim' > "$STAGE_DIR/shim/x86_64-windows/steam.exe"

bash "$REPO_ROOT/scripts/package-runner.sh" \
    --stage-dir "$STAGE_DIR" \
    --out-dir "$OUT_DIR"

source "$REPO_ROOT/versions.env"
RUNNER_DIR="$OUT_DIR/$HYPERX_PROTON_VERSION"

[[ -x "$RUNNER_DIR/proton" ]]
[[ -f "$RUNNER_DIR/version" ]]
[[ "$(tr -d '[:space:]' < "$RUNNER_DIR/version")" == "$HYPERX_PROTON_VERSION" ]]

# Validate hyperx-native.json using python3 or jq
python3 - "$RUNNER_DIR/hyperx-native.json" "$HYPERX_PROTON_VERSION" <<'PY'
import json, sys
with open(sys.argv[1], "r", encoding="utf-8") as f:
    data = json.load(f)
assert data["schema_version"] == 1
assert data["runner_mode"] == "native"
assert data["requires_steamrt"] is False
assert data["version"] == sys.argv[2]
assert "dxvk_sarek" in data["components"]
PY

# Verify Valve/Heroic VDF metadata exists, matches version & commandline, and has NO require_tool_appid
[[ -f "$RUNNER_DIR/compatibilitytool.vdf" ]] || { echo "Missing compatibilitytool.vdf"; exit 1; }
grep -qF "\"$HYPERX_PROTON_VERSION\"" "$RUNNER_DIR/compatibilitytool.vdf" || { echo "compatibilitytool.vdf missing version $HYPERX_PROTON_VERSION"; exit 1; }
[[ -f "$RUNNER_DIR/toolmanifest.vdf" ]] || { echo "Missing toolmanifest.vdf"; exit 1; }
grep -qF '"/proton %verb%"' "$RUNNER_DIR/toolmanifest.vdf" || { echo "toolmanifest.vdf missing commandline \"/proton %verb%\""; exit 1; }
if grep -qi 'require_tool_appid' "$RUNNER_DIR/compatibilitytool.vdf" "$RUNNER_DIR/toolmanifest.vdf"; then
    echo "ERROR: VDF metadata must not contain require_tool_appid!"
    exit 1
fi

# Verify required DLLs and preloader_exec symlinks exist and forbidden d3d9.dll was excluded
for path in \
    "files/lib/wine/i386-unix/wine64" \
    "files/lib/wine/i386-unix/wine64-preloader" \
    "files/lib/wine/x86_64-windows/steam.exe" \
    "files/lib/wine/icu/x86_64-windows/icudt68.dll" \
    "files/lib/wine/icu/x86_64-windows/icuin68.dll" \
    "files/lib/wine/icu/x86_64-windows/icuuc68.dll" \
    "files/lib/wine/icu/i386-windows/icudt68.dll" \
    "files/lib/wine/icu/i386-windows/icuin68.dll" \
    "files/lib/wine/icu/i386-windows/icuuc68.dll" \
    "files/lib/dxvk/x86_64-windows/d3d11.dll" \
    "files/lib/dxvk/x86_64-windows/dxgi.dll" \
    "files/lib/dxvk-sarek/x86_64-windows/d3d11.dll" \
    "files/lib/dxvk-sarek/x86_64-windows/dxgi.dll" \
    "files/lib/vkd3d-proton/x86_64-windows/d3d12.dll" \
    "files/lib/vkd3d-proton/x86_64-windows/d3d12core.dll" \
    "files/lib/nvapi/x86_64-windows/nvapi64.dll"; do
    [[ -f "$RUNNER_DIR/$path" ]] || { echo "Missing $path"; exit 1; }
done

if find "$RUNNER_DIR" \( -name 'd3d9.dll' -o -name 'ddraw.dll' -o -name '*.a' \) | grep -q .; then
    echo "ERROR: forbidden d3d9.dll, ddraw.dll, or static .a archive was copied into runner!"
    exit 1
fi

[[ "$( "$RUNNER_DIR/proton" version )" == "$HYPERX_PROTON_VERSION" ]]
"$RUNNER_DIR/proton" doctor

# Verify release manifest generation (latest.json + <asset>.sha256)
TARBALL="$OUT_DIR/${HYPERX_PROTON_VERSION}.tar.zst"
[[ -f "$TARBALL" ]] || { echo "Missing smoke tarball: $TARBALL (is zstd installed?)"; exit 1; }

RELEASE_TAG="v${HYPERX_PROTON_VERSION#hyperx-proton-}"
bash "$REPO_ROOT/scripts/generate-release-manifest.sh" \
    --tarball "$TARBALL" \
    --tag "$RELEASE_TAG" \
    --out-dir "$OUT_DIR" \
    --release-notes "Smoke test release"

SHA256_FILE="${TARBALL}.sha256"
MANIFEST_FILE="$OUT_DIR/latest.json"
[[ -f "$SHA256_FILE" ]] || { echo "Missing checksum file: $SHA256_FILE"; exit 1; }
[[ -f "$MANIFEST_FILE" ]] || { echo "Missing manifest file: $MANIFEST_FILE"; exit 1; }

(cd "$OUT_DIR" && sha256sum -c "$(basename "$SHA256_FILE")")

EXPECTED_SHA256="$(sha256sum "$TARBALL" | awk '{print $1}')"
python3 - "$MANIFEST_FILE" "$HYPERX_PROTON_VERSION" "$RELEASE_TAG" "$EXPECTED_SHA256" "$(basename "$TARBALL")" <<'PY'
import json, re, sys
manifest_path, expected_ver, expected_tag, expected_sha, expected_asset = sys.argv[1:6]
with open(manifest_path, "r", encoding="utf-8") as f:
    data = json.load(f)
assert data["schema_version"] == 1, data
assert data["version"] == expected_ver, data
assert data["tag"] == expected_tag, data
assert data["asset_name"] == expected_asset, data
assert re.fullmatch(r"[0-9a-f]{64}", data["sha256"]), data["sha256"]
assert data["sha256"] == expected_sha, (data["sha256"], expected_sha)
assert isinstance(data["size_bytes"], int) and data["size_bytes"] > 0, data["size_bytes"]
assert "budicuy/hyperx-proton/releases/download/" in data["download_url"], data["download_url"]
assert data["download_url"].endswith(f"/{expected_tag}/{expected_asset}"), data["download_url"]
assert data["min_glibc"] == "2.38", data["min_glibc"]
assert isinstance(data["published_at"], str) and data["published_at"].endswith("Z"), data["published_at"]
assert data["release_notes"] == "Smoke test release", data["release_notes"]
PY

echo "packaging_smoke.sh: ALL CHECKS PASSED"
