use hyperx_proton::config::{load_manifest, resolve_prefix_layout};
use hyperx_proton::doctor::{CheckSeverity, run_doctor};
use hyperx_proton::prefix::{acquire_prefix_lock, sync_prefix_dlls};
use hyperx_proton::runtime::{build_runtime_env, is_ntsync_available};
use std::collections::BTreeMap;
use std::env;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

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
    let rt = build_runtime_env(runner_root, &layout, &host_env, is_ntsync_available());

    {
        let _lock = acquire_prefix_lock(&layout.lock_file)?;
        let report =
            sync_prefix_dlls(runner_root, &layout, &manifest.version, rt.nvapi_enabled)?;

        if report.needs_wineboot {
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
    }

    let status = Command::new(&rt.wine_bin)
        .arg(exe)
        .args(exe_args)
        .envs(&rt.env)
        .status()
        .map_err(|err| format!("failed to execute {}: {err}", rt.wine_bin.display()))?;

    if wait_for_exit {
        let _ = Command::new(&rt.wineserver_bin)
            .arg("-w")
            .envs(&rt.env)
            .status();
    }

    Ok(status.code().unwrap_or(1))
}

fn main() -> ExitCode {
    let args: Vec<String> = env::args().skip(1).collect();
    let Some(verb) = args.first().map(String::as_str) else {
        eprintln!(
            "Usage: proton <waitforexitandrun|run|doctor|version|getcompatpath|getnativepath> [args...]"
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
            Ok(code) => ExitCode::from(u8::try_from(code).unwrap_or(1)),
            Err(err) => {
                eprintln!("hyperx-proton: {err}");
                ExitCode::from(1)
            }
        },
        "run" => match run_launch(&runner_root, false, &args[1..]) {
            Ok(code) => ExitCode::from(u8::try_from(code).unwrap_or(1)),
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
