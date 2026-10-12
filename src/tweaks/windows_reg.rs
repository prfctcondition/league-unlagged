//! Windows Core, DirectX Flip Model, HAGS, Multimedia SystemProfile, Kernel Timer, and BCD Tweaks.
//! Includes global and per-application DirectX swapchain upgrade configuration in `UserGpuPreferences`.

use crate::hardware::CREATE_NO_WINDOW;
use crate::tweaks::scheduler::is_running_as_admin;
use std::os::windows::process::CommandExt;
use std::process::Command;
use winreg::enums::*;
use winreg::RegKey;

#[derive(Debug, Clone, Default)]
pub struct WindowsRegState {
    pub game_mode_on: bool,
    pub windowed_opt_on: bool,
    pub hags_disabled: bool,
    pub no_lazy_mode_on: bool,
    pub system_responsiveness_10: bool,
    pub current_system_responsiveness: u32,
    pub network_throttling_10: bool,
    pub current_network_throttling: u32,
    pub global_timer_requests_on: bool,
    pub dynamic_tick_disabled: bool,
}

impl WindowsRegState {
    pub fn refresh(league_paths: &[String]) -> Self {
        let hkcu = RegKey::predef(HKEY_CURRENT_USER);
        let hklm = RegKey::predef(HKEY_LOCAL_MACHINE);

        // 1. Game Mode: BOTH AllowAutoGameMode == 1 AND AutoGameModeEnabled == 1
        let mut game_mode_on = false;
        if let Ok(gb) = hkcu.open_subkey_with_flags(r"Software\Microsoft\GameBar", KEY_READ) {
            let allow: u32 = gb.get_value("AllowAutoGameMode").unwrap_or(1);
            let enabled: u32 = gb.get_value("AutoGameModeEnabled").unwrap_or(1);
            game_mode_on = allow == 1 && enabled == 1;
        }

        // 2. Optimization for Windowed Games:
        // Check BOTH DirectXUserGlobalSettings AND any League of Legends.exe per-app override
        let mut windowed_opt_on = false;
        if let Ok(dx) =
            hkcu.open_subkey_with_flags(r"Software\Microsoft\DirectX\UserGpuPreferences", KEY_READ)
        {
            let global_str: String = dx.get_value("DirectXUserGlobalSettings").unwrap_or_default();
            let global_ok = global_str.contains("SwapEffectUpgradeEnable=1");

            let mut per_app_blocked = false;
            for path in league_paths {
                if let Ok(app_str) = dx.get_value::<String, _>(path) {
                    if app_str.contains("SwapEffectUpgradeEnable=0") {
                        per_app_blocked = true;
                    }
                }
            }
            windowed_opt_on = global_ok && !per_app_blocked;
        }

        // 3. HAGS Disabled: HwSchMode == 1 in HKLM\SYSTEM\CurrentControlSet\Control\GraphicsDrivers
        let mut hags_disabled = false;
        if let Ok(gd) = hklm.open_subkey_with_flags(
            r"SYSTEM\CurrentControlSet\Control\GraphicsDrivers",
            KEY_READ,
        ) {
            let mode: u32 = gd.get_value("HwSchMode").unwrap_or(0);
            hags_disabled = mode == 1;
        }

        // 4. Multimedia\SystemProfile: NoLazyMode, SystemResponsiveness, NetworkThrottlingIndex
        let mut no_lazy_mode_on = false;
        let mut current_system_responsiveness = 20u32;
        let mut current_network_throttling = 10u32;
        if let Ok(sp) = hklm.open_subkey_with_flags(
            r"SOFTWARE\Microsoft\Windows NT\CurrentVersion\Multimedia\SystemProfile",
            KEY_READ,
        ) {
            let nlm1: u32 = sp.get_value("NoLazyMode").unwrap_or(0);
            let nlm2: u32 = sp.get_value("NolazyMode").unwrap_or(0);
            no_lazy_mode_on = nlm1 == 1 || nlm2 == 1;
            current_system_responsiveness = sp.get_value("SystemResponsiveness").unwrap_or(20);
            current_network_throttling = sp.get_value("NetworkThrottlingIndex").unwrap_or(10);
        }

        // 5. GlobalTimerResolutionRequests in Session Manager\kernel
        let mut global_timer_requests_on = false;
        if let Ok(krn) = hklm.open_subkey_with_flags(
            r"SYSTEM\CurrentControlSet\Control\Session Manager\kernel",
            KEY_READ,
        ) {
            let v: u32 = krn.get_value("GlobalTimerResolutionRequests").unwrap_or(0);
            global_timer_requests_on = v == 1;
        }

        // 6. BCD disabledynamictick
        let dynamic_tick_disabled = check_bcd_dynamic_tick_disabled();

        Self {
            game_mode_on,
            windowed_opt_on,
            hags_disabled,
            no_lazy_mode_on,
            system_responsiveness_10: current_system_responsiveness == 10,
            current_system_responsiveness,
            network_throttling_10: current_network_throttling == 10,
            current_network_throttling,
            global_timer_requests_on,
            dynamic_tick_disabled,
        }
    }
}

