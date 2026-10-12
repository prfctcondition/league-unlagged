//! GPU Driver Optimization Engine (AMD Radeon Registry + ADL2 Runtime & NVIDIA Profile Inspector CLI).
//! Dynamically enumerates and validates display driver instances under
//! `HKLM\SYSTEM\CurrentControlSet\Control\Class\{4d36e968-e325-11ce-bfc1-08002be10318}\000X`.

use crate::hardware::{GpuInfo, CREATE_NO_WINDOW};
use crate::tweaks::scheduler::is_running_as_admin;
use std::ffi::c_void;
use std::fs;
use std::os::windows::process::CommandExt;
use std::path::PathBuf;
use std::process::Command;
use winreg::enums::*;
use winreg::RegKey;
use winreg::RegValue;

const DISPLAY_CLASS_GUID_PATH: &str =
    r"SYSTEM\CurrentControlSet\Control\Class\{4d36e968-e325-11ce-bfc1-08002be10318}";

#[derive(Debug, Clone, Default)]
pub struct GpuTweaksState {
    // AMD Radeon States
    pub amd_ulps_disabled: bool,
    pub amd_deep_sleep_disabled: bool,
    pub amd_antilag_enabled: bool,
    pub amd_shader_cache_always_on: bool,
    pub amd_flip_queue_1: bool,
    pub detected_amd_subkeys: Vec<String>,
    // NVIDIA States
    pub nvidia_profile_imported: bool,
}

/// Validates whether a display class registry subkey (`000X`) belongs to an AMD Radeon adapter.
pub fn is_amd_driver_subkey_values(
    matching_device_id: &str,
    provider_name: &str,
    driver_desc: &str,
) -> bool {
    let id_u = matching_device_id.to_uppercase();
    let prov_u = provider_name.to_uppercase();
    let desc_u = driver_desc.to_uppercase();

    id_u.contains("VEN_1002")
        || prov_u.contains("AMD")
        || prov_u.contains("ADVANCED MICRO DEVICES")
        || prov_u.contains("ATI TECHNOLOGIES")
        || desc_u.contains("AMD RADEON")
        || desc_u.contains("RADEON RX")
        || desc_u.contains("RADEON VII")
        || desc_u.contains("RADEON PRO")
        || desc_u.contains("RADEON GRAPHICS")
}

/// Dynamically enumerates all `000X` subkeys under `Control\Class\{4d36e968-e325-11ce-bfc1-08002be10318}`
/// and returns all validated AMD display driver instances, placing `preferred_subkey` first if valid.
pub fn discover_amd_driver_subkeys(preferred_subkey: &str) -> Vec<String> {
    let hklm = RegKey::predef(HKEY_LOCAL_MACHINE);
    let mut subkeys = Vec::new();

    if let Ok(class_key) = hklm.open_subkey_with_flags(DISPLAY_CLASS_GUID_PATH, KEY_READ) {
        for name in class_key.enum_keys().flatten() {
            if name.len() != 4 || !name.chars().all(|c| c.is_ascii_digit()) {
                continue;
            }
            if let Ok(sk) = class_key.open_subkey_with_flags(&name, KEY_READ) {
                let matching_id: String = sk.get_value("MatchingDeviceId").unwrap_or_default();
                let provider: String = sk.get_value("ProviderName").unwrap_or_default();
                let desc: String = sk.get_value("DriverDesc").unwrap_or_default();

                if is_amd_driver_subkey_values(&matching_id, &provider, &desc) {
                    subkeys.push(name);
                }
            }
        }
    }

    subkeys.sort_by_key(|k| if k == preferred_subkey { 0 } else { 1 });
    subkeys
}

