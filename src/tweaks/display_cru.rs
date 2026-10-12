//! Windows 11 Display Topology Optimization (VESA DisplayID 2.0 Tiled Display Topology `0x28`).
//! Directly constructs and applies `EDID_OVERRIDE` in the Windows Registry (preserving existing CEA-861
//! high-refresh-rate extension blocks) and provides optional ToastyX CRU integration.

use crate::hardware::CREATE_NO_WINDOW;
use crate::tweaks::scheduler::is_running_as_admin;
use std::fs;
use std::os::windows::process::CommandExt;
use std::path::PathBuf;
use std::process::Command;
use winreg::enums::*;
use winreg::RegKey;
use winreg::RegValue;

#[derive(Debug, Clone, Default)]
pub struct DisplayCruState {
    pub edid_override_present: bool,
    pub cru_downloaded: bool,
    pub active_displays: Vec<String>,
}

impl DisplayCruState {
    pub fn refresh() -> Self {
        let (edid_override_present, active_displays) = scan_display_edid_overrides();
        let cru_downloaded = get_cru_dir().join("CRU.exe").exists();
        Self {
            edid_override_present,
            cru_downloaded,
            active_displays,
        }
    }
}

pub fn get_cru_dir() -> PathBuf {
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            return dir.join("tools").join("cru");
        }
    }
    std::env::current_dir()
        .unwrap_or_else(|_| PathBuf::from("."))
        .join("tools")
        .join("cru")
}

/// Extracts preferred native resolution `(width, height)` from a 128-byte base EDID block
/// (Detailed Timing Descriptor #1 at bytes `54..72`).
pub fn extract_native_resolution_from_edid(edid: &[u8]) -> (u16, u16) {
    if edid.len() >= 72 && (edid[54] != 0 || edid[55] != 0) {
        let h_active = (edid[56] as u16) | (((edid[58] as u16) & 0xF0) << 4);
        let v_active = (edid[59] as u16) | (((edid[61] as u16) & 0xF0) << 4);
        if h_active >= 640 && v_active >= 480 {
            return (h_active, v_active);
        }
    }
    (1920, 1080)
}

/// Builds a standard 128-byte VESA DisplayID 2.0 (`0x70`) EDID extension block containing
/// a Tiled Display Topology (`0x28`) data block (`1x1` single-enclosure tile at native resolution).
pub fn build_displayid2_tiled_extension_block(base_edid: &[u8]) -> [u8; 128] {
    let (width, height) = extract_native_resolution_from_edid(base_edid);
    let w_minus_1 = width.saturating_sub(1);
    let h_minus_1 = height.saturating_sub(1);

    let mut block = [0u8; 128];
    // Byte 0: EDID Extension Tag for DisplayID (0x70)
    block[0] = 0x70;

    // Bytes 1..5: DisplayID 2.0 Section Header (`displayid_header`)
    block[1] = 0x20; // rev = 0x20 (DisplayID v2.0)
    block[2] = 25;   // prod_id / bytes count in section = 25 bytes (3-byte block header + 22-byte payload)
    block[3] = 0x00; // primary use case = 0x00 (Extension)
    block[4] = 0x00; // extension count = 0

    // Bytes 5..30: Tiled Display Topology Data Block (`0x28`, 25 bytes total)
    block[5] = 0x28; // tag = DATA_BLOCK_2_TILED_DISPLAY_TOPOLOGY (0x28)
    block[6] = 0x00; // revision = 0
    block[7] = 22;   // payload length = 22 bytes (0x16)
    block[8] = 0x81; // capabilities: bit 7 (0x80) = single physical enclosure, bit 0 (0x01) = scale to fit
    block[9] = 0x00; // num_h_tile - 1 = 0 (1 horizontal tile)
    block[10] = 0x00; // num_v_tile - 1 = 0 (1 vertical tile)
    block[11] = 0x00; // tile_h_loc = 0, tile_v_loc = 0 (location 1,1)
    block[12] = (w_minus_1 & 0xFF) as u8;
    block[13] = ((w_minus_1 >> 8) & 0xFF) as u8;
    block[14] = (h_minus_1 & 0xFF) as u8;
    block[15] = ((h_minus_1 >> 8) & 0xFF) as u8;
    // Bytes 16..21: bezel info (0,0,0,0,0)
    // Bytes 21..30: 9-byte topology_id (copied from base EDID vendor/product/serial bytes 8..17 if available)
    if base_edid.len() >= 17 {
        block[21..30].copy_from_slice(&base_edid[8..17]);
    } else {
        block[21..30].copy_from_slice(b"UNLAGGED1");
    }

    // Byte 30: DisplayID Section Checksum over bytes 1..30 so sum(block[1..=30]) % 256 == 0
    let section_sum: u32 = block[1..30].iter().map(|&b| b as u32).sum();
    block[30] = ((256 - (section_sum % 256)) % 256) as u8;

    // Byte 127: Outer 128-byte EDID Extension Block Checksum over bytes 0..127 so sum(block[0..=127]) % 256 == 0
    let edid_sum: u32 = block[0..127].iter().map(|&b| b as u32).sum();
    block[127] = ((256 - (edid_sum % 256)) % 256) as u8;

    block
}

