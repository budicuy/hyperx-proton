use std::collections::BTreeMap;
use std::ffi::{CStr, c_char, c_void};
use std::path::Path;
use std::ptr;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GpuVendor {
    Nvidia,
    Amd,
    Intel,
    Other(u32),
}

impl GpuVendor {
    pub fn from_vendor_id(vendor_id: u32) -> Self {
        match vendor_id {
            0x10DE => Self::Nvidia,
            0x1002 => Self::Amd,
            0x8086 => Self::Intel,
            other => Self::Other(other),
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Nvidia => "NVIDIA",
            Self::Amd => "AMD",
            Self::Intel => "Intel",
            Self::Other(_) => "Other",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GpuDeviceType {
    Discrete,
    Integrated,
    Virtual,
    Cpu,
    Other,
}

impl GpuDeviceType {
    pub fn from_vk_type(vk_type: u32) -> Self {
        match vk_type {
            1 => Self::Integrated,
            2 => Self::Discrete,
            3 => Self::Virtual,
            4 => Self::Cpu,
            _ => Self::Other,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Discrete => "Discrete",
            Self::Integrated => "Integrated",
            Self::Virtual => "Virtual",
            Self::Cpu => "Cpu",
            Self::Other => "Other",
        }
    }

    fn priority_score(self) -> u32 {
        match self {
            Self::Discrete => 300,
            Self::Integrated => 200,
            Self::Virtual => 100,
            Self::Other => 50,
            Self::Cpu => 10,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GpuProfile {
    pub vendor: GpuVendor,
    pub vendor_id: u32,
    pub device_id: u32,
    pub device_name: String,
    pub device_type: GpuDeviceType,
    pub api_major: u32,
    pub api_minor: u32,
    pub api_patch: u32,
    pub has_gpl: bool,
    pub has_present_wait: bool,
    pub has_shader_module_id: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GpuProbeResult {
    Detected(GpuProfile),
    Unavailable(String),
}

impl GpuProbeResult {
    pub fn vendor(&self) -> Option<GpuVendor> {
        match self {
            Self::Detected(profile) => Some(profile.vendor),
            Self::Unavailable(_) => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DxvkBackend {
    Standard,
    Sarek,
}

impl DxvkBackend {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Standard => "standard",
            Self::Sarek => "sarek",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "standard" => Some(Self::Standard),
            "sarek" => Some(Self::Sarek),
            _ => None,
        }
    }

    pub fn rel_dll_dir(&self) -> &'static str {
        match self {
            Self::Standard => "files/lib/dxvk/x86_64-windows",
            Self::Sarek => "files/lib/dxvk-sarek/x86_64-windows",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DxvkSelection {
    pub backend: DxvkBackend,
    pub reason: String,
    pub warning: Option<String>,
}

pub fn is_sarek_bundled(runner_root: &Path) -> bool {
    let dir = runner_root.join(DxvkBackend::Sarek.rel_dll_dir());
    dir.join("d3d11.dll").is_file() && dir.join("dxgi.dll").is_file()
}

fn parse_mesa_device_select(raw: &str) -> Option<(u32, u32)> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    let prefix = trimmed.split(['!', ',']).next()?.trim();
    let (vendor_hex, device_hex) = prefix.split_once(':')?;
    let vendor_clean = vendor_hex.trim().trim_start_matches("0x").trim_start_matches("0X");
    let device_clean = device_hex.trim().trim_start_matches("0x").trim_start_matches("0X");
    let vendor_id = u32::from_str_radix(vendor_clean, 16).ok()?;
    let device_id = u32::from_str_radix(device_clean, 16).ok()?;
    Some((vendor_id, device_id))
}

pub fn select_primary_gpu(
    devices: &[GpuProfile],
    filter_name: Option<&str>,
    mesa_select: Option<&str>,
) -> Option<GpuProfile> {
    if devices.is_empty() {
        return None;
    }

    if let Some(needle) = filter_name.map(str::trim).filter(|s| !s.is_empty()) {
        let needle_lower = needle.to_ascii_lowercase();
        if let Some(matched) = devices
            .iter()
            .find(|d| d.device_name.to_ascii_lowercase().contains(&needle_lower))
        {
            return Some(matched.clone());
        }
    }

    if let Some((vendor_id, device_id)) = mesa_select.and_then(parse_mesa_device_select)
        && let Some(matched) = devices
            .iter()
            .find(|d| d.vendor_id == vendor_id && d.device_id == device_id)
    {
        return Some(matched.clone());
    }

    devices
        .iter()
        .enumerate()
        .max_by_key(|(idx, d)| {
            (
                d.device_type.priority_score(),
                d.api_major,
                d.api_minor,
                d.api_patch,
                std::cmp::Reverse(*idx),
            )
        })
        .map(|(_, d)| d.clone())
}

pub fn choose_dxvk_backend(
    probe: &GpuProbeResult,
    env_override: Option<&str>,
    rule_override: Option<DxvkBackend>,
    sarek_bundled: bool,
) -> DxvkSelection {
    if let Some(raw_env) = env_override.map(str::trim).filter(|s| !s.is_empty())
        && let Some(requested) = DxvkBackend::parse(raw_env)
    {
        return match requested {
            DxvkBackend::Standard => DxvkSelection {
                backend: DxvkBackend::Standard,
                reason: "HYPERX_PROTON_DXVK_BACKEND=standard".to_string(),
                warning: None,
            },
            DxvkBackend::Sarek => {
                if sarek_bundled {
                    DxvkSelection {
                        backend: DxvkBackend::Sarek,
                        reason: "HYPERX_PROTON_DXVK_BACKEND=sarek".to_string(),
                        warning: None,
                    }
                } else {
                    DxvkSelection {
                        backend: DxvkBackend::Standard,
                        reason: "fallback to standard (dxvk-sarek not bundled)".to_string(),
                        warning: Some(
                            "HYPERX_PROTON_DXVK_BACKEND=sarek requested, but dxvk-sarek bundle is missing; falling back to standard DXVK".to_string(),
                        ),
                    }
                }
            }
        };
    }

    if let Some(rule_backend) = rule_override {
        match rule_backend {
            DxvkBackend::Standard => {
                return DxvkSelection {
                    backend: DxvkBackend::Standard,
                    reason: "rule override (standard)".to_string(),
                    warning: None,
                };
            }
            DxvkBackend::Sarek => {
                if sarek_bundled {
                    return DxvkSelection {
                        backend: DxvkBackend::Sarek,
                        reason: "rule override (sarek)".to_string(),
                        warning: None,
                    };
                }
                return DxvkSelection {
                    backend: DxvkBackend::Standard,
                    reason: "fallback to standard (rule requested sarek, bundle missing)".to_string(),
                    warning: Some(
                        "Game rule requested dxvk-sarek, but dxvk-sarek bundle is missing; falling back to standard DXVK".to_string(),
                    ),
                };
            }
        }
    }

    match probe {
        GpuProbeResult::Detected(gpu) if (gpu.api_major, gpu.api_minor) < (1, 3) => {
            if sarek_bundled {
                DxvkSelection {
                    backend: DxvkBackend::Sarek,
                    reason: format!(
                        "auto-selected sarek for Vulkan {}.{}",
                        gpu.api_major, gpu.api_minor
                    ),
                    warning: None,
                }
            } else {
                DxvkSelection {
                    backend: DxvkBackend::Standard,
                    reason: format!(
                        "fallback to standard for Vulkan {}.{} (dxvk-sarek missing)",
                        gpu.api_major, gpu.api_minor
                    ),
                    warning: Some(format!(
                        "GPU '{}' reports Vulkan {}.{} (< 1.3), but dxvk-sarek bundle is missing; falling back to standard DXVK",
                        gpu.device_name, gpu.api_major, gpu.api_minor
                    )),
                }
            }
        }
        GpuProbeResult::Detected(gpu) => DxvkSelection {
            backend: DxvkBackend::Standard,
            reason: format!("auto-selected standard for Vulkan {}.{}", gpu.api_major, gpu.api_minor),
            warning: None,
        },
        GpuProbeResult::Unavailable(_) => DxvkSelection {
            backend: DxvkBackend::Standard,
            reason: "default standard (GPU probe unavailable)".to_string(),
            warning: None,
        },
    }
}

// --- Vulkan C ABI bindings (zero external crate) ---

type VkInstance = *mut c_void;
type VkPhysicalDevice = *mut c_void;
type VkResult = i32;

const VK_SUCCESS: VkResult = 0;
const VK_ERROR_INCOMPATIBLE_DRIVER: VkResult = -9;
const VK_STRUCTURE_TYPE_APPLICATION_INFO: u32 = 0;
const VK_STRUCTURE_TYPE_INSTANCE_CREATE_INFO: u32 = 1;

const fn vk_make_api_version(variant: u32, major: u32, minor: u32, patch: u32) -> u32 {
    (variant << 29) | (major << 22) | (minor << 12) | patch
}

#[repr(C)]
struct VkApplicationInfo {
    s_type: u32,
    p_next: *const c_void,
    p_application_name: *const c_char,
    application_version: u32,
    p_engine_name: *const c_char,
    engine_version: u32,
    api_version: u32,
}

#[repr(C)]
struct VkInstanceCreateInfo {
    s_type: u32,
    p_next: *const c_void,
    flags: u32,
    p_application_info: *const VkApplicationInfo,
    enabled_layer_count: u32,
    pp_enabled_layer_names: *const *const c_char,
    enabled_extension_count: u32,
    pp_enabled_extension_names: *const *const c_char,
}

#[repr(C, align(8))]
struct VkPhysicalDeviceProperties {
    api_version: u32,
    driver_version: u32,
    vendor_id: u32,
    device_id: u32,
    device_type: u32,
    device_name: [u8; 256],
    _tail: [u8; 548],
}

#[repr(C)]
#[derive(Clone, Copy)]
struct VkExtensionProperties {
    extension_name: [u8; 256],
    spec_version: u32,
}

type PfnVkCreateInstance = unsafe extern "C" fn(
    *const VkInstanceCreateInfo,
    *const c_void,
    *mut VkInstance,
) -> VkResult;
type PfnVkDestroyInstance = unsafe extern "C" fn(VkInstance, *const c_void);
type PfnVkEnumeratePhysicalDevices = unsafe extern "C" fn(
    VkInstance,
    *mut u32,
    *mut VkPhysicalDevice,
) -> VkResult;
type PfnVkGetPhysicalDeviceProperties =
    unsafe extern "C" fn(VkPhysicalDevice, *mut VkPhysicalDeviceProperties);
type PfnVkEnumerateDeviceExtensionProperties = unsafe extern "C" fn(
    VkPhysicalDevice,
    *const c_char,
    *mut u32,
    *mut VkExtensionProperties,
) -> VkResult;

struct VulkanLibGuard(*mut c_void);

impl Drop for VulkanLibGuard {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe {
                libc::dlclose(self.0);
            }
        }
    }
}

struct VkInstanceGuard {
    instance: VkInstance,
    destroy_fn: PfnVkDestroyInstance,
}

impl Drop for VkInstanceGuard {
    fn drop(&mut self) {
        if !self.instance.is_null() {
            unsafe {
                (self.destroy_fn)(self.instance, ptr::null());
            }
        }
    }
}

unsafe fn load_symbol<T>(lib: *mut c_void, name: &CStr) -> Option<T> {
    let ptr = unsafe { libc::dlsym(lib, name.as_ptr()) };
    if ptr.is_null() {
        None
    } else {
        Some(unsafe { std::mem::transmute_copy(&ptr) })
    }
}

fn cstr_from_fixed_bytes(buf: &[u8]) -> String {
    let len = buf.iter().position(|&b| b == 0).unwrap_or(buf.len());
    String::from_utf8_lossy(&buf[..len]).into_owned()
}

pub fn probe_primary_gpu(host_env: &BTreeMap<String, String>) -> GpuProbeResult {
    if host_env
        .get("HYPERX_PROTON_NO_GPU_PROBE")
        .is_some_and(|v| v == "1")
    {
        return GpuProbeResult::Unavailable("disabled by HYPERX_PROTON_NO_GPU_PROBE=1".to_string());
    }

    let lib_handle = unsafe { libc::dlopen(c"libvulkan.so.1".as_ptr(), libc::RTLD_LAZY | libc::RTLD_LOCAL) };
    if lib_handle.is_null() {
        return GpuProbeResult::Unavailable("libvulkan.so.1 not available".to_string());
    }
    let _lib_guard = VulkanLibGuard(lib_handle);

    let (
        vk_create_instance,
        vk_destroy_instance,
        vk_enum_devices,
        vk_get_props,
        vk_enum_exts,
    ) = unsafe {
        let Some(f1) = load_symbol::<PfnVkCreateInstance>(lib_handle, c"vkCreateInstance") else {
            return GpuProbeResult::Unavailable("missing vkCreateInstance".to_string());
        };
        let Some(f2) = load_symbol::<PfnVkDestroyInstance>(lib_handle, c"vkDestroyInstance") else {
            return GpuProbeResult::Unavailable("missing vkDestroyInstance".to_string());
        };
        let Some(f3) = load_symbol::<PfnVkEnumeratePhysicalDevices>(
            lib_handle,
            c"vkEnumeratePhysicalDevices",
        ) else {
            return GpuProbeResult::Unavailable("missing vkEnumeratePhysicalDevices".to_string());
        };
        let Some(f4) = load_symbol::<PfnVkGetPhysicalDeviceProperties>(
            lib_handle,
            c"vkGetPhysicalDeviceProperties",
        ) else {
            return GpuProbeResult::Unavailable("missing vkGetPhysicalDeviceProperties".to_string());
        };
        let Some(f5) = load_symbol::<PfnVkEnumerateDeviceExtensionProperties>(
            lib_handle,
            c"vkEnumerateDeviceExtensionProperties",
        ) else {
            return GpuProbeResult::Unavailable(
                "missing vkEnumerateDeviceExtensionProperties".to_string(),
            );
        };
        (f1, f2, f3, f4, f5)
    };

    let app_info = VkApplicationInfo {
        s_type: VK_STRUCTURE_TYPE_APPLICATION_INFO,
        p_next: ptr::null(),
        p_application_name: c"HyperX-Proton".as_ptr(),
        application_version: 1,
        p_engine_name: c"HyperX-Proton".as_ptr(),
        engine_version: 1,
        api_version: vk_make_api_version(0, 1, 1, 0),
    };
    let create_info = VkInstanceCreateInfo {
        s_type: VK_STRUCTURE_TYPE_INSTANCE_CREATE_INFO,
        p_next: ptr::null(),
        flags: 0,
        p_application_info: &app_info,
        enabled_layer_count: 0,
        pp_enabled_layer_names: ptr::null(),
        enabled_extension_count: 0,
        pp_enabled_extension_names: ptr::null(),
    };

    let mut instance: VkInstance = ptr::null_mut();
    let mut res = unsafe { vk_create_instance(&create_info, ptr::null(), &mut instance) };
    if res == VK_ERROR_INCOMPATIBLE_DRIVER {
        let app_info_10 = VkApplicationInfo {
            api_version: vk_make_api_version(0, 1, 0, 0),
            ..app_info
        };
        let create_info_10 = VkInstanceCreateInfo {
            p_application_info: &app_info_10,
            ..create_info
        };
        res = unsafe { vk_create_instance(&create_info_10, ptr::null(), &mut instance) };
    }
    if res != VK_SUCCESS || instance.is_null() {
        return GpuProbeResult::Unavailable(format!("vkCreateInstance failed with code {res}"));
    }
    let _instance_guard = VkInstanceGuard {
        instance,
        destroy_fn: vk_destroy_instance,
    };

    let mut device_count: u32 = 0;
    let res = unsafe { vk_enum_devices(instance, &mut device_count, ptr::null_mut()) };
    if res != VK_SUCCESS || device_count == 0 {
        return GpuProbeResult::Unavailable("no Vulkan physical devices found".to_string());
    }

    let mut raw_devices = vec![ptr::null_mut(); device_count as usize];
    let res = unsafe { vk_enum_devices(instance, &mut device_count, raw_devices.as_mut_ptr()) };
    if res != VK_SUCCESS && res != 1 {
        // VK_INCOMPLETE == 1 is acceptable if device_count > 0
        return GpuProbeResult::Unavailable(format!(
            "vkEnumeratePhysicalDevices failed with code {res}"
        ));
    }
    raw_devices.truncate(device_count as usize);

    let mut profiles = Vec::with_capacity(raw_devices.len());
    for dev in raw_devices {
        if dev.is_null() {
            continue;
        }
        let mut props: VkPhysicalDeviceProperties = unsafe { std::mem::zeroed() };
        unsafe { vk_get_props(dev, &mut props) };

        let mut ext_count: u32 = 0;
        let ext_res = unsafe { vk_enum_exts(dev, ptr::null(), &mut ext_count, ptr::null_mut()) };
        let mut has_gpl = false;
        let mut has_present_wait = false;
        let mut has_shader_module_id = false;
        if ext_res == VK_SUCCESS && ext_count > 0 {
            let mut exts: Vec<VkExtensionProperties> =
                vec![unsafe { std::mem::zeroed() }; ext_count as usize];
            let ext_res2 =
                unsafe { vk_enum_exts(dev, ptr::null(), &mut ext_count, exts.as_mut_ptr()) };
            if ext_res2 == VK_SUCCESS || ext_res2 == 1 {
                exts.truncate(ext_count as usize);
                for ext in &exts {
                    match cstr_from_fixed_bytes(&ext.extension_name).as_str() {
                        "VK_EXT_graphics_pipeline_library" => has_gpl = true,
                        "VK_KHR_present_wait" => has_present_wait = true,
                        "VK_EXT_shader_module_identifier" => has_shader_module_id = true,
                        _ => {}
                    }
                }
            }
        }

        let api = props.api_version;
        profiles.push(GpuProfile {
            vendor: GpuVendor::from_vendor_id(props.vendor_id),
            vendor_id: props.vendor_id,
            device_id: props.device_id,
            device_name: cstr_from_fixed_bytes(&props.device_name),
            device_type: GpuDeviceType::from_vk_type(props.device_type),
            api_major: (api >> 22) & 0x7f,
            api_minor: (api >> 12) & 0x3ff,
            api_patch: api & 0xfff,
            has_gpl,
            has_present_wait,
            has_shader_module_id,
        });
    }

    let filter_name = host_env.get("DXVK_FILTER_DEVICE_NAME").map(String::as_str);
    let mesa_select = host_env.get("MESA_VK_DEVICE_SELECT").map(String::as_str);

    match select_primary_gpu(&profiles, filter_name, mesa_select) {
        Some(gpu) => GpuProbeResult::Detected(gpu),
        None => GpuProbeResult::Unavailable("no usable Vulkan GPU profile".to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::mem::{offset_of, size_of};

    #[test]
    fn vulkan_c_abi_struct_sizes_and_offsets_match_x86_64() {
        assert_eq!(size_of::<VkApplicationInfo>(), 48);
        assert_eq!(size_of::<VkInstanceCreateInfo>(), 64);
        assert_eq!(size_of::<VkPhysicalDeviceProperties>(), 824);
        assert_eq!(size_of::<VkExtensionProperties>(), 260);
        assert_eq!(offset_of!(VkPhysicalDeviceProperties, vendor_id), 8);
        assert_eq!(offset_of!(VkPhysicalDeviceProperties, device_name), 20);
    }
}