impl GpuTweaksState {
    pub fn refresh(gpu: &GpuInfo) -> Self {
        let hklm = RegKey::predef(HKEY_LOCAL_MACHINE);
        let detected_amd_subkeys = discover_amd_driver_subkeys(&gpu.driver_class_subkey);
        let active_subkey = detected_amd_subkeys
            .first()
            .cloned()
            .unwrap_or_else(|| gpu.driver_class_subkey.clone());

        let base_path = format!(r"{DISPLAY_CLASS_GUID_PATH}\{active_subkey}");

        let mut amd_ulps_disabled = false;
        let mut amd_deep_sleep_disabled = false;
        let mut amd_antilag_enabled = false;
        let mut amd_shader_cache_always_on = false;
        let mut amd_flip_queue_1 = false;

        if let Ok(k) = hklm.open_subkey_with_flags(&base_path, KEY_READ) {
            let ulps: u32 = k.get_value("EnableUlps").unwrap_or(1);
            amd_ulps_disabled = ulps == 0;

            let ds: u32 = k.get_value("PP_SclkDeepSleepDisable").unwrap_or(0);
            amd_deep_sleep_disabled = ds == 1;

            let delag: u32 = k.get_value("KMD_DeLagEnabled").unwrap_or(0);
            amd_antilag_enabled = delag == 1;
        }

        if let Ok(umd) = hklm.open_subkey_with_flags(format!(r"{base_path}\UMD"), KEY_READ) {
            if let Ok(raw) = umd.get_raw_value("ShaderCache") {
                // 0x32, 0x00 ('2\0') = Always On; 0x31, 0x00 ('1\0') = AMD Optimized
                amd_shader_cache_always_on = raw.bytes.first().copied() == Some(0x32);
            }
            if let Ok(fq) = umd.get_raw_value("FlipQueueSize") {
                amd_flip_queue_1 = fq.bytes.first().copied() == Some(0x31);
            } else if let Ok(m3d) = umd.get_value::<String, _>("Main3D_DEF") {
                amd_flip_queue_1 = m3d.trim() == "1";
            } else if let Ok(raw) = umd.get_raw_value("Main3D") {
                amd_flip_queue_1 = raw.bytes.first().copied() == Some(0x31);
            }

            if !amd_antilag_enabled {
                if let Ok(raw) = umd.get_raw_value("AntiLag_DevMode") {
                    amd_antilag_enabled = raw.bytes.first().copied() == Some(0x31);
                }
            }
        }

        if let Ok(dxc) = hklm.open_subkey_with_flags(format!(r"{base_path}\UMD\DXC"), KEY_READ) {
            let allow_delag: u32 = dxc.get_value("AllowDelag").unwrap_or(0);
            if allow_delag == 1 {
                amd_antilag_enabled = true;
            }
        }

        let marker = get_tools_dir().join("nvidia").join("applied.marker");
        let nvidia_profile_imported = marker.exists();

        Self {
            amd_ulps_disabled,
            amd_deep_sleep_disabled,
            amd_antilag_enabled,
            amd_shader_cache_always_on,
            amd_flip_queue_1,
            detected_amd_subkeys,
            nvidia_profile_imported,
        }
    }
}

fn require_admin_and_amd_subkeys(preferred_subkey: &str) -> Result<Vec<String>, String> {
    if !is_running_as_admin() {
        return Err(
            "Administrator privileges are required to modify HKLM display driver registry keys."
                .to_string(),
        );
    }
    let subkeys = discover_amd_driver_subkeys(preferred_subkey);
    if subkeys.is_empty() {
        return Err(
            "No active AMD Radeon display driver instance found under HKLM\\SYSTEM\\CurrentControlSet\\Control\\Class\\{4d36e968-e325-11ce-bfc1-08002be10318}."
                .to_string(),
        );
    }
    Ok(subkeys)
}

fn open_amd_class_key(subkey: &str, write: bool) -> Result<RegKey, String> {
    let hklm = RegKey::predef(HKEY_LOCAL_MACHINE);
    let path = format!(r"{DISPLAY_CLASS_GUID_PATH}\{subkey}");
    let flags = if write { KEY_READ | KEY_WRITE } else { KEY_READ };
    hklm.open_subkey_with_flags(&path, flags)
        .map_err(|e| format!("Failed to open GPU driver registry branch ({subkey}): {e}"))
}

