//! PCI/PCIe Interrupt Affinity Policy & Message Signaled Interrupts (MSI Mode) Engine.
//! Dynamically detects hardware MSI capability under `HKLM\SYSTEM\CurrentControlSet\Enum\...`,
//! enforces strict safety blacklists, and configures `MSISupported`, `MessageNumberLimit`,
//! and `AssignmentSetOverride` across supported PCIe devices.

use crate::hardware::{mask_to_cpu_list_string, SystemHardwareReport, CREATE_NO_WINDOW};
use crate::tweaks::scheduler::is_running_as_admin;
use std::os::windows::process::CommandExt;
use std::process::Command;
use winreg::enums::*;
use winreg::RegKey;
use winreg::RegValue;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MsiDeviceCategory {
    DiscreteGpu,
    UsbXhci,
    NvmeStorage,
    NetworkAdapter,
}

impl MsiDeviceCategory {
    pub fn category_name(self) -> &'static str {
        match self {
            MsiDeviceCategory::DiscreteGpu => "Discrete GPU (Display Adapter)",
            MsiDeviceCategory::UsbXhci => "USB 3.0 xHCI Host Controller",
            MsiDeviceCategory::NvmeStorage => "NVMe Storage Controller",
            MsiDeviceCategory::NetworkAdapter => "Network Interface Controller",
        }
    }

    /// Returns the recommended `MessageNumberLimit` for the device category.
    pub fn recommended_message_number_limit(self) -> Option<u32> {
        match self {
            MsiDeviceCategory::DiscreteGpu => Some(16),
            MsiDeviceCategory::UsbXhci => Some(8),
            MsiDeviceCategory::NvmeStorage => Some(16),
            MsiDeviceCategory::NetworkAdapter => Some(8),
        }
    }
}

#[derive(Debug, Clone)]
pub struct DeviceIrqStatus {
    pub category: MsiDeviceCategory,
    pub label: String,
    pub pnp_id: String,
    pub hardware_msi_capable: bool,
    pub msi_supported: bool,
    pub message_number_limit: Option<u32>,
    pub device_policy: u32,
    pub current_mask: u64,
    pub recommended_mask: u64,
    pub is_pinned_to_recommended: bool,
}

#[derive(Debug, Clone, Default)]
pub struct InterruptsState {
    pub gpu_irq: Option<DeviceIrqStatus>,
    pub usb_irqs: Vec<DeviceIrqStatus>,
    pub nvme_irqs: Vec<DeviceIrqStatus>,
    pub nic_irq: Option<DeviceIrqStatus>,
}

/// Strict safety blacklist filter: prevents touching legacy audio controllers, IDE/legacy SATA
/// controllers, or sensitive platform timers/bridges known to cause instability or BSODs.
pub fn is_msi_blacklisted_device(pnp_id: &str, description_or_service: &str) -> bool {
    let pnp_u = pnp_id.trim().to_uppercase();
    let desc_u = description_or_service.trim().to_uppercase();

    // Must be a valid PCI/PCIe bus device
    if !pnp_u.starts_with("PCI\\") {
        return true;
    }

    // Legacy Audio / High Definition Audio controllers
    let audio_tokens = [
        "HDAUDIO",
        "HIGH DEFINITION AUDIO",
        "AUDIO CONTROLLER",
        "SOUND",
        "REALTEK AUDIO",
        "AZALIA",
    ];
    if audio_tokens.iter().any(|t| pnp_u.contains(t) || desc_u.contains(t)) {
        return true;
    }

    // Legacy IDE / SATA / AHCI controllers (unless explicitly NVMe)
    let is_nvme = desc_u.contains("NVME")
        || desc_u.contains("NVM EXPRESS")
        || desc_u.contains("STORNVME");
    let sata_ide_tokens = [
        "PCIIDE",
        "ATAPI",
        "IASTOR",
        "STORAHCI",
        "STANDARD SATA",
        "SATA AHCI",
        "IDE CONTROLLER",
        "SERIAL ATA",
    ];
    if !is_nvme
        && sata_ide_tokens
            .iter()
            .any(|t| pnp_u.contains(t) || desc_u.contains(t))
    {
        return true;
    }

    // Sensitive Platform Timers, ACPI, LPC, SMBus, ISA, and Host Bridges
    let timer_bridge_tokens = [
        "HPET",
        "HIGH PRECISION EVENT TIMER",
        "SYSTEM TIMER",
        "SMBUS",
        "ISA BRIDGE",
        "LPC CONTROLLER",
        "HOST BRIDGE",
        "PCI BRIDGE",
    ];
    if timer_bridge_tokens
        .iter()
        .any(|t| pnp_u.contains(t) || desc_u.contains(t))
    {
        return true;
    }

    false
}

