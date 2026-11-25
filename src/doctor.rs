use crate::config::load_manifest;
use crate::runtime::is_ntsync_available;
use std::path::Path;
use std::process::Command;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CheckSeverity {
    Ok,
    Warn,
    Fail,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DoctorCheck {
    pub name: &'static str,
    pub severity: CheckSeverity,
    pub detail: String,
}

fn find_host_lib(lib_name: &str) -> Option<String> {
    for dir in ["/usr/lib", "/usr/lib64", "/lib", "/lib64"] {
        let candidate = Path::new(dir).join(lib_name);
        if candidate.exists() {
            return Some(candidate.display().to_string());
        }
    }
    None
}

fn check_gst_plugin(plugin: &str) -> Result<(), String> {
    let out = Command::new("gst-inspect-1.0")
        .arg(plugin)
        .output()
        .map_err(|err| format!("gst-inspect-1.0 unavailable: {err}"))?;
    if out.status.success() {
        Ok(())
    } else {
        Err(format!("GStreamer plugin '{plugin}' not found"))
    }
}

/// Runs host and runner-bundle diagnostics for `hyperx-proton doctor`.
pub fn run_doctor(runner_root: &Path) -> Vec<DoctorCheck> {
    let mut checks = Vec::new();

    if is_ntsync_available() {
        checks.push(DoctorCheck {
            name: "ntsync",
            severity: CheckSeverity::Ok,
            detail: "/dev/ntsync is available".to_string(),
        });
    } else {
        checks.push(DoctorCheck {
            name: "ntsync",
            severity: CheckSeverity::Warn,
            detail: "/dev/ntsync unavailable; will fall back to WINEFSYNC=1".to_string(),
        });
    }

    match load_manifest(runner_root) {
        Ok(m) => checks.push(DoctorCheck {
            name: "manifest",
            severity: CheckSeverity::Ok,
            detail: format!("{} ({})", m.name, m.version),
        }),
        Err(err) => checks.push(DoctorCheck {
            name: "manifest",
            severity: CheckSeverity::Fail,
            detail: err,
        }),
    }

    for (name, rel) in [
        ("wine", "files/bin/wine"),
        ("wineserver", "files/bin/wineserver"),
        ("dxvk:d3d11.dll", "files/lib/dxvk/x86_64-windows/d3d11.dll"),
        ("dxvk:dxgi.dll", "files/lib/dxvk/x86_64-windows/dxgi.dll"),
        (
            "vkd3d:d3d12.dll",
            "files/lib/vkd3d-proton/x86_64-windows/d3d12.dll",
        ),
        (
            "vkd3d:d3d12core.dll",
            "files/lib/vkd3d-proton/x86_64-windows/d3d12core.dll",
        ),
    ] {
        let path = runner_root.join(rel);
        if path.is_file() {
            checks.push(DoctorCheck {
                name,
                severity: CheckSeverity::Ok,
                detail: path.display().to_string(),
            });
        } else {
            checks.push(DoctorCheck {
                name,
                severity: CheckSeverity::Fail,
                detail: format!("missing {}", path.display()),
            });
        }
    }

    let nvapi_path = runner_root.join("files/lib/nvapi/x86_64-windows/nvapi64.dll");
    if nvapi_path.is_file() {
        checks.push(DoctorCheck {
            name: "nvapi:nvapi64.dll",
            severity: CheckSeverity::Ok,
            detail: nvapi_path.display().to_string(),
        });
    } else {
        checks.push(DoctorCheck {
            name: "nvapi:nvapi64.dll",
            severity: CheckSeverity::Warn,
            detail: format!("opt-in nvapi64.dll not bundled at {}", nvapi_path.display()),
        });
    }

    for (name, lib) in [
        ("host:vulkan", "libvulkan.so.1"),
        ("host:gnutls", "libgnutls.so.30"),
        ("host:gstreamer", "libgstreamer-1.0.so.0"),
        ("host:fontconfig", "libfontconfig.so.1"),
        ("host:freetype", "libfreetype.so.6"),
    ] {
        match find_host_lib(lib) {
            Some(found) => checks.push(DoctorCheck {
                name,
                severity: CheckSeverity::Ok,
                detail: found,
            }),
            None => checks.push(DoctorCheck {
                name,
                severity: CheckSeverity::Fail,
                detail: format!("missing host library {lib}"),
            }),
        }
    }

    for (name, plugin) in [
        ("gst:uridecodebin", "uridecodebin"),
        ("gst:libav", "libav"),
    ] {
        match check_gst_plugin(plugin) {
            Ok(()) => checks.push(DoctorCheck {
                name,
                severity: CheckSeverity::Ok,
                detail: format!("GStreamer plugin '{plugin}' available"),
            }),
            Err(err) => checks.push(DoctorCheck {
                name,
                severity: CheckSeverity::Warn,
                detail: err,
            }),
        }
    }

    checks
}