pub fn set_game_mode(enable: bool) -> Result<String, String> {
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let (gb, _) = hkcu
        .create_subkey(r"Software\Microsoft\GameBar")
        .map_err(|e| format!("Failed to open GameBar registry key: {e}"))?;
    let val: u32 = if enable { 1 } else { 0 };
    gb.set_value("AllowAutoGameMode", &val)
        .map_err(|e| format!("Failed to write AllowAutoGameMode: {e}"))?;
    gb.set_value("AutoGameModeEnabled", &val)
        .map_err(|e| format!("Failed to write AutoGameModeEnabled: {e}"))?;
    Ok(format!(
        "Windows Game Mode -> {} (AllowAutoGameMode={val}, AutoGameModeEnabled={val})",
        if enable { "Status: Active" } else { "Status: Disabled" }
    ))
}

/// Updates BOTH global `DirectXUserGlobalSettings` AND per-app `League of Legends.exe` entries in `UserGpuPreferences`.
pub fn set_windowed_optimizations(enable: bool, league_paths: &[String]) -> Result<String, String> {
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let (dx, _) = hkcu
        .create_subkey(r"Software\Microsoft\DirectX\UserGpuPreferences")
        .map_err(|e| format!("Failed to open UserGpuPreferences: {e}"))?;

    let flag = if enable {
        "SwapEffectUpgradeEnable=1;"
    } else {
        "SwapEffectUpgradeEnable=0;"
    };
    let existing_global: String = dx.get_value("DirectXUserGlobalSettings").unwrap_or_default();
    let new_global = patch_swap_effect_string(&existing_global, enable, false);
    dx.set_value("DirectXUserGlobalSettings", &new_global)
        .map_err(|e| format!("Failed to write DirectXUserGlobalSettings: {e}"))?;

    let mut patched_paths = Vec::new();
    let existing_names: Vec<String> = dx
        .enum_values()
        .flatten()
        .map(|(n, _)| n)
        .filter(|n| n.to_lowercase().ends_with("league of legends.exe"))
        .collect();

    for name in existing_names.iter().chain(league_paths.iter()) {
        if patched_paths
            .iter()
            .any(|p: &String| p.eq_ignore_ascii_case(name))
        {
            continue;
        }
        let cur: String = dx
            .get_value(name)
            .unwrap_or_else(|_| "GpuPreference=2;".to_string());
        let updated = patch_swap_effect_string(&cur, enable, true);
        dx.set_value(name, &updated)
            .map_err(|e| format!("Failed to update {name}: {e}"))?;
        patched_paths.push(name.clone());
    }

    Ok(format!(
        "DirectX Flip Model Optimization -> {} ({flag} global and {} executable profile(s))",
        if enable { "Status: Active" } else { "Status: Disabled" },
        patched_paths.len()
    ))
}

