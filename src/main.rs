use hyperx_proton::config::{PrefixLayout, load_manifest, resolve_prefix_layout};
use hyperx_proton::doctor::{CheckSeverity, run_doctor};
use hyperx_proton::prefix::{
    acquire_prefix_lock, prefix_needs_wineboot, prune_foreign_runner_symlinks, sync_prefix_dlls,
};
use hyperx_proton::rules::{
    RuleEvaluation, apply_ensure_files, evaluate_rules, load_rules, should_use_steam_shim,
};
use hyperx_proton::runtime::{
    RuntimeEnv, build_runtime_env_with_sync, is_fsync_available, is_ntsync_available,
};
use std::collections::BTreeMap;
use std::env;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

/// Target soft `RLIMIT_NOFILE` ceiling so `WINEESYNC=1` does not hit `EMFILE`
/// on distros that default to a 1024 soft file-descriptor limit.
const TARGET_NOFILE_SOFT: libc::rlim_t = 524_288;

fn raise_nofile_limit(target_soft: libc::rlim_t) -> Option<libc::rlim_t> {
    let mut lim = libc::rlimit {
        rlim_cur: 0,
        rlim_max: 0,
    };
    if unsafe { libc::getrlimit(libc::RLIMIT_NOFILE, &mut lim) } != 0 {
        return None;
    }

    let desired = target_soft.min(lim.rlim_max);
    if lim.rlim_cur < desired {
        lim.rlim_cur = desired;
        if unsafe { libc::setrlimit(libc::RLIMIT_NOFILE, &lim) } == 0 {
            return Some(desired);
        }
    }

    Some(lim.rlim_cur)
}

fn resolve_runner_root() -> Result<PathBuf, String> {
    if let Ok(override_root) = env::var("HYPERX_PROTON_ROOT")
        && !override_root.trim().is_empty()
    {
        return Ok(PathBuf::from(override_root));
    }

    let exe = env::current_exe()
        .map_err(|err| format!("failed to resolve current executable path: {err}"))?;
    exe.parent()
        .map(Path::to_path_buf)
        .ok_or_else(|| "executable has no parent directory".to_string())
}

fn unix_to_wine_path(unix_path: &str) -> String {
    let normalized = unix_path.replace('/', "\\");
    if normalized.starts_with('\\') {
        format!("Z:{normalized}")
    } else {
        format!("Z:\\{normalized}")
    }
}

fn wine_to_unix_path(wine_path: &str) -> String {
    let trimmed = wine_path.trim();
    let without_drive = trimmed
        .strip_prefix("Z:")
        .or_else(|| trimmed.strip_prefix("z:"))
        .unwrap_or(trimmed);
    without_drive.replace('\\', "/")
}

fn ensure_prefix_ready(
    runner_root: &Path,
    layout: &PrefixLayout,
    manifest_version: &str,
    rt: &RuntimeEnv,
) -> Result<(), String> {
    let _lock = acquire_prefix_lock(&layout.lock_file)?;
    let needs_boot = prefix_needs_wineboot(runner_root, layout, manifest_version);

    if needs_boot {
        // Remove stale symlinks left behind by foreign Proton/Wine builds (e.g. GE-Proton)
        // before wineboot so wineboot/explorer load our matching Wine binaries cleanly.
        prune_foreign_runner_symlinks(runner_root, &layout.wineprefix)?;

        let boot_status = Command::new(&rt.wine_bin)
            .args(["wineboot", "-u"])
            .envs(&rt.env)
            .status()
            .map_err(|err| format!("failed to run wineboot: {err}"))?;
        if !boot_status.success() {
            return Err(format!(
                "wineboot -u exited with status {:?}",
                boot_status.code()
            ));
        }

        let wait_status = Command::new(&rt.wineserver_bin)
            .arg("-w")
            .envs(&rt.env)
            .status()
            .map_err(|err| format!("failed to run wineserver -w after wineboot: {err}"))?;
        if !wait_status.success() {
            return Err(format!(
                "wineserver -w after wineboot exited with status {:?}",
                wait_status.code()
            ));
        }
    }

    // Copy DX11/DX12 (and opt-in NVAPI) DLLs AFTER wineboot so wineboot cannot
    // overwrite them with Wine builtin stubs, and stamp version only on success.
    let _report = sync_prefix_dlls(
        runner_root,
        layout,
        manifest_version,
        rt.nvapi_enabled,
    )?;

    for key in [
        "DXVK_SHADER_CACHE_PATH",
        "DXVK_STATE_CACHE_PATH",
        "VKD3D_SHADER_CACHE_PATH",
        "__GL_SHADER_DISK_CACHE_PATH",
        "MESA_SHADER_CACHE_DIR",
    ] {
        if let Some(dir) = rt.env.get(key).filter(|s| !s.is_empty()) {
            let _ = std::fs::create_dir_all(dir);
        }
    }

    Ok(())
}