/// Applies or removes the VESA DisplayID 2.0 Tiled Topology `EDID_OVERRIDE` across all active monitors
/// in `HKLM\SYSTEM\CurrentControlSet\Enum\DISPLAY`, preserving any existing CEA-861 extension blocks.
pub fn set_displayid2_tiled_topology_override(enable: bool) -> Result<String, String> {
    if !is_running_as_admin() {
        return Err(
            "Administrator privileges required to modify monitor EDID_OVERRIDE keys.".to_string(),
        );
    }

    let hklm = RegKey::predef(HKEY_LOCAL_MACHINE);
    let disp_root = hklm
        .open_subkey_with_flags(r"SYSTEM\CurrentControlSet\Enum\DISPLAY", KEY_READ)
        .map_err(|e| format!("Failed to open HKLM\\...\\Enum\\DISPLAY: {e}"))?;

    let mut modified_monitors = Vec::new();

    for monitor_id in disp_root.enum_keys().flatten() {
        if monitor_id.starts_with("Default_Monitor") {
            continue;
        }
        let Ok(mon_key) = disp_root.open_subkey_with_flags(&monitor_id, KEY_READ) else {
            continue;
        };

        // First pass: find the fullest EDID blob across all instances of this monitor_id
        // (so if one instance has all 256 bytes including CEA-861 and another only cached 128 bytes, we preserve the full 256 bytes).
        let mut best_edid: Vec<u8> = Vec::new();
        let instances: Vec<String> = mon_key.enum_keys().flatten().collect();
        for inst_id in &instances {
            let param_path = format!(r"{inst_id}\Device Parameters");
            if let Ok(dp_key) = mon_key.open_subkey_with_flags(&param_path, KEY_READ) {
                if let Ok(raw) = dp_key.get_raw_value("EDID") {
                    if raw.bytes.len() > best_edid.len() && raw.bytes.len() >= 128 {
                        best_edid = raw.bytes;
                    }
                }
            }
        }

        if best_edid.len() < 128 {
            continue;
        }

        for inst_id in &instances {
            let param_path = format!(r"{inst_id}\Device Parameters");
            let Ok(dp_key) = mon_key.open_subkey_with_flags(&param_path, KEY_READ | KEY_WRITE) else {
                return Err(format!(
                    "Access denied opening Device Parameters for monitor {monitor_id}. Run as Administrator."
                ));
            };

            if enable {
                let mut block0 = [0u8; 128];
                block0.copy_from_slice(&best_edid[0..128]);

                let existing_ext_blocks = best_edid.len().saturating_sub(128) / 128;
                let orig_ext_flag = (block0[126] as usize).max(existing_ext_blocks);
                let new_ext_flag = (orig_ext_flag + 1).min(254) as u8;
                block0[126] = new_ext_flag;

                let sum0: u32 = block0[0..127].iter().map(|&b| b as u32).sum();
                block0[127] = ((256 - (sum0 % 256)) % 256) as u8;

                let (ov_key, _) = dp_key
                    .create_subkey("EDID_OVERRIDE")
                    .map_err(|e| format!("Failed to create EDID_OVERRIDE for {monitor_id}: {e}"))?;

                ov_key
                    .set_raw_value(
                        "0",
                        &RegValue {
                            vtype: REG_BINARY,
                            bytes: block0.to_vec(),
                        },
                    )
                    .map_err(|e| format!("Failed to write EDID_OVERRIDE block 0: {e}"))?;

                // Preserve existing extension blocks (e.g. block "1" CEA-861 with high refresh rate timings)
                for k in 1..=existing_ext_blocks {
                    let slice = &best_edid[k * 128..(k + 1) * 128];
                    let _ = ov_key.set_raw_value(
                        &k.to_string(),
                        &RegValue {
                            vtype: REG_BINARY,
                            bytes: slice.to_vec(),
                        },
                    );
                }

                // Write our new DisplayID 2.0 Tiled Topology extension block at the new extension index
                let displayid_block = build_displayid2_tiled_extension_block(&best_edid);
                let target_idx = new_ext_flag.to_string();
                ov_key
                    .set_raw_value(
                        &target_idx,
                        &RegValue {
                            vtype: REG_BINARY,
                            bytes: displayid_block.to_vec(),
                        },
                    )
                    .map_err(|e| format!("Failed to write EDID_OVERRIDE block {target_idx}: {e}"))?;
            } else {
                let _ = dp_key.delete_subkey_all("EDID_OVERRIDE");
            }
        }

        let (w, h) = extract_native_resolution_from_edid(&best_edid);
        modified_monitors.push(format!("{monitor_id} ({w}x{h})"));
    }

    if modified_monitors.is_empty() {
        return Err("No active monitors with valid EDID records found in registry.".to_string());
    }

    if enable {
        Ok(format!(
            "DisplayID 2.0 (Tiled Display Topology 1x1) written to EDID_OVERRIDE for: {}. Restart the display driver to apply immediately.",
            modified_monitors.join(", ")
        ))
    } else {
        Ok(format!(
            "EDID_OVERRIDE removed (factory EDID restored) for: {}. Restart the display driver to apply.",
            modified_monitors.join(", ")
        ))
    }
}