pub fn patch_swap_effect_string(
    existing: &str,
    enable: bool,
    ensure_high_perf_gpu: bool,
) -> String {
    let mut parts: Vec<String> = existing
        .split(';')
        .map(|s| s.trim())
        .filter(|s| !s.is_empty() && !s.starts_with("SwapEffectUpgradeEnable="))
        .map(|s| s.to_string())
        .collect();

    parts.insert(
        0,
        format!("SwapEffectUpgradeEnable={}", if enable { 1 } else { 0 }),
    );
    if ensure_high_perf_gpu && !parts.iter().any(|p| p.starts_with("GpuPreference=")) {
        parts.push("GpuPreference=2".to_string());
    }
    format!("{};", parts.join(";"))
}

pub fn set_hags_disabled(disable_hags: bool) -> Result<String, String> {
    if !is_running_as_admin() {
        return Err(
            "Administrator privileges are required to modify HKLM\\SYSTEM\\CurrentControlSet\\Control\\GraphicsDrivers."
                .to_string(),
        );
    }
    let hklm = RegKey::predef(HKEY_LOCAL_MACHINE);
    let (gd, _) = hklm
        .create_subkey(r"SYSTEM\CurrentControlSet\Control\GraphicsDrivers")
        .map_err(|e| format!("Failed to open GraphicsDrivers key: {e}"))?;
    // HwSchMode = 1 means Disabled (OFF), 2 means Enabled (ON)
    let val: u32 = if disable_hags { 1 } else { 2 };
    gd.set_value("HwSchMode", &val)
        .map_err(|e| format!("Failed to write HwSchMode: {e}"))?;
    Ok(format!(
        "Hardware-Accelerated GPU Scheduling (HAGS) -> {} (HwSchMode={val})",
        if disable_hags {
            "Disabled (Optimal)"
        } else {
            "Enabled"
        }
    ))
}

pub fn set_no_lazy_mode(enable: bool) -> Result<String, String> {
    if !is_running_as_admin() {
        return Err(
            "Administrator privileges are required to modify Multimedia SystemProfile registry keys."
                .to_string(),
        );
    }
    let hklm = RegKey::predef(HKEY_LOCAL_MACHINE);
    let (sp, _) = hklm
        .create_subkey(r"SOFTWARE\Microsoft\Windows NT\CurrentVersion\Multimedia\SystemProfile")
        .map_err(|e| format!("Failed to open Multimedia\\SystemProfile: {e}"))?;
    let (games, _) = hklm
        .create_subkey(
            r"SOFTWARE\Microsoft\Windows NT\CurrentVersion\Multimedia\SystemProfile\Tasks\Games",
        )
        .map_err(|e| format!("Failed to open SystemProfile\\Tasks\\Games: {e}"))?;

    if enable {
        let one: u32 = 1;
        sp.set_value("NoLazyMode", &one)
            .map_err(|e| format!("Failed to write NoLazyMode: {e}"))?;
        let _ = games.set_value("NoLazyMode", &one);
        Ok("Multimedia SystemProfile: NoLazyMode = 1 applied (SystemProfile & Tasks\\Games).".to_string())
    } else {
        let _ = sp.delete_value("NoLazyMode");
        let _ = sp.delete_value("NolazyMode");
        let _ = games.delete_value("NoLazyMode");
        Ok("Multimedia SystemProfile: NoLazyMode removed (Restored to default).".to_string())
    }
}

pub fn set_system_responsiveness(val: u32) -> Result<String, String> {
    if !is_running_as_admin() {
        return Err(
            "Administrator privileges are required to modify SystemResponsiveness.".to_string(),
        );
    }
    let hklm = RegKey::predef(HKEY_LOCAL_MACHINE);
    let (sp, _) = hklm
        .create_subkey(r"SOFTWARE\Microsoft\Windows NT\CurrentVersion\Multimedia\SystemProfile")
        .map_err(|e| format!("Failed to open Multimedia\\SystemProfile: {e}"))?;
    sp.set_value("SystemResponsiveness", &val)
        .map_err(|e| format!("Failed to write SystemResponsiveness: {e}"))?;
    Ok(format!(
        "Multimedia SystemProfile: SystemResponsiveness = {val}%"
    ))
}