fn run_in_prefix(runner_root: &Path, args: &[String]) -> Result<i32, String> {
    let exe = args
        .first()
        .ok_or_else(|| "missing target executable argument".to_string())?;
    let exe_args = &args[1..];

    let manifest = load_manifest(runner_root)?;
    let compat_env = env::var_os("STEAM_COMPAT_DATA_PATH").map(PathBuf::from);
    let prefix_env = env::var_os("WINEPREFIX").map(PathBuf::from);
    let layout = resolve_prefix_layout(compat_env.as_deref(), prefix_env.as_deref())?;

    let host_env: BTreeMap<String, String> = env::vars().collect();
    let rt = build_runtime_env_with_sync(
        runner_root,
        &layout,
        &host_env,
        is_ntsync_available(),
        is_fsync_available(),
        &RuleEvaluation::default(),
    );

    let _ = raise_nofile_limit(TARGET_NOFILE_SOFT);

    ensure_prefix_ready(runner_root, &layout, &manifest.version, &rt)?;

    let status = Command::new(&rt.wine_bin)
        .arg(exe)
        .args(exe_args)
        .envs(&rt.env)
        .status()
        .map_err(|err| format!("failed to execute {}: {err}", rt.wine_bin.display()))?;

    Ok(status.code().unwrap_or(1))
}

fn run_launch(runner_root: &Path, wait_for_exit: bool, args: &[String]) -> Result<i32, String> {
    let exe = args
        .first()
        .ok_or_else(|| "missing target executable argument".to_string())?;
    let exe_args = &args[1..];

    let manifest = load_manifest(runner_root)?;
    let compat_env = env::var_os("STEAM_COMPAT_DATA_PATH").map(PathBuf::from);
    let prefix_env = env::var_os("WINEPREFIX").map(PathBuf::from);
    let layout = resolve_prefix_layout(compat_env.as_deref(), prefix_env.as_deref())?;

    let host_env: BTreeMap<String, String> = env::vars().collect();
    let loaded_rules = load_rules(&host_env);
    for warn in &loaded_rules.warnings {
        eprintln!("hyperx-proton: warning: {warn}");
    }
    let rule_eval = evaluate_rules(&loaded_rules.rules, exe, &host_env);

    let rt = build_runtime_env_with_sync(
        runner_root,
        &layout,
        &host_env,
        is_ntsync_available(),
        is_fsync_available(),
        &rule_eval,
    );

    let _ = raise_nofile_limit(TARGET_NOFILE_SOFT);

    ensure_prefix_ready(runner_root, &layout, &manifest.version, &rt)?;

    if let Some(exe_dir) = Path::new(exe).parent() {
        let _ = apply_ensure_files(exe_dir, &rule_eval.ensure_files);
    }

    let launch_env = &rt.env;
    let has_steam_shim = layout
        .wineprefix
        .join("drive_c/windows/system32/steam.exe")
        .is_file();

    let mut cmd = Command::new(&rt.wine_bin);
    if should_use_steam_shim(&rule_eval, launch_env, has_steam_shim) {
        cmd.arg("C:\\windows\\system32\\steam.exe");
    }
    let mut child = cmd
        .arg(exe)
        .args(exe_args)
        .envs(launch_env)
        .spawn()
        .map_err(|err| format!("failed to execute {}: {err}", rt.wine_bin.display()))?;

    let status = child
        .wait()
        .map_err(|err| format!("failed to wait for {}: {err}", rt.wine_bin.display()))?;

    if wait_for_exit {
        let _ = Command::new(&rt.wineserver_bin)
            .arg("-w")
            .envs(&rt.env)
            .status();
    }

    Ok(status.code().unwrap_or(1))
}