fn scan_display_edid_overrides() -> (bool, Vec<String>) {
    let hklm = RegKey::predef(HKEY_LOCAL_MACHINE);
    let mut found_override = false;
    let mut displays = Vec::new();

    if let Ok(disp_root) = hklm.open_subkey_with_flags(r"SYSTEM\CurrentControlSet\Enum\DISPLAY", KEY_READ) {
        for monitor_id in disp_root.enum_keys().flatten() {
            if monitor_id.starts_with("Default_Monitor") {
                continue;
            }
            if let Ok(mon_key) = disp_root.open_subkey_with_flags(&monitor_id, KEY_READ) {
                let mut mon_has_override = false;
                let mut mon_has_edid = false;
                let mut res_label = String::new();
                for inst_id in mon_key.enum_keys().flatten() {
                    let param_path = format!(r"{inst_id}\Device Parameters");
                    if let Ok(dp_key) = mon_key.open_subkey_with_flags(&param_path, KEY_READ) {
                        if let Ok(raw) = dp_key.get_raw_value("EDID") {
                            mon_has_edid = true;
                            let (w, h) = extract_native_resolution_from_edid(&raw.bytes);
                            res_label = format!(" {w}x{h}");
                        }
                        if let Ok(ov_key) = dp_key.open_subkey_with_flags("EDID_OVERRIDE", KEY_READ) {
                            if ov_key.get_raw_value("0").is_ok()
                                || ov_key.get_raw_value("1").is_ok()
                                || ov_key.get_raw_value("2").is_ok()
                            {
                                mon_has_override = true;
                                found_override = true;
                            }
                        }
                    }
                }
                if mon_has_override {
                    displays.push(format!("{monitor_id}{res_label} [DisplayID 2.0: Active]"));
                } else if mon_has_edid {
                    displays.push(format!("{monitor_id}{res_label} [Standard EDID]"));
                }
            }
        }
    }
    (found_override, displays)
}

