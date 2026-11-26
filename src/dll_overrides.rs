use std::collections::HashSet;

const BASE_OVERRIDES: &[(&str, &str)] = &[
    ("d3d11", "n"),
    ("dxgi", "n"),
    ("d3d12", "n"),
    ("d3d12core", "n"),
    ("lsteamclient", "d"),
    ("mscoree", "d"),
    ("mshtml", "d"),
    ("winemenubuilder.exe", "d"),
];

fn normalize_dll_name(raw: &str) -> String {
    let trimmed = raw.trim().to_ascii_lowercase();
    trimmed
        .strip_suffix(".dll")
        .unwrap_or(&trimmed)
        .to_string()
}

/// Merges `WINEDLLOVERRIDES` non-destructively, layering `existing` (user/launcher)
/// over `extra_overrides` (game rules), over `BASE_OVERRIDES`, and finally `nvapi64=n`.
pub fn merge_dll_overrides_with_extras(
    existing: Option<&str>,
    extra_overrides: &[String],
    enable_nvapi: bool,
) -> String {
    let mut entries: Vec<String> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();

    if let Some(raw) = existing {
        for part in raw.split(';') {
            let trimmed = part.trim();
            if trimmed.is_empty() {
                continue;
            }
            if let Some((dll_part, _mode)) = trimmed.split_once('=') {
                let key = normalize_dll_name(dll_part);
                if !key.is_empty() {
                    seen.insert(key);
                }
            }
            entries.push(trimmed.to_string());
        }
    }

    for extra in extra_overrides {
        for part in extra.split(';') {
            let trimmed = part.trim();
            if trimmed.is_empty() {
                continue;
            }
            if let Some((dll_part, _mode)) = trimmed.split_once('=') {
                let key = normalize_dll_name(dll_part);
                if key.is_empty() || !seen.insert(key) {
                    continue;
                }
            }
            entries.push(trimmed.to_string());
        }
    }

    for (dll, mode) in BASE_OVERRIDES {
        if seen.insert((*dll).to_string()) {
            entries.push(format!("{dll}={mode}"));
        }
    }

    if enable_nvapi && seen.insert("nvapi64".to_string()) {
        entries.push("nvapi64=n".to_string());
    }

    entries.join(";")
}
