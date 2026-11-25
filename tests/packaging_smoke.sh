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
touch "$STAGE_DIR/wine/usr/lib/wine/i386-windows/kernel32.dll"
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

if find "$RUNNER_DIR" \( -name 'd3d9.dll' -o -name 'ddraw.dll' \) | grep -q .; then
    echo "ERROR: forbidden d3d9.dll or ddraw.dll was copied into runner!"
    exit 1
fi

[[ "$( "$RUNNER_DIR/proton" version )" == "$HYPERX_PROTON_VERSION" ]]
"$RUNNER_DIR/proton" doctor

echo "packaging_smoke.sh: ALL CHECKS PASSED"