pub fn set_network_throttling_index(val: u32) -> Result<String, String> {
    if !is_running_as_admin() {
        return Err(
            "Administrator privileges are required to modify NetworkThrottlingIndex.".to_string(),
        );
    }
    let hklm = RegKey::predef(HKEY_LOCAL_MACHINE);
    let (sp, _) = hklm
        .create_subkey(r"SOFTWARE\Microsoft\Windows NT\CurrentVersion\Multimedia\SystemProfile")
        .map_err(|e| format!("Failed to open Multimedia\\SystemProfile: {e}"))?;
    sp.set_value("NetworkThrottlingIndex", &val)
        .map_err(|e| format!("Failed to write NetworkThrottlingIndex: {e}"))?;
    Ok(format!(
        "Multimedia SystemProfile: NetworkThrottlingIndex = {val} (0x{val:X})"
    ))
}

pub fn set_global_timer_resolution_requests(enable: bool) -> Result<String, String> {
    if !is_running_as_admin() {
        return Err(
            "Administrator privileges are required to modify Session Manager\\kernel."
                .to_string(),
        );
    }
    let hklm = RegKey::predef(HKEY_LOCAL_MACHINE);
    let (krn, _) = hklm
        .create_subkey(r"SYSTEM\CurrentControlSet\Control\Session Manager\kernel")
        .map_err(|e| format!("Failed to open Session Manager\\kernel: {e}"))?;
    let val: u32 = if enable { 1 } else { 0 };
    krn.set_value("GlobalTimerResolutionRequests", &val)
        .map_err(|e| format!("Failed to write GlobalTimerResolutionRequests: {e}"))?;
    Ok(format!("Kernel GlobalTimerResolutionRequests -> {val}"))
}

fn check_bcd_dynamic_tick_disabled() -> bool {
    if let Ok(out) = Command::new("bcdedit")
        .args(["/enum", "{current}"])
        .creation_flags(CREATE_NO_WINDOW)
        .output()
    {
        let s = String::from_utf8_lossy(&out.stdout).to_lowercase();
        for line in s.lines() {
            if line.contains("disabledynamictick") && line.contains("yes") {
                return true;
            }
        }
    }
    false
}

pub fn set_bcd_disable_dynamic_tick(disable_tick: bool) -> Result<String, String> {
    if !is_running_as_admin() {
        return Err("Administrator privileges are required to execute bcdedit.".to_string());
    }
    let args = if disable_tick {
        vec!["/set", "disabledynamictick", "yes"]
    } else {
        vec!["/deletevalue", "disabledynamictick"]
    };
    let out = Command::new("bcdedit")
        .args(&args)
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .map_err(|e| format!("Failed to execute bcdedit: {e}"))?;

    if out.status.success() || !disable_tick {
        Ok(format!(
            "BCD Kernel Dynamic Tick -> {}",
            if disable_tick {
                "disabledynamictick = yes"
            } else {
                "Removed (Windows Default)"
            }
        ))
    } else {
        Err(format!(
            "bcdedit returned an error: {}",
            String::from_utf8_lossy(&out.stderr)
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_patch_swap_effect_string_preserves_gpu_preference() {
        let orig = "SwapEffectUpgradeEnable=0;GpuPreference=2;";
        let updated = patch_swap_effect_string(orig, true, true);
        assert_eq!(updated, "SwapEffectUpgradeEnable=1;GpuPreference=2;");

        let reverted = patch_swap_effect_string(&updated, false, true);
        assert_eq!(reverted, "SwapEffectUpgradeEnable=0;GpuPreference=2;");
    }
}
