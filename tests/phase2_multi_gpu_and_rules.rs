use std::collections::BTreeMap;

use hyperx_proton::gpu::{
    DxvkBackend, GpuDeviceType, GpuProbeResult, GpuProfile, GpuVendor,
    choose_dxvk_backend, probe_primary_gpu, select_primary_gpu,
};

fn mock_gpu(
    vendor_id: u32,
    device_id: u32,
    name: &str,
    device_type: GpuDeviceType,
    major: u32,
    minor: u32,
) -> GpuProfile {
    GpuProfile {
        vendor: GpuVendor::from_vendor_id(vendor_id),
        vendor_id,
        device_id,
        device_name: name.to_string(),
        device_type,
        api_major: major,
        api_minor: minor,
        api_patch: 0,
        has_gpl: minor >= 3,
        has_present_wait: minor >= 3,
        has_shader_module_id: vendor_id == 0x10DE && minor >= 3,
    }
}

#[test]
fn select_primary_gpu_prefers_discrete_ignores_llvmpipe_and_honors_filters() {
    let devices = vec![
        mock_gpu(0x10005, 0x0000, "llvmpipe (LLVM 23)", GpuDeviceType::Cpu, 1, 4),
        mock_gpu(0x8086, 0x4680, "Intel UHD Graphics 730", GpuDeviceType::Integrated, 1, 3),
        mock_gpu(0x10DE, 0x21C4, "NVIDIA GeForce GTX 1660 SUPER", GpuDeviceType::Discrete, 1, 4),
        mock_gpu(0x1002, 0x744C, "AMD Radeon RX 7900 XTX", GpuDeviceType::Discrete, 1, 3),
    ];

    // Default: Discrete with highest Vulkan API wins (1.4 > 1.3), never Cpu
    let picked = select_primary_gpu(&devices, None, None).unwrap();
    assert_eq!(picked.vendor, GpuVendor::Nvidia);
    assert_eq!(picked.device_id, 0x21C4);

    // DXVK_FILTER_DEVICE_NAME selects Intel iGPU explicitly
    let intel = select_primary_gpu(&devices, Some("uhd graphics"), None).unwrap();
    assert_eq!(intel.vendor, GpuVendor::Intel);

    // MESA_VK_DEVICE_SELECT with trailing '!' selects AMD RX 7900 XTX
    let amd = select_primary_gpu(&devices, None, Some("1002:744c!")).unwrap();
    assert_eq!(amd.vendor, GpuVendor::Amd);
    assert_eq!(amd.device_id, 0x744C);

    // MESA_VK_DEVICE_SELECT without trailing '!' also works (case-insensitive)
    let amd_no_bang = select_primary_gpu(&devices, None, Some("1002:744C")).unwrap();
    assert_eq!(amd_no_bang.vendor, GpuVendor::Amd);
    assert_eq!(amd_no_bang.device_id, 0x744C);

    // Only Cpu available still falls back to Cpu instead of None
    let cpu_only = select_primary_gpu(&devices[..1], None, None).unwrap();
    assert_eq!(cpu_only.device_type, GpuDeviceType::Cpu);
}

#[test]
fn choose_dxvk_backend_auto_selects_sarek_for_vulkan_1_2_and_handles_overrides() {
    let vk12_intel = GpuProbeResult::Detected(mock_gpu(
        0x8086, 0x1912, "Intel HD 530", GpuDeviceType::Integrated, 1, 2,
    ));
    let vk14_nvidia = GpuProbeResult::Detected(mock_gpu(
        0x10DE, 0x21C4, "GTX 1660 SUPER", GpuDeviceType::Discrete, 1, 4,
    ));
    let unavail = GpuProbeResult::Unavailable("no vulkan".to_string());

    // Vulkan 1.2 + bundled -> Sarek
    let sel = choose_dxvk_backend(&vk12_intel, None, None, true);
    assert_eq!(sel.backend, DxvkBackend::Sarek);
    assert!(sel.warning.is_none());

    // Vulkan 1.2 + NOT bundled -> fallback Standard with warning
    let sel_missing = choose_dxvk_backend(&vk12_intel, None, None, false);
    assert_eq!(sel_missing.backend, DxvkBackend::Standard);
    assert!(sel_missing.warning.is_some());

    // Vulkan 1.4 + auto -> Standard
    assert_eq!(choose_dxvk_backend(&vk14_nvidia, None, None, true).backend, DxvkBackend::Standard);

    // Rule override sarek wins over auto on Vulkan 1.4, env override standard beats rule
    assert_eq!(
        choose_dxvk_backend(&vk14_nvidia, None, Some(DxvkBackend::Sarek), true).backend,
        DxvkBackend::Sarek
    );
    assert_eq!(
        choose_dxvk_backend(&vk14_nvidia, Some("standard"), Some(DxvkBackend::Sarek), true).backend,
        DxvkBackend::Standard
    );

    // Unavailable GPU -> fail-soft Standard
    assert_eq!(choose_dxvk_backend(&unavail, None, None, true).backend, DxvkBackend::Standard);
}

#[test]
fn probe_primary_gpu_respects_no_probe_env_and_is_fail_soft() {
    let mut env = BTreeMap::new();
    env.insert("HYPERX_PROTON_NO_GPU_PROBE".to_string(), "1".to_string());
    assert!(matches!(probe_primary_gpu(&env), GpuProbeResult::Unavailable(_)));
}