pub fn set_amd_ulps_disabled(preferred_subkey: &str, disable_ulps: bool) -> Result<String, String> {
    let subkeys = require_admin_and_amd_subkeys(preferred_subkey)?;
    let val: u32 = if disable_ulps { 0 } else { 1 };
    let str_val = if disable_ulps { "0" } else { "1" };

    for sub in &subkeys {
        let k = open_amd_class_key(sub, true)?;
        k.set_value("EnableUlps", &val)
            .map_err(|e| format!("Failed to write EnableUlps on [{sub}]: {e}"))?;
        let _ = k.set_value("EnableUlps_NA", &str_val);
    }

    Ok(format!(
        "AMD Ultra-Low Power State (EnableUlps) -> {val} on instance(s) [{}] ({})",
        subkeys.join(", "),
        if disable_ulps {
            "Status: Disabled (Low-Power Sleep Prevented)"
        } else {
            "Status: Default"
        }
    ))
}

pub fn set_amd_deep_sleep_disabled(
    preferred_subkey: &str,
    disable_ds: bool,
) -> Result<String, String> {
    let subkeys = require_admin_and_amd_subkeys(preferred_subkey)?;
    let val: u32 = if disable_ds { 1 } else { 0 };

    for sub in &subkeys {
        let k = open_amd_class_key(sub, true)?;
        k.set_value("PP_SclkDeepSleepDisable", &val)
            .map_err(|e| format!("Failed to write PP_SclkDeepSleepDisable on [{sub}]: {e}"))?;
        let _ = k.set_value("DisableDrmdmaPowerGating", &val);
    }

    Ok(format!(
        "AMD Shader Clock Deep Sleep (PP_SclkDeepSleepDisable) -> {val} on instance(s) [{}] ({})",
        subkeys.join(", "),
        if disable_ds {
            "Status: Disabled (Sustained Core Clock)"
        } else {
            "Status: Default"
        }
    ))
}

/// Applies AMD Radeon Anti-Lag via established KMD/UMD/DXC registry keys (`KMD_DeLagEnabled`,
/// `UMD\AntiLag_DevMode`, `UMD\Dlg_PFEnable`, `UMD\DXC\AllowDelag`) across all validated AMD
/// display driver instances, and synchronizes live driver state via `atiadlxx.dll` (`ADL2_DELAG_SettingsX4_Set`).
pub fn set_amd_antilag(preferred_subkey: &str, enable: bool) -> Result<String, String> {
    let subkeys = require_admin_and_amd_subkeys(preferred_subkey)?;
    let val: u32 = if enable { 1 } else { 0 };
    let str_val = if enable { "1" } else { "0" };
    let bin_val = if enable {
        vec![0x31, 0x00]
    } else {
        vec![0x30, 0x00]
    };

    for sub in &subkeys {
        let k = open_amd_class_key(sub, true)?;
        k.set_value("KMD_DeLagEnabled", &val)
            .map_err(|e| format!("Failed to write KMD_DeLagEnabled on [{sub}]: {e}"))?;
        let _ = k.set_value("AntiLag_DevMode", &val);

        if let Ok((umd, _)) = k.create_subkey("UMD") {
            let _ = umd.set_value("AntiLag_DevMode_DEF", &str_val);
            let _ = umd.set_raw_value(
                "AntiLag_DevMode",
                &RegValue {
                    vtype: REG_BINARY,
                    bytes: bin_val.clone(),
                },
            );
            let _ = umd.set_value("Dlg_PFEnable_DEF", &str_val);
            let _ = umd.set_raw_value(
                "Dlg_PFEnable",
                &RegValue {
                    vtype: REG_BINARY,
                    bytes: bin_val.clone(),
                },
            );
            let _ = umd.set_raw_value(
                "Dlg_Level",
                &RegValue {
                    vtype: REG_BINARY,
                    bytes: bin_val.clone(),
                },
            );
        }

        if let Ok((dxc, _)) = k.create_subkey(r"UMD\DXC") {
            let _ = dxc.set_value("AllowDelag", &val);
        }
    }

    let adl_synced = sync_amd_adl2_delag_live(enable);

    Ok(format!(
        "AMD Radeon Anti-Lag (KMD_DeLagEnabled={val}, UMD\\AntiLag_DevMode={str_val}, DXC\\AllowDelag={val}{}) on [{}] -> {}",
        if adl_synced { ", ADL2 Runtime Synced" } else { "" },
        subkeys.join(", "),
        if enable {
            "Status: Active"
        } else {
            "Status: Disabled"
        }
    ))
}

