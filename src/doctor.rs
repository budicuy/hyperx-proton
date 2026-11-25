use crate::config::load_manifest;
use crate::gpu::{GpuProbeResult, choose_dxvk_backend, is_sarek_bundled, probe_primary_gpu};
use crate::rules::load_rules;
use crate::runtime::{SyncMode, is_fsync_available, is_ntsync_available, resolve_sync_mode};
use crate::supervisor::is_pidfd_supported;
use crate::topology::{compute_shader_compiler_threads, detect_physical_cores};
use std::collections::BTreeMap;
use std::ffi::{CStr, CString};
use std::path::Path;
use std::process::Command;

pub const MIN_GLIBC_VERSION: (u32, u32) = (2, 38);

#[repr(C)]
struct LinkMap {
    l_addr: libc::uintptr_t,
    l_name: *const libc::c_char,
}

unsafe extern "C" {
    fn dlinfo(
        handle: *mut libc::c_void,
        request: libc::c_int,
        info: *mut libc::c_void,
    ) -> libc::c_int;
    fn gnu_get_libc_version() -> *const libc::c_char;
}

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

/// Parses a `major.minor[.patch]` glibc version string into `(major, minor)`.
pub fn parse_glibc_version(raw: &str) -> Option<(u32, u32)> {
    let mut parts = raw.trim().split('.');
    let major = parts.next()?.parse::<u32>().ok()?;
    let minor = parts.next()?.parse::<u32>().ok()?;
    Some((major, minor))
}

/// Evaluates whether the host glibc version meets `MIN_GLIBC_VERSION` (`2.38`).
pub fn check_glibc_floor(raw: &str) -> DoctorCheck {
    match parse_glibc_version(raw) {
        Some(ver) if ver >= MIN_GLIBC_VERSION => DoctorCheck {
            name: "host:glibc",
            severity: CheckSeverity::Ok,
            detail: format!(
                "glibc {raw} (>= {}.{})",
                MIN_GLIBC_VERSION.0, MIN_GLIBC_VERSION.1
            ),
        },
        Some(_) => DoctorCheck {
            name: "host:glibc",
            severity: CheckSeverity::Fail,
            detail: format!(
                "glibc {raw} is too old; Wine requires glibc >= {}.{}",
                MIN_GLIBC_VERSION.0, MIN_GLIBC_VERSION.1
            ),
        },
        None => DoctorCheck {
            name: "host:glibc",
            severity: CheckSeverity::Warn,
            detail: format!("could not parse host glibc version '{raw}'"),
        },
    }
}

/// Resolves a host shared library across any Linux distro's dynamic linker paths
/// using `dlopen` + `dlinfo(RTLD_DI_LINKMAP)`.
pub fn find_host_lib(lib_name: &str) -> Option<String> {
    const RTLD_DI_LINKMAP: libc::c_int = 2;
    let c_name = CString::new(lib_name).ok()?;
    let handle = unsafe { libc::dlopen(c_name.as_ptr(), libc::RTLD_LAZY | libc::RTLD_LOCAL) };
    if handle.is_null() {
        return None;
    }

    let mut lm: *const LinkMap = std::ptr::null();
    let rc = unsafe {
        dlinfo(
            handle,
            RTLD_DI_LINKMAP,
            (&mut lm as *mut *const LinkMap).cast::<libc::c_void>(),
        )
    };
    let resolved = if rc == 0 && !lm.is_null() {
        let l_name = unsafe { (*lm).l_name };
        if !l_name.is_null() {
            let path = unsafe { CStr::from_ptr(l_name) }
                .to_string_lossy()
                .into_owned();
            if path.is_empty() {
                lib_name.to_string()
            } else {
                path
            }
        } else {
            lib_name.to_string()
        }
    } else {
        lib_name.to_string()
    };

    unsafe {
        libc::dlclose(handle);
    }
    Some(resolved)
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

    let ntsync_avail = is_ntsync_available();
    let fsync_avail = is_fsync_available();
    let active_sync = resolve_sync_mode(ntsync_avail, fsync_avail, false, false, false);
    checks.push(DoctorCheck {
        name: "sync:tier",
        severity: match active_sync {
            SyncMode::Ntsync | SyncMode::Fsync => CheckSeverity::Ok,
            SyncMode::Esync | SyncMode::Plain => CheckSeverity::Warn,
        },
        detail: format!(
            "active={} (ntsync={}, fsync={})",
            active_sync.as_str(),
            ntsync_avail,
            fsync_avail
        ),
    });

    let glibc_raw = unsafe {
        let ptr = gnu_get_libc_version();
        if ptr.is_null() {
            String::new()
        } else {
            CStr::from_ptr(ptr).to_string_lossy().into_owned()
        }
    };
    checks.push(check_glibc_floor(&glibc_raw));

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
