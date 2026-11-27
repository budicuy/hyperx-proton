use std::collections::{BTreeMap, HashSet};
use std::fs::{self, OpenOptions};
use std::path::{Component, Path, PathBuf};

use serde::Deserialize;

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct GameRule {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub match_criteria: RuleMatch,
    #[serde(default)]
    pub steam_game_id: Option<String>,
    #[serde(default)]
    pub env: BTreeMap<String, String>,
    #[serde(default)]
    pub dll_overrides: Vec<String>,
    #[serde(default)]
    pub use_steam_shim: Option<bool>,
    #[serde(default)]
    pub ensure_files: Vec<EnsureFileRule>,
}

#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
pub struct RuleMatch {
    #[serde(default)]
    pub exe_all: Vec<String>,
    #[serde(default)]
    pub exe_any: Vec<String>,
    #[serde(default)]
    pub steam_app_ids: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct EnsureFileRule {
    pub if_exists_rel: String,
    pub create_rel: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoadedRules {
    pub rules: Vec<GameRule>,
    pub builtin_count: usize,
    pub custom_count: usize,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RuleEvaluation {
    pub matched_rule_ids: Vec<String>,
    pub steam_game_id: Option<String>,
    pub env: BTreeMap<String, String>,
    pub dll_overrides: Vec<String>,
    pub use_steam_shim: Option<bool>,
    pub ensure_files: Vec<EnsureFileRule>,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum RuleFilePayload {
    Single(Box<GameRule>),
    Multiple(Vec<GameRule>),
}

pub fn builtin_rules() -> Vec<GameRule> {
    vec![
        GameRule {
            id: "wuthering-waves".to_string(),
            name: "Wuthering Waves".to_string(),
            match_criteria: RuleMatch {
                exe_all: vec!["client-win64-shipping.exe".to_string()],
                exe_any: vec!["wuwa".to_string(), "wuthering".to_string()],
                steam_app_ids: vec!["3513350".to_string()],
            },
            steam_game_id: Some("3513350".to_string()),
            env: BTreeMap::new(),
            dll_overrides: vec!["KRSDKExternal.exe=d".to_string()],
            use_steam_shim: Some(false),
            ensure_files: vec![
                EnsureFileRule {
                    if_exists_rel:
                        "ThirdParty/KrPcSdk_Global/KRSDKRes/KRSDKWebViewOSR/KRWebClient.exe"
                            .to_string(),
                    create_rel:
                        "ThirdParty/KrPcSdk_Global/KRSDKRes/KRSDKWebViewOSR/usebitmap.flag"
                            .to_string(),
                },
                EnsureFileRule {
                    if_exists_rel:
                        "ThirdParty/KrPcSdk_CN/KRSDKRes/KRSDKWebViewOSR/KRWebClient.exe"
                            .to_string(),
                    create_rel: "ThirdParty/KrPcSdk_CN/KRSDKRes/KRSDKWebViewOSR/usebitmap.flag"
                        .to_string(),
                },
            ],
        },
        GameRule {
            id: "honkai-star-rail".to_string(),
            name: "Honkai: Star Rail".to_string(),
            match_criteria: RuleMatch {
                exe_all: Vec::new(),
                exe_any: vec!["starrail.exe".to_string()],
                steam_app_ids: Vec::new(),
            },
            steam_game_id: None,
            env: BTreeMap::from([("STUB_WINTRUST".to_string(), "1".to_string())]),
            dll_overrides: vec!["jsproxy=n,b".to_string()],
            use_steam_shim: Some(true),
            ensure_files: Vec::new(),
        },
        GameRule {
            id: "genshin-impact".to_string(),
            name: "Genshin Impact".to_string(),
            match_criteria: RuleMatch {
                exe_all: Vec::new(),
                exe_any: vec!["genshinimpact.exe".to_string(), "yuanshen.exe".to_string()],
                steam_app_ids: Vec::new(),
            },
            steam_game_id: None,
            env: BTreeMap::from([("WINE_ENABLE_TIMEOUT_FIX".to_string(), "1".to_string())]),
            dll_overrides: Vec::new(),
            use_steam_shim: Some(true),
            ensure_files: Vec::new(),
        },
        GameRule {
            id: "zenless-zone-zero".to_string(),
            name: "Zenless Zone Zero".to_string(),
            match_criteria: RuleMatch {
                exe_all: Vec::new(),
                exe_any: vec!["zenlesszonezero.exe".to_string()],
                steam_app_ids: Vec::new(),
            },
            steam_game_id: None,
            env: BTreeMap::new(),
            dll_overrides: Vec::new(),
            use_steam_shim: Some(true),
            ensure_files: Vec::new(),
        },
    ]
}

pub fn resolve_custom_rules_dir(host_env: &BTreeMap<String, String>) -> Option<PathBuf> {
    if let Some(dir) = host_env
        .get("HYPERX_PROTON_RULES_DIR")
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
    {
        return Some(PathBuf::from(dir));
    }

    if let Some(xdg) = host_env
        .get("XDG_CONFIG_HOME")
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
    {
        return Some(PathBuf::from(xdg).join("hyperx-proton/rules"));
    }

    host_env
        .get("HOME")
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .map(|home| PathBuf::from(home).join(".config/hyperx-proton/rules"))
}

pub fn load_rules(host_env: &BTreeMap<String, String>) -> LoadedRules {
    let mut rules = builtin_rules();
    let builtin_count = rules.len();
    let mut custom_count = 0usize;
    let mut warnings = Vec::new();

    let Some(rules_dir) = resolve_custom_rules_dir(host_env) else {
        return LoadedRules {
            rules,
            builtin_count,
            custom_count,
            warnings,
        };
    };

    if !rules_dir.is_dir() {
        return LoadedRules {
            rules,
            builtin_count,
            custom_count,
            warnings,
        };
    }

    let entries = match fs::read_dir(&rules_dir) {
        Ok(rd) => rd,
        Err(err) => {
            warnings.push(format!(
                "failed to read rules directory {}: {err}",
                rules_dir.display()
            ));
            return LoadedRules {
                rules,
                builtin_count,
                custom_count,
                warnings,
            };
        }
    };

    let mut json_files: Vec<PathBuf> = entries
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| {
            p.is_file()
                && p.extension()
                    .and_then(|ext| ext.to_str())
                    .is_some_and(|ext| ext.eq_ignore_ascii_case("json"))
        })
        .collect();
    json_files.sort();

    for path in json_files {
        let raw = match fs::read_to_string(&path) {
            Ok(s) => s,
            Err(err) => {
                warnings.push(format!("failed to read rule file {}: {err}", path.display()));
                continue;
            }
        };

        let parsed = match serde_json::from_str::<RuleFilePayload>(&raw) {
            Ok(RuleFilePayload::Single(rule)) => vec![*rule],
            Ok(RuleFilePayload::Multiple(list)) => list,
            Err(err) => {
                warnings.push(format!(
                    "ignoring invalid rule JSON {}: {err}",
                    path.display()
                ));
                continue;
            }
        };

        for custom_rule in parsed {
            custom_count += 1;
            if let Some(pos) = rules.iter().position(|r| r.id == custom_rule.id) {
                rules[pos] = custom_rule;
            } else {
                rules.push(custom_rule);
            }
        }
    }

    LoadedRules {
        rules,
        builtin_count,
        custom_count,
        warnings,
    }
}

fn rule_matches(rule: &GameRule, exe_lower: &str, host_env: &BTreeMap<String, String>) -> bool {
    let criteria = &rule.match_criteria;
    if criteria.steam_app_ids.is_empty()
        && criteria.exe_all.is_empty()
        && criteria.exe_any.is_empty()
    {
        return false;
    }

    let steam_match = !criteria.steam_app_ids.is_empty()
        && ["SteamGameId", "SteamAppId"].iter().any(|key| {
            host_env
                .get(*key)
                .map(|v| v.trim())
                .filter(|v| !v.is_empty())
                .is_some_and(|val| criteria.steam_app_ids.iter().any(|id| id == val))
        });

    if steam_match {
        return true;
    }

    if criteria.exe_all.is_empty() && criteria.exe_any.is_empty() {
        return false;
    }

    let all_ok = criteria
        .exe_all
        .iter()
        .all(|sub| exe_lower.contains(&sub.to_ascii_lowercase()));
    let any_ok = criteria.exe_any.is_empty()
        || criteria
            .exe_any
            .iter()
            .any(|sub| exe_lower.contains(&sub.to_ascii_lowercase()));

    all_ok && any_ok
}

pub fn evaluate_rules(
    rules: &[GameRule],
    exe_path: &str,
    host_env: &BTreeMap<String, String>,
) -> RuleEvaluation {
    if host_env
        .get("HYPERX_PROTON_NO_RULES")
        .is_some_and(|v| v == "1")
    {
        return RuleEvaluation::default();
    }

    let exe_lower = exe_path.to_ascii_lowercase();
    let matched: Vec<&GameRule> = rules
        .iter()
        .filter(|r| rule_matches(r, &exe_lower, host_env))
        .collect();

    if matched.is_empty() {
        return RuleEvaluation::default();
    }

    let mut eval = RuleEvaluation::default();
    let mut seen_ensure_rels = HashSet::new();

    for rule in &matched {
        eval.matched_rule_ids.push(rule.id.clone());

        if let Some(id) = &rule.steam_game_id {
            eval.steam_game_id = Some(id.clone());
        }
        for (k, v) in &rule.env {
            eval.env.insert(k.clone(), v.clone());
        }
        if let Some(use_shim) = rule.use_steam_shim {
            eval.use_steam_shim = Some(use_shim);
        }
        for ef in &rule.ensure_files {
            if seen_ensure_rels.insert(ef.create_rel.clone()) {
                eval.ensure_files.push(ef.clone());
            }
        }
    }

    // Later rules take precedence over earlier rules in first-seen-wins DLL override merging.
    for rule in matched.iter().rev() {
        eval.dll_overrides.extend(rule.dll_overrides.iter().cloned());
    }

    eval
}

pub fn should_use_steam_shim(
    eval: &RuleEvaluation,
    host_env: &BTreeMap<String, String>,
    has_steam_shim_file: bool,
) -> bool {
    if !has_steam_shim_file {
        return false;
    }
    if host_env
        .get("HYPERX_PROTON_NO_STEAM_SHIM")
        .is_some_and(|v| v == "1")
    {
        return false;
    }
    if host_env
        .get("HYPERX_PROTON_FORCE_STEAM_SHIM")
        .is_some_and(|v| v == "1")
    {
        return true;
    }
    eval.use_steam_shim.unwrap_or(false)
}

fn is_safe_relative_path(raw: &str) -> Option<&Path> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    let path = Path::new(trimmed);
    if path.is_absolute() {
        return None;
    }
    for comp in path.components() {
        match comp {
            Component::Normal(_) | Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => return None,
        }
    }
    Some(path)
}

pub fn apply_ensure_files(exe_dir: &Path, rules: &[EnsureFileRule]) -> usize {
    let mut created_count = 0usize;
    for rule in rules {
        let Some(if_exists) = is_safe_relative_path(&rule.if_exists_rel) else {
            continue;
        };
        let Some(create_rel) = is_safe_relative_path(&rule.create_rel) else {
            continue;
        };

        if !exe_dir.join(if_exists).exists() {
            continue;
        }

        let target = exe_dir.join(create_rel);
        if target.exists() {
            continue;
        }

        if let Some(parent) = target.parent()
            && fs::create_dir_all(parent).is_err()
        {
            continue;
        }

        if OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&target)
            .is_ok()
        {
            created_count += 1;
        }
    }
    created_count
}