impl InterruptsState {
    pub fn refresh(hw: &SystemHardwareReport) -> Self {
        let gpu_irq = if !hw.gpu.pnp_device_id.is_empty()
            && !is_msi_blacklisted_device(&hw.gpu.pnp_device_id, &hw.gpu.name)
        {
            Some(read_device_irq_status(
                MsiDeviceCategory::DiscreteGpu,
                &format!("GPU: {}", hw.gpu.name),
                &hw.gpu.pnp_device_id,
                hw.cpu_plan.gpu_irq_mask,
            ))
        } else {
            None
        };

        let mut usb_irqs = Vec::new();
        for (idx, u) in hw.usb_controllers.iter().enumerate() {
            if u.pnp_device_id.is_empty() || is_msi_blacklisted_device(&u.pnp_device_id, &u.name) {
                continue;
            }
            let rec_mask = if idx == 0 {
                hw.cpu_plan.usb_primary_irq_mask
            } else {
                hw.cpu_plan.usb_secondary_irq_mask
            };
            let bus_tag = if u.is_cpu_soc_direct {
                "Direct CPU SoC Lanes"
            } else {
                "Motherboard Chipset Bus"
            };
            let label = format!("USB #{}: {} [{}]", idx + 1, u.name, bus_tag);
            usb_irqs.push(read_device_irq_status(
                MsiDeviceCategory::UsbXhci,
                &label,
                &u.pnp_device_id,
                rec_mask,
            ));
        }

        let mut nvme_irqs = Vec::new();
        for (idx, nvme) in hw.nvme_controllers.iter().enumerate() {
            let combined_desc = format!("{} {}", nvme.name, nvme.service);
            if nvme.pnp_device_id.is_empty()
                || is_msi_blacklisted_device(&nvme.pnp_device_id, &combined_desc)
            {
                continue;
            }
            let label = format!("NVMe #{}: {}", idx + 1, nvme.name);
            nvme_irqs.push(read_device_irq_status(
                MsiDeviceCategory::NvmeStorage,
                &label,
                &nvme.pnp_device_id,
                hw.cpu_plan.usb_secondary_irq_mask,
            ));
        }

        let nic_irq = if !hw.nic.pnp_device_id.is_empty()
            && !is_msi_blacklisted_device(&hw.nic.pnp_device_id, &hw.nic.description)
        {
            Some(read_device_irq_status(
                MsiDeviceCategory::NetworkAdapter,
                &format!("NIC: {}", hw.nic.description),
                &hw.nic.pnp_device_id,
                hw.cpu_plan.nic_irq_mask,
            ))
        } else {
            None
        };

        Self {
            gpu_irq,
            usb_irqs,
            nvme_irqs,
            nic_irq,
        }
    }
}

pub fn encode_mask_to_reg_binary(mask: u64) -> Vec<u8> {
    if mask <= 0xFFFF {
        (mask as u16).to_le_bytes().to_vec()
    } else if mask <= 0xFFFF_FFFF {
        (mask as u32).to_le_bytes().to_vec()
    } else {
        mask.to_le_bytes().to_vec()
    }
}

pub fn decode_reg_binary_to_mask(bytes: &[u8]) -> u64 {
    let mut mask = 0u64;
    for (i, &b) in bytes.iter().take(8).enumerate() {
        mask |= (b as u64) << (i * 8);
    }
    mask
}

