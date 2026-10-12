//! Universal Network Adapter & UDP Stack Tweaks.
//! Manages Windows UDP Receive Segment Coalescing Offload (URO), NIC Interrupt Moderation, and Energy-Efficient Ethernet (EEE).

use crate::hardware::CREATE_NO_WINDOW;
use crate::tweaks::scheduler::is_running_as_admin;
use std::os::windows::process::CommandExt;
use std::process::Command;
use std::sync::atomic::{AtomicI8, Ordering};

/// Caches the most recent explicitly applied URO state (-1 = unknown, 0 = enabled, 1 = disabled)
/// so localized Windows builds without English `netsh` resource packs still report immediate state accuracy.
static LAST_APPLIED_URO_DISABLED: AtomicI8 = AtomicI8::new(-1);

#[derive(Debug, Clone, Default)]
pub struct NetworkState {
    pub uro_disabled: bool,
    pub interrupt_moderation_disabled: bool,
    pub green_ethernet_disabled: bool,
}

impl NetworkState {
    pub fn refresh(adapter_name: &str) -> Self {
        let uro_disabled = check_uro_disabled();
        let (interrupt_moderation_disabled, green_ethernet_disabled) =
            check_nic_properties(adapter_name);
        Self {
            uro_disabled,
            interrupt_moderation_disabled,
            green_ethernet_disabled,
        }
    }
}

pub fn check_uro_disabled() -> bool {
    let cached = LAST_APPLIED_URO_DISABLED.load(Ordering::Relaxed);
    if cached == 1 {
        return true;
    } else if cached == 0 {
        return false;
    }

    if let Ok(out) = Command::new("netsh")
        .args(["int", "udp", "show", "global"])
        .creation_flags(CREATE_NO_WINDOW)
        .output()
    {
        let text = String::from_utf8_lossy(&out.stdout).to_lowercase();
        for line in text.lines() {
            if (line.contains("receive offload") || line.contains("coalescing"))
                && (line.contains("disabled") || line.contains("off"))
            {
                return true;
            }
        }
        // Fallback: first setting line containing ':' in `netsh int udp show global` is Receive Offload State (URO)
        if let Some(first_setting) = text.lines().find(|l| l.contains(':')) {
            if let Some(val) = first_setting.split(':').nth(1) {
                let v = val.trim();
                if v.contains("disabled") || v.contains("off") || v.contains("false") {
                    return true;
                }
            }
        }
    }
    false
}

pub fn set_uro_disabled(disable: bool) -> Result<String, String> {
    if !is_running_as_admin() {
        return Err(
            "Administrator privileges required to modify UDP global offload parameters.".to_string(),
        );
    }

    let arg = if disable { "uro=disabled" } else { "uro=enabled" };
    let out = Command::new("netsh")
        .args(["int", "udp", "set", "global", arg])
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .map_err(|e| format!("Failed to execute netsh: {e}"))?;

    if out.status.success() {
        LAST_APPLIED_URO_DISABLED.store(if disable { 1 } else { 0 }, Ordering::Relaxed);
        Ok(format!(
            "UDP Receive Segment Coalescing Offload (URO) -> {}",
            if disable {
                "Status: Disabled (Unbuffered UDP Packet Delivery)"
            } else {
                "Status: Enabled (Windows Default)"
            }
        ))
    } else {
        Err(format!(
            "netsh int udp set global {arg} failed: {}",
            String::from_utf8_lossy(&out.stdout).trim()
        ))
    }
}

fn check_nic_properties(adapter_name: &str) -> (bool, bool) {
    if adapter_name.trim().is_empty() {
        return (false, false);
    }
    let script = format!(
        r#"
$ErrorActionPreference = 'SilentlyContinue'
$props = Get-NetAdapterAdvancedProperty -Name '{}'
$im = ($props | Where-Object {{ $_.RegistryKeyword -eq '*InterruptModeration' }}).RegistryValue
$eee = ($props | Where-Object {{ $_.RegistryKeyword -in @('*EEE', 'AdvancedEEE', 'EnableGreenEthernet') }}).RegistryValue
$imOff = ($im -contains '0')
$eeeOff = ($eee -notcontains '1')
Write-Output "$imOff|$eeeOff"
"#,
        adapter_name.replace('\'', "''")
    );

    if let Ok(out) = Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-Command", &script])
        .creation_flags(CREATE_NO_WINDOW)
        .output()
    {
        let s = String::from_utf8_lossy(&out.stdout).trim().to_lowercase();
        let parts: Vec<&str> = s.split('|').collect();
        if parts.len() == 2 {
            return (parts[0] == "true", parts[1] == "true");
        }
    }
    (false, false)
}

pub fn set_nic_interrupt_moderation_disabled(
    adapter_name: &str,
    disable: bool,
) -> Result<String, String> {
    if !is_running_as_admin() {
        return Err(
            "Administrator privileges required to modify network adapter driver properties."
                .to_string(),
        );
    }
    if adapter_name.trim().is_empty() {
        return Err("No active network interface controller detected.".to_string());
    }

    let val = if disable { "0" } else { "1" };
    let script = format!(
        r#"
$ErrorActionPreference = 'Stop'
Set-NetAdapterAdvancedProperty -Name '{}' -RegistryKeyword '*InterruptModeration' -RegistryValue {} -NoRestart
Restart-NetAdapter -Name '{}'
"#,
        adapter_name.replace('\'', "''"),
        val,
        adapter_name.replace('\'', "''")
    );

    let out = Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-Command", &script])
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .map_err(|e| format!("Failed to execute Set-NetAdapterAdvancedProperty: {e}"))?;

    if out.status.success() {
        Ok(format!(
            "NIC ({adapter_name}): Interrupt Moderation -> {}",
            if disable {
                "Status: Disabled (0)"
            } else {
                "Status: Enabled (1)"
            }
        ))
    } else {
        Err(format!(
            "Failed to modify *InterruptModeration on {adapter_name}: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        ))
    }
}

pub fn set_nic_green_ethernet_disabled(
    adapter_name: &str,
    disable: bool,
) -> Result<String, String> {
    if !is_running_as_admin() {
        return Err(
            "Administrator privileges required to modify network adapter power properties."
                .to_string(),
        );
    }
    if adapter_name.trim().is_empty() {
        return Err("No active network interface controller detected.".to_string());
    }

    let val = if disable { "0" } else { "1" };
    let script = format!(
        r#"
$ErrorActionPreference = 'SilentlyContinue'
foreach ($kw in @('*EEE', 'AdvancedEEE', 'EnableGreenEthernet', 'PowerSavingMode')) {{
    Set-NetAdapterAdvancedProperty -Name '{}' -RegistryKeyword $kw -RegistryValue {} -NoRestart
}}
Restart-NetAdapter -Name '{}'
"#,
        adapter_name.replace('\'', "''"),
        val,
        adapter_name.replace('\'', "''")
    );

    let _ = Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-Command", &script])
        .creation_flags(CREATE_NO_WINDOW)
        .output();

    Ok(format!(
        "NIC ({adapter_name}): Energy-Efficient Ethernet (EEE) -> {}",
        if disable {
            "Status: Disabled (0)"
        } else {
            "Status: Enabled (1)"
        }
    ))
}