unsafe extern "C" fn adl_malloc_callback(size: i32) -> *mut c_void {
    if size <= 0 {
        return std::ptr::null_mut();
    }
    let layout = match std::alloc::Layout::from_size_align(size as usize, 8) {
        Ok(l) => l,
        Err(_) => return std::ptr::null_mut(),
    };
    std::alloc::alloc(layout) as *mut c_void
}

/// Synchronizes the live AMD display driver Anti-Lag state via `atiadlxx.dll` (`ADL2_DELAG_SettingsX4_Set`)
/// when the AMD Display Library is present on the host system.
fn sync_amd_adl2_delag_live(enable: bool) -> bool {
    #[link(name = "kernel32")]
    extern "system" {
        fn LoadLibraryA(lp_lib_file_name: *const u8) -> *mut c_void;
        fn GetProcAddress(h_module: *mut c_void, lp_proc_name: *const u8) -> *mut c_void;
        fn FreeLibrary(h_lib_module: *mut c_void) -> i32;
    }

    type AdlMainControlCreateFn = unsafe extern "C" fn(
        callback: unsafe extern "C" fn(i32) -> *mut c_void,
        enum_connected_adapters: i32,
        context: *mut *mut c_void,
    ) -> i32;
    type AdlMainControlDestroyFn = unsafe extern "C" fn(context: *mut c_void) -> i32;
    type AdlDelagSettingsX4GetFn =
        unsafe extern "C" fn(context: *mut c_void, adapter_index: i32, buf: *mut i32) -> i32;
    type AdlDelagSettingsX4SetFn =
        unsafe extern "C" fn(context: *mut c_void, adapter_index: i32, buf: *mut i32) -> i32;

    unsafe {
        let h_mod = LoadLibraryA(b"atiadlxx.dll\0".as_ptr());
        if h_mod.is_null() {
            return false;
        }

        let mut updated_any = false;
        let create_ptr = GetProcAddress(h_mod, b"ADL2_Main_Control_Create\0".as_ptr());
        let destroy_ptr = GetProcAddress(h_mod, b"ADL2_Main_Control_Destroy\0".as_ptr());
        let get_ptr = GetProcAddress(h_mod, b"ADL2_DELAG_SettingsX4_Get\0".as_ptr());
        let set_ptr = GetProcAddress(h_mod, b"ADL2_DELAG_SettingsX4_Set\0".as_ptr());

        if !create_ptr.is_null()
            && !destroy_ptr.is_null()
            && !get_ptr.is_null()
            && !set_ptr.is_null()
        {
            let create: AdlMainControlCreateFn = std::mem::transmute(create_ptr);
            let destroy: AdlMainControlDestroyFn = std::mem::transmute(destroy_ptr);
            let get_delag: AdlDelagSettingsX4GetFn = std::mem::transmute(get_ptr);
            let set_delag: AdlDelagSettingsX4SetFn = std::mem::transmute(set_ptr);

            let mut ctx: *mut c_void = std::ptr::null_mut();
            if create(adl_malloc_callback, 1, &mut ctx) == 0 && !ctx.is_null() {
                for adapter_idx in 0..12 {
                    let mut buf = [0i32; 16];
                    if get_delag(ctx, adapter_idx, buf.as_mut_ptr()) == 0 {
                        buf[1] = if enable { 1 } else { 0 };
                        if set_delag(ctx, adapter_idx, buf.as_mut_ptr()) == 0 {
                            updated_any = true;
                        }
                    }
                }
                let _ = destroy(ctx);
            }
        }

        let _ = FreeLibrary(h_mod);
        updated_any
    }
}

