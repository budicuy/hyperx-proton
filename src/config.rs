use serde::Deserialize;
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

/// Manifest schema stored in `<runner_root>/hyperx-native.json`.
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct HyperxNativeManifest {
    pub schema_version: u32,
    pub runner_mode: String,
    pub requires_steamrt: bool,
    pub name: String,
    pub version: String,
    pub arch: String,
    #[serde(default)]
    pub components: BTreeMap<String, String>,
}

/// Resolved Wine prefix and lockfile paths for a launch session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrefixLayout {
    pub wineprefix: PathBuf,
    pub compat_data_dir: Option<PathBuf>,
    pub lock_file: PathBuf,
}

/// Loads and validates `<runner_root>/hyperx-native.json`.
pub fn load_manifest(runner_root: &Path) -> Result<HyperxNativeManifest, String> {
    let manifest_path = runner_root.join("hyperx-native.json");
    let raw = fs::read_to_string(&manifest_path).map_err(|err| {
        format!(
            "failed to read manifest at {}: {err}",
            manifest_path.display()
        )
    })?;

    let manifest: HyperxNativeManifest = serde_json::from_str(&raw).map_err(|err| {
        format!(
            "failed to parse manifest at {}: {err}",
            manifest_path.display()
        )
    })?;

    if manifest.schema_version != 1 {
        return Err(format!(
            "unsupported hyperx-native.json schema_version: {} (expected 1)",
            manifest.schema_version
        ));
    }
    if manifest.runner_mode != "native" {
        return Err(format!(
            "invalid runner_mode {:?} in {} (expected \"native\")",
            manifest.runner_mode,
            manifest_path.display()
        ));
    }
    if manifest.requires_steamrt {
        return Err(format!(
            "invalid manifest {}: requires_steamrt must be false",
            manifest_path.display()
        ));
    }

    Ok(manifest)
}

fn non_empty_path(path: Option<&Path>) -> Option<&Path> {
    path.filter(|p| !p.as_os_str().is_empty())
}

/// Resolves prefix layout from `STEAM_COMPAT_DATA_PATH` and `WINEPREFIX`.
///
/// When `compat_data_path` is provided, `<compat_data_path>/pfx` is always used
/// so existing `SHARED/pfx` directories are preserved without splitting into flat
/// `<dir>/drive_c`. The lockfile is placed inside `compat_data_path` or `wineprefix`
/// rather than a shared parent directory.
pub fn resolve_prefix_layout(
    compat_data_path: Option<&Path>,
    wineprefix: Option<&Path>,
) -> Result<PrefixLayout, String> {
    if let Some(compat) = non_empty_path(compat_data_path) {
        return Ok(PrefixLayout {
            wineprefix: compat.join("pfx"),
            compat_data_dir: Some(compat.to_path_buf()),
            lock_file: compat.join(".hyperx-proton.lock"),
        });
    }

    if let Some(prefix) = non_empty_path(wineprefix) {
        return Ok(PrefixLayout {
            wineprefix: prefix.to_path_buf(),
            compat_data_dir: None,
            lock_file: prefix.join(".hyperx-proton.lock"),
        });
    }

    Err("neither STEAM_COMPAT_DATA_PATH nor WINEPREFIX is set".to_string())
}