fn exit_code_from_i32(code: i32) -> ExitCode {
    ExitCode::from((code & 0xff) as u8)
}

fn main() -> ExitCode {
    let args: Vec<String> = env::args().skip(1).collect();
    let Some(verb) = args.first().map(String::as_str) else {
        eprintln!(
            "Usage: proton <waitforexitandrun|run|runinprefix|doctor|version|getcompatpath|getnativepath> [args...]"
        );
        return ExitCode::from(2);
    };

    let runner_root = match resolve_runner_root() {
        Ok(p) => p,
        Err(err) => {
            eprintln!("hyperx-proton: {err}");
            return ExitCode::from(1);
        }
    };

    match verb {
        "version" => match load_manifest(&runner_root) {
            Ok(m) => {
                println!("{}", m.version);
                ExitCode::SUCCESS
            }
            Err(err) => {
                eprintln!("hyperx-proton: {err}");
                ExitCode::from(1)
            }
        },
        "doctor" => {
            let checks = run_doctor(&runner_root);
            let mut failed = false;
            for c in checks {
                let tag = match c.severity {
                    CheckSeverity::Ok => "OK",
                    CheckSeverity::Warn => "WARN",
                    CheckSeverity::Fail => {
                        failed = true;
                        "FAIL"
                    }
                };
                println!("[{tag}] {}: {}", c.name, c.detail);
            }
            if failed {
                ExitCode::from(1)
            } else {
                ExitCode::SUCCESS
            }
        }
        "getcompatpath" => match args.get(1) {
            Some(path) => {
                println!("{}", unix_to_wine_path(path));
                ExitCode::SUCCESS
            }
            None => {
                eprintln!("Usage: proton getcompatpath <unix-path>");
                ExitCode::from(2)
            }
        },
        "getnativepath" => match args.get(1) {
            Some(path) => {
                println!("{}", wine_to_unix_path(path));
                ExitCode::SUCCESS
            }
            None => {
                eprintln!("Usage: proton getnativepath <wine-path>");
                ExitCode::from(2)
            }
        },
        "waitforexitandrun" => match run_launch(&runner_root, true, &args[1..]) {
            Ok(code) => exit_code_from_i32(code),
            Err(err) => {
                eprintln!("hyperx-proton: {err}");
                ExitCode::from(1)
            }
        },
        "run" => match run_launch(&runner_root, false, &args[1..]) {
            Ok(code) => exit_code_from_i32(code),
            Err(err) => {
                eprintln!("hyperx-proton: {err}");
                ExitCode::from(1)
            }
        },
        "runinprefix" => match run_in_prefix(&runner_root, &args[1..]) {
            Ok(code) => exit_code_from_i32(code),
            Err(err) => {
                eprintln!("hyperx-proton: {err}");
                ExitCode::from(1)
            }
        },
        other => {
            eprintln!("hyperx-proton: unknown verb '{other}'");
            ExitCode::from(2)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exit_code_from_i32_preserves_low_8_bits() {
        assert_eq!(exit_code_from_i32(0), ExitCode::from(0));
        assert_eq!(exit_code_from_i32(258), ExitCode::from(2));
        assert_eq!(exit_code_from_i32(-1), ExitCode::from(255));
    }
}