pub fn read_device_irq_status(
    category: MsiDeviceCategory,
    label: &str,
    pnp_id: &str,
    recommended_mask: u64,
) -> DeviceIrqStatus {
    let hklm = RegKey::predef(HKEY_LOCAL_MACHINE);
    let base_path = format!(
        r"SYSTEM\CurrentControlSet\Enum\{pnp_id}\Device Parameters\Interrupt Management"
    );

    let mut hardware_msi_capable = false;
    let mut msi_supported = false;
    let mut message_number_limit = None;

    if let Ok(msi_key) = hklm.open_subkey_with_flags(
        format!(r"{base_path}\MessageSignaledInterruptProperties"),
        KEY_READ,
    ) {
        hardware_msi_capable = true;
        let val: u32 = msi_key.get_value("MSISupported").unwrap_or(0);
        msi_supported = val == 1;
        message_number_limit = msi_key.get_value::<u32, _>("MessageNumberLimit").ok();
    } else if hklm.open_subkey_with_flags(&base_path, KEY_READ).is_ok() {
        hardware_msi_capable = true;
    }

    let mut device_policy = 0u32;
    let mut current_mask = 0u64;
    if let Ok(aff_key) =
        hklm.open_subkey_with_flags(format!(r"{base_path}\Affinity Policy"), KEY_READ)
    {
        device_policy = aff_key.get_value("DevicePolicy").unwrap_or(0);
        if let Ok(raw) = aff_key.get_raw_value("AssignmentSetOverride") {
            current_mask = decode_reg_binary_to_mask(&raw.bytes);
        }
    }

    let is_pinned =
        device_policy == 4 && current_mask != 0 && (current_mask & 1) == 0 && msi_supported;

    DeviceIrqStatus {
        category,
        label: label.to_string(),
        pnp_id: pnp_id.to_string(),
        hardware_msi_capable,
        msi_supported,
        message_number_limit,
        device_policy,
        current_mask,
        recommended_mask,
        is_pinned_to_recommended: is_pinned,
    }
}

/// Safely configures Message Signaled Interrupts (`MSISupported` and `MessageNumberLimit`)
/// for a verified PCI device after checking Administrator privileges and safety blacklists.
pub fn set_device_msi_mode(
    category: MsiDeviceCategory,
    pnp_id: &str,
    device_name: &str,
    enable_msi: bool,
) -> Result<String, String> {
    if !is_running_as_admin() {
        return Err(format!(
            "Administrator privileges are required to configure MSI Mode for {device_name}."
        ));
    }
    if pnp_id.trim().is_empty() {
        return Err("Device PNPDeviceID is empty.".to_string());
    }
    if is_msi_blacklisted_device(pnp_id, device_name) {
        return Err(format!(
            "Safety Blacklist: MSI modification is blocked for '{device_name}' ({pnp_id}) to prevent system instability."
        ));
    }

    let hklm = RegKey::predef(HKEY_LOCAL_MACHINE);
    let enum_dev_path = format!(r"SYSTEM\CurrentControlSet\Enum\{pnp_id}\Device Parameters");
    if hklm.open_subkey_with_flags(&enum_dev_path, KEY_READ).is_err() {
        return Err(format!(
            "Registry branch HKLM\\{enum_dev_path} does not exist for {device_name}."
        ));
    }

    let msi_path =
        format!(r"{enum_dev_path}\Interrupt Management\MessageSignaledInterruptProperties");
    let (msi_key, _) = hklm.create_subkey(&msi_path).map_err(|e| {
        format!("Access denied to MessageSignaledInterruptProperties for {device_name}: {e}")
    })?;

    let val: u32 = if enable_msi { 1 } else { 0 };
    msi_key
        .set_value("MSISupported", &val)
        .map_err(|e| format!("Failed to write MSISupported for {device_name}: {e}"))?;

    if enable_msi {
        if let Some(limit) = category.recommended_message_number_limit() {
            let _ = msi_key.set_value("MessageNumberLimit", &limit);
        }
    }

    Ok(format!(
        "Message Signaled Interrupts (MSI) for {device_name} -> MSISupported={val} ({})",
        if enable_msi {
            "Status: Active"
        } else {
            "Status: Disabled (Line-Based Interrupts)"
        }
    ))
}