pub fn download_and_launch_cru() -> Result<String, String> {
    let cru_dir = get_cru_dir();
    let _ = fs::create_dir_all(&cru_dir);
    let cru_exe = cru_dir.join("CRU.exe");

    if !cru_exe.exists() {
        let dl_script = format!(
            r#"
$ErrorActionPreference = 'Stop'
[Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12
$zip = Join-Path '{}' 'cru.zip'
$headers = @{{ 'User-Agent' = 'Mozilla/5.0 (Windows NT 10.0; Win64; x64)' }}
Invoke-WebRequest -Uri 'https://www.monitortests.com/download/cru/cru-1.5.3.zip' -Headers $headers -OutFile $zip
Expand-Archive -Path $zip -DestinationPath '{}' -Force
"#,
            cru_dir.display(),
            cru_dir.display()
        );
        let out = Command::new("powershell")
            .args(["-NoProfile", "-NonInteractive", "-Command", &dl_script])
            .creation_flags(CREATE_NO_WINDOW)
            .output()
            .map_err(|e| format!("Failed to download CRU: {e}"))?;

        if !out.status.success() && !cru_exe.exists() {
            return Err(format!(
                "Automatic CRU download failed (you can use the built-in DisplayID 2.0 toggle directly without CRU): {}",
                String::from_utf8_lossy(&out.stderr).lines().next().unwrap_or("network error")
            ));
        }
    }

    if cru_exe.exists() {
        Command::new(&cru_exe)
            .current_dir(&cru_dir)
            .spawn()
            .map_err(|e| format!("Failed to launch CRU.exe: {e}"))?;
        Ok("CRU.exe launched (you can also toggle DisplayID 2.0 directly in this card without CRU).".to_string())
    } else {
        Err("CRU.exe not found in tools/cru directory.".to_string())
    }
}

/// Restarts the active display driver (`pnputil /restart-device` or `restart64.exe -q`)
/// in a detached background helper after the current process cleanly releases its pre-reset
/// OpenGL/DirectX render context, and automatically reopens `league-unlagged.exe --driver-restarted`
/// as soon as the WDDM display driver finishes reloading.
pub fn run_cru_restart64(gpu_pnp_id: &str) -> Result<String, String> {
    let exe_path = std::env::current_exe()
        .map_err(|e| format!("Failed to locate application executable: {e}"))?;
    let work_dir = exe_path
        .parent()
        .unwrap_or_else(|| std::path::Path::new("."))
        .to_path_buf();

    let cru_dir = get_cru_dir();
    let restart_exe = cru_dir.join("restart64.exe");
    let pid = std::process::id();

    let exe_escaped = exe_path.display().to_string().replace('\'', "''");
    let work_escaped = work_dir.display().to_string().replace('\'', "''");
    let pnp_escaped = gpu_pnp_id.trim().replace('\'', "''");
    let restart_escaped = restart_exe.display().to_string().replace('\'', "''");

    let script = format!(
        r#"
$ErrorActionPreference = 'SilentlyContinue'
Wait-Process -Id {pid} -Timeout 3 -ErrorAction SilentlyContinue
$restarted = $false
if ('{pnp_escaped}' -ne '') {{
    $p = Start-Process -FilePath 'pnputil.exe' -ArgumentList '/restart-device', '{pnp_escaped}' -WindowStyle Hidden -Wait -PassThru
    if ($p.ExitCode -eq 0) {{ $restarted = $true }}
}}
if (-not $restarted -and (Test-Path -LiteralPath '{restart_escaped}')) {{
    Start-Process -FilePath '{restart_escaped}' -ArgumentList '-q' -WindowStyle Hidden -Wait
}}
Start-Sleep -Milliseconds 900
Start-Process -FilePath '{exe_escaped}' -WorkingDirectory '{work_escaped}' -ArgumentList '--driver-restarted'
"#
    );

    Command::new("powershell")
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-WindowStyle",
            "Hidden",
            "-Command",
            &script,
        ])
        .creation_flags(CREATE_NO_WINDOW)
        .spawn()
        .map_err(|e| format!("Failed to launch display driver restart helper: {e}"))?;

    std::process::exit(0);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_displayid2_tiled_extension_block_checksums_and_structure() {
        let mut fake_edid = [0u8; 128];
        // Set 1920x1080 in Detailed Timing Descriptor #1 (bytes 54..72)
        fake_edid[54] = 0x02;
        fake_edid[55] = 0x3A;
        fake_edid[56] = 0x80; // 1920 & 0xFF = 0x80
        fake_edid[58] = 0x70; // (1920 >> 8) << 4 = 0x70
        fake_edid[59] = 0x38; // 1080 & 0xFF = 0x38
        fake_edid[61] = 0x40; // (1080 >> 8) << 4 = 0x40

        assert_eq!(extract_native_resolution_from_edid(&fake_edid), (1920, 1080));

        let ext = build_displayid2_tiled_extension_block(&fake_edid);
        assert_eq!(ext[0], 0x70); // DisplayID extension tag
        assert_eq!(ext[1], 0x20); // DisplayID v2.0
        assert_eq!(ext[2], 25);   // 25 payload bytes in section
        assert_eq!(ext[5], 0x28); // Tiled Display Topology tag (0x28)
        assert_eq!(ext[7], 22);   // 22 bytes in tiled block
        // 1919 = 0x077F, 1079 = 0x0437
        assert_eq!(ext[12], 0x7F);
        assert_eq!(ext[13], 0x07);
        assert_eq!(ext[14], 0x37);
        assert_eq!(ext[15], 0x04);

        // Verify DisplayID inner section checksum (sum of bytes 1..=30 mod 256 == 0)
        let inner_sum: u32 = ext[1..=30].iter().map(|&b| b as u32).sum();
        assert_eq!(inner_sum % 256, 0);

        // Verify outer 128-byte EDID block checksum (sum of bytes 0..=127 mod 256 == 0)
        let outer_sum: u32 = ext.iter().map(|&b| b as u32).sum();
        assert_eq!(outer_sum % 256, 0);
    }
}