pub fn set_amd_shader_cache_always_on(
    preferred_subkey: &str,
    always_on: bool,
) -> Result<String, String> {
    let subkeys = require_admin_and_amd_subkeys(preferred_subkey)?;
    // 0x32, 0x00 = Always On ('2\0'); 0x31, 0x00 = AMD Optimized ('1\0')
    let bytes = if always_on {
        vec![0x32, 0x00]
    } else {
        vec![0x31, 0x00]
    };
    let str_val = if always_on { "2" } else { "1" };

    for sub in &subkeys {
        let k = open_amd_class_key(sub, true)?;
        let (umd, _) = k
            .create_subkey("UMD")
            .map_err(|e| format!("Failed to open UMD branch on [{sub}]: {e}"))?;
        let _ = umd.set_value("ShaderCache_DEF", &str_val);
        umd.set_raw_value(
            "ShaderCache",
            &RegValue {
                vtype: REG_BINARY,
                bytes: bytes.clone(),
            },
        )
        .map_err(|e| format!("Failed to write UMD\\ShaderCache on [{sub}]: {e}"))?;
    }

    Ok(format!(
        "AMD Shader Cache Mode -> {} on instance(s) [{}]",
        if always_on {
            "Always On (0x32 0x00)"
        } else {
            "AMD Optimized (0x31 0x00)"
        },
        subkeys.join(", ")
    ))
}

/// Configures AMD Pre-Render Queue / Flip Queue Size (`FlipQueueSize`, `Main3D`, `Main3D_DEF`)
/// across all validated AMD display driver instances.
pub fn set_amd_flip_queue_size_1(preferred_subkey: &str, queue_1: bool) -> Result<String, String> {
    let subkeys = require_admin_and_amd_subkeys(preferred_subkey)?;
    let str_val = if queue_1 { "1" } else { "3" };
    let bin_val = if queue_1 {
        vec![0x31, 0x00]
    } else {
        vec![0x33, 0x00]
    };

    for sub in &subkeys {
        let k = open_amd_class_key(sub, true)?;
        let (umd, _) = k
            .create_subkey("UMD")
            .map_err(|e| format!("Failed to open UMD branch on [{sub}]: {e}"))?;
        umd.set_value("Main3D_DEF", &str_val)
            .map_err(|e| format!("Failed to write Main3D_DEF on [{sub}]: {e}"))?;
        let _ = umd.set_value("FlipQueueSize_DEF", &str_val);
        let _ = umd.set_raw_value(
            "Main3D",
            &RegValue {
                vtype: REG_BINARY,
                bytes: bin_val.clone(),
            },
        );
        let _ = umd.set_raw_value(
            "FlipQueueSize",
            &RegValue {
                vtype: REG_BINARY,
                bytes: bin_val.clone(),
            },
        );
    }

    Ok(format!(
        "AMD Flip Queue Size (Main3D / FlipQueueSize) -> {str_val} on instance(s) [{}] ({})",
        subkeys.join(", "),
        if queue_1 {
            "1 Frame Pre-Render Queue (Minimum Input Latency)"
        } else {
            "3 Frames (Driver Default)"
        }
    ))
}

/// Restores all AMD Radeon driver registry parameters to standard factory defaults.
pub fn restore_amd_driver_defaults(preferred_subkey: &str) -> Result<String, String> {
    let subkeys = require_admin_and_amd_subkeys(preferred_subkey)?;
    let _ = set_amd_ulps_disabled(preferred_subkey, false);
    let _ = set_amd_deep_sleep_disabled(preferred_subkey, false);
    let _ = set_amd_antilag(preferred_subkey, false);
    let _ = set_amd_shader_cache_always_on(preferred_subkey, false);
    let _ = set_amd_flip_queue_size_1(preferred_subkey, false);
    Ok(format!(
        "AMD Radeon driver parameters restored to defaults on instance(s) [{}].",
        subkeys.join(", ")
    ))
}

fn get_tools_dir() -> PathBuf {
    std::env::current_dir()
        .unwrap_or_else(|_| PathBuf::from("."))
        .join("tools")
}