/// Applies MSI Mode (`MSISupported = 1`) and pins Interrupt Affinity (`DevicePolicy = 4`, `AssignmentSetOverride = mask`)
/// for a verified PCI device (`pnp_id`). If `enable` is false, removes the affinity override (reverts to default OS policy).
pub fn set_device_interrupt_affinity(
    pnp_id: &str,
    device_name: &str,
    enable: bool,
    target_mask: u64,
) -> Result<String, String> {
    if !is_running_as_admin() {
        return Err(format!(
            "Administrator privileges are required to modify Interrupt Affinity Policy for {device_name}."
        ));
    }
    if pnp_id.trim().is_empty() {
        return Err("Device PNPDeviceID is empty.".to_string());
    }
    if is_msi_blacklisted_device(pnp_id, device_name) {
        return Err(format!(
            "Safety Blacklist: Interrupt modification is blocked for '{device_name}' ({pnp_id})."
        ));
    }

    let hklm = RegKey::predef(HKEY_LOCAL_MACHINE);
    let base_path =
        format!(r"SYSTEM\CurrentControlSet\Enum\{pnp_id}\Device Parameters\Interrupt Management");

    // 1. Ensure MSI Mode is enabled when pinning interrupt affinity
    if enable {
        if let Ok((msi_key, _)) =
            hklm.create_subkey(format!(r"{base_path}\MessageSignaledInterruptProperties"))
        {
            let one: u32 = 1;
            let _ = msi_key.set_value("MSISupported", &one);
        }
    }

    // 2. Configure Affinity Policy
    let (aff_key, _) = hklm
        .create_subkey(format!(r"{base_path}\Affinity Policy"))
        .map_err(|e| {
            format!("Access denied to Affinity Policy ({device_name}). Run as Administrator: {e}")
        })?;

    if enable {
        let policy_specified: u32 = 4; // IrqPolicySpecifiedProcessors
        aff_key
            .set_value("DevicePolicy", &policy_specified)
            .map_err(|e| format!("Failed to write DevicePolicy: {e}"))?;

        let raw_val = RegValue {
            vtype: REG_BINARY,
            bytes: encode_mask_to_reg_binary(target_mask),
        };
        aff_key
            .set_raw_value("AssignmentSetOverride", &raw_val)
            .map_err(|e| format!("Failed to write AssignmentSetOverride: {e}"))?;

        Ok(format!(
            "Interrupt Affinity for {device_name} pinned to CPU [{}] (Mask 0x{:04X}, MSI: Active)",
            mask_to_cpu_list_string(target_mask),
            target_mask
        ))
    } else {
        let _ = aff_key.delete_value("DevicePolicy");
        let _ = aff_key.delete_value("AssignmentSetOverride");
        Ok(format!(
            "Interrupt Affinity for {device_name} restored to Windows Default Policy"
        ))
    }
}

pub fn restart_pnp_device(pnp_id: &str) -> Result<String, String> {
    let out = Command::new("pnputil")
        .args(["/restart-device", pnp_id])
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .map_err(|e| format!("Failed to execute pnputil: {e}"))?;

    if out.status.success() {
        Ok(format!(
            "Device {pnp_id} restarted successfully (Interrupt policy reloaded)."
        ))
    } else {
        Ok(format!(
            "Interrupt policy saved to registry for {pnp_id} (Takes effect after system reboot)."
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_reg_binary_mask_roundtrip() {
        let bytes = encode_mask_to_reg_binary(0x0004);
        assert_eq!(bytes, vec![0x04, 0x00]);
        assert_eq!(decode_reg_binary_to_mask(&bytes), 0x0004);

        let bytes2 = encode_mask_to_reg_binary(0x0010);
        assert_eq!(bytes2, vec![0x10, 0x00]);
        assert_eq!(decode_reg_binary_to_mask(&bytes2), 0x0010);
    }

    #[test]
    fn test_msi_blacklist_filters_unsafe_devices() {
        // Blocked: Legacy Audio, SATA AHCI, IDE, HPET, ACPI
        assert!(is_msi_blacklisted_device(
            "HDAUDIO\\FUNC_01&VEN_10EC&DEV_0892",
            "Realtek High Definition Audio"
        ));
        assert!(is_msi_blacklisted_device(
            "PCI\\VEN_1022&DEV_1487",
            "High Definition Audio Controller"
        ));
        assert!(is_msi_blacklisted_device(
            "PCI\\VEN_1022&DEV_7901",
            "Standard SATA AHCI Controller"
        ));
        assert!(is_msi_blacklisted_device(
            "ACPI\\PNP0103\\0",
            "High precision event timer"
        ));

        // Allowed: Discrete GPU, USB xHCI, NVMe Storage, Ethernet NIC
        assert!(!is_msi_blacklisted_device(
            "PCI\\VEN_1002&DEV_731F",
            "AMD Radeon RX 5700 XT"
        ));
        assert!(!is_msi_blacklisted_device(
            "PCI\\VEN_1022&DEV_149C",
            "AMD USB 3.10 eXtensible Host Controller"
        ));
        assert!(!is_msi_blacklisted_device(
            "PCI\\VEN_144D&DEV_A808",
            "Standard NVM Express Controller"
        ));
        assert!(!is_msi_blacklisted_device(
            "PCI\\VEN_10EC&DEV_8168",
            "Realtek PCIe GbE Family Controller"
        ));
    }
}
