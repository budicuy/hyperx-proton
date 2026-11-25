use crate::config::load_manifest;
use crate::gpu::{GpuProbeResult, choose_dxvk_backend, is_sarek_bundled, probe_primary_gpu};
use crate::rules::load_rules;
use crate::runtime::is_ntsync_available;
use crate::supervisor::is_pidfd_supported;
use crate::topology::{compute_shader_compiler_threads, detect_physical_cores};
use std::collections::BTreeMap;
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

    let phys_cores = detect_physical_cores();
    let shader_threads = compute_shader_compiler_threads(phys_cores);
    checks.push(DoctorCheck {
        name: "cpu:topology",
        severity: CheckSeverity::Ok,
        detail: format!("{phys_cores} physical cores -> {shader_threads} shader compiler threads"),
    });

    if is_pidfd_supported() {
        checks.push(DoctorCheck {
            name: "kernel:pidfd",
            severity: CheckSeverity::Ok,
            detail: "pidfd_open event-driven thread supervisor active".to_string(),
        });
    } else {
        checks.push(DoctorCheck {
            name: "kernel:pidfd",
            severity: CheckSeverity::Warn,
            detail: "pidfd_open unavailable; will fall back to standard child wait".to_string(),
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
        ("wine:i386-unix/wine64", "files/lib/wine/i386-unix/wine64"),
        (
            "wine:i386-unix/wine64-preloader",
            "files/lib/wine/i386-unix/wine64-preloader",
        ),
        ("shim:steam.exe", "files/lib/wine/x86_64-windows/steam.exe"),
        (
            "icu:x86_64/icuuc68.dll",
            "files/lib/wine/icu/x86_64-windows/icuuc68.dll",
        ),
        (
            "icu:x86_64/icuin68.dll",
            "files/lib/wine/icu/x86_64-windows/icuin68.dll",
        ),
        (
            "icu:x86_64/icudt68.dll",
            "files/lib/wine/icu/x86_64-windows/icudt68.dll",
        ),
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

    for (name, rel) in [
        (
            "dxvk-sarek:d3d11.dll",
            "files/lib/dxvk-sarek/x86_64-windows/d3d11.dll",
        ),
        (
            "dxvk-sarek:dxgi.dll",
            "files/lib/dxvk-sarek/x86_64-windows/dxgi.dll",
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
                severity: CheckSeverity::Warn,
                detail: format!("optional dxvk-sarek fallback missing at {}", path.display()),
            });
        }
    }

    let host_env: BTreeMap<String, String> = std::env::vars().collect();
    let gpu_probe = probe_primary_gpu(&host_env);
    match &gpu_probe {
        GpuProbeResult::Detected(gpu) => checks.push(DoctorCheck {
            name: "gpu:vulkan",
            severity: CheckSeverity::Ok,
            detail: format!(
                "{} [{}, {}, Vulkan {}.{}.{}, GPL={}, present_wait={}]",
                gpu.device_name,
                gpu.vendor.as_str(),
                gpu.device_type.as_str(),
                gpu.api_major,
                gpu.api_minor,
                gpu.api_patch,
                if gpu.has_gpl { "yes" } else { "no" },
                if gpu.has_present_wait { "yes" } else { "no" },
            ),
        }),
        GpuProbeResult::Unavailable(reason) => checks.push(DoctorCheck {
            name: "gpu:vulkan",
            severity: CheckSeverity::Warn,
            detail: reason.clone(),
        }),
    }

    let sarek_bundled = is_sarek_bundled(runner_root);
    let dxvk_sel = choose_dxvk_backend(
        &gpu_probe,
        host_env.get("HYPERX_PROTON_DXVK_BACKEND").map(String::as_str),
        None,
        sarek_bundled,
    );
    checks.push(DoctorCheck {
        name: "gpu:dxvk-backend",
        severity: if dxvk_sel.warning.is_some() {
            CheckSeverity::Warn
        } else {
            CheckSeverity::Ok
        },
        detail: format!("{} ({})", dxvk_sel.backend.as_str(), dxvk_sel.reason),
    });

    let loaded_rules = load_rules(&host_env);
    if loaded_rules.warnings.is_empty() {
        checks.push(DoctorCheck {
            name: "rules:engine",
            severity: CheckSeverity::Ok,
            detail: format!(
                "{} built-in + {} custom rules loaded",
                loaded_rules.builtin_count, loaded_rules.custom_count
            ),
        });
    } else {
        checks.push(DoctorCheck {
            name: "rules:engine",
            severity: CheckSeverity::Warn,
            detail: format!(
                "{} built-in + {} custom rules loaded ({} warning(s): {})",
                loaded_rules.builtin_count,
                loaded_rules.custom_count,
                loaded_rules.warnings.len(),
                loaded_rules.warnings.join("; ")
            ),
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