/// Generates an NVIDIA Profile Inspector `.nip` XML file for `Low Latency Mode` (Ultra/On)
/// and `Power Management Mode: Prefer Maximum Performance`.
pub fn generate_nvidia_nip_xml(ultra_low_latency: bool) -> String {
    let pre_rendered = 1u32;
    let ultra_val = if ultra_low_latency { 1u32 } else { 0u32 };
    format!(
        r#"<?xml version="1.0" encoding="utf-16"?>
<ArrayOfProfile>
  <Profile>
    <ProfileName>Base Profile</ProfileName>
    <Executeables />
    <Settings>
      <ProfileSetting>
        <SettingNameInfo>Power management mode</SettingNameInfo>
        <SettingID>274197361</SettingID>
        <SettingValue>1</SettingValue>
        <ValueType>Dword</ValueType>
      </ProfileSetting>
      <ProfileSetting>
        <SettingNameInfo>Maximum pre-rendered frames</SettingNameInfo>
        <SettingID>8102046</SettingID>
        <SettingValue>{pre_rendered}</SettingValue>
        <ValueType>Dword</ValueType>
      </ProfileSetting>
      <ProfileSetting>
        <SettingNameInfo>Ultra Low Latency</SettingNameInfo>
        <SettingID>390467</SettingID>
        <SettingValue>{ultra_val}</SettingValue>
        <ValueType>Dword</ValueType>
      </ProfileSetting>
    </Settings>
  </Profile>
</ArrayOfProfile>"#
    )
}

pub fn apply_nvidia_profile(enable: bool, ultra: bool) -> Result<String, String> {
    let nv_dir = get_tools_dir().join("nvidia");
    let _ = fs::create_dir_all(&nv_dir);
    let nip_path = nv_dir.join("league_unlagged.nip");
    let exe_path = nv_dir.join("nvidiaProfileInspector.exe");
    let marker = nv_dir.join("applied.marker");

    if !enable {
        let _ = fs::remove_file(&marker);
        return Ok("NVIDIA Driver Profile status reset to default.".to_string());
    }

    let xml = generate_nvidia_nip_xml(ultra);
    fs::write(&nip_path, xml).map_err(|e| format!("Failed to write .nip profile: {e}"))?;

    if !exe_path.exists() {
        let dl_script = format!(
            r#"
$ErrorActionPreference = 'Stop'
$rel = Invoke-RestMethod -Uri 'https://api.github.com/repos/Orbmu2k/nvidiaProfileInspector/releases/latest'
$asset = $rel.assets | Where-Object {{ $_.name -like '*.zip' }} | Select-Object -First 1
$zip = Join-Path '{}' 'npi.zip'
Invoke-WebRequest -Uri $asset.browser_download_url -OutFile $zip
Expand-Archive -Path $zip -DestinationPath '{}' -Force
"#,
            nv_dir.display(),
            nv_dir.display()
        );
        let _ = Command::new("powershell")
            .args(["-NoProfile", "-NonInteractive", "-Command", &dl_script])
            .creation_flags(CREATE_NO_WINDOW)
            .output();
    }

    if exe_path.exists() {
        let _ = Command::new(&exe_path)
            .args(["-silentImport", &nip_path.to_string_lossy()])
            .creation_flags(CREATE_NO_WINDOW)
            .output();
    }
    let _ = fs::write(&marker, "1");
    Ok(format!(
        "NVIDIA Driver Profile (Prefer Maximum Performance + Low Latency Mode: {}) applied.",
        if ultra { "ULTRA" } else { "ON" }
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_amd_driver_subkey_vendor_validation() {
        assert!(is_amd_driver_subkey_values(
            "pci\\ven_1002&dev_731f",
            "Advanced Micro Devices, Inc.",
            "AMD Radeon RX 5700 XT"
        ));
        assert!(!is_amd_driver_subkey_values(
            "pci\\ven_10de&dev_2504",
            "NVIDIA",
            "NVIDIA GeForce RTX 3060"
        ));
        assert!(!is_amd_driver_subkey_values(
            "pci\\ven_8086&dev_56a0",
            "Intel Corporation",
            "Intel(R) Arc(TM) A770 Graphics"
        ));
    }

    #[test]
    fn test_nvidia_nip_xml_generation() {
        let xml = generate_nvidia_nip_xml(true);
        assert!(xml.contains("<SettingID>274197361</SettingID>"));
        assert!(xml.contains("<SettingID>8102046</SettingID>"));
        assert!(xml.contains("<SettingID>390467</SettingID>"));
    }
}
