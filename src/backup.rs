//! Backup & 1-Click Rollback Engine.
//! Automatically snapshots original system states to `backup_original_state.json` before changes,
//! creates Windows System Restore points, and restores all settings in 1 click.

use crate::hardware::{SystemHardwareReport, CREATE_NO_WINDOW};
use crate::tweaks::{gpu, interrupts, network, scheduler, windows_reg};
use serde::{Deserialize, Serialize};
use std::fs;
use std::os::windows::process::CommandExt;
use std::path::PathBuf;
use std::process::Command;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemBackupSnapshot {
    pub captured_at: String,
    pub game_mode_on: bool,
    pub windowed_opt_on: bool,
    pub hags_disabled: bool,
    pub no_lazy_mode_on: bool,
    pub system_responsiveness: u32,
    pub network_throttling_index: u32,
    pub global_timer_requests_on: bool,
    pub dynamic_tick_disabled: bool,
    pub short_thread_policy_index: u32,
    pub uro_disabled: bool,
    pub interrupt_moderation_disabled: bool,
    pub amd_ulps_disabled: bool,
    pub amd_deep_sleep_disabled: bool,
    pub amd_antilag_enabled: bool,
    pub amd_shader_cache_always_on: bool,
    pub amd_flip_queue_1: bool,
}

pub fn backup_file_path() -> PathBuf {
    std::env::current_dir()
        .unwrap_or_else(|_| PathBuf::from("."))
        .join("backup_original_state.json")
}

impl SystemBackupSnapshot {
    pub fn ensure_initial_snapshot(
        win_state: &windows_reg::WindowsRegState,
        sched_state: &scheduler::SchedulerState,
        net_state: &network::NetworkState,
        gpu_state: &gpu::GpuTweaksState,
    ) -> Self {
        let path = backup_file_path();
        if let Ok(content) = fs::read_to_string(&path) {
            if let Ok(existing) = serde_json::from_str::<SystemBackupSnapshot>(&content) {
                return existing;
            }
        }

        let snap = SystemBackupSnapshot {
            captured_at: "Initial System Baseline Snapshot".to_string(),
            game_mode_on: win_state.game_mode_on,
            windowed_opt_on: win_state.windowed_opt_on,
            hags_disabled: win_state.hags_disabled,
            no_lazy_mode_on: win_state.no_lazy_mode_on,
            system_responsiveness: win_state.current_system_responsiveness,
            network_throttling_index: win_state.current_network_throttling,
            global_timer_requests_on: win_state.global_timer_requests_on,
            dynamic_tick_disabled: win_state.dynamic_tick_disabled,
            short_thread_policy_index: sched_state.current_short_thread_policy,
            uro_disabled: net_state.uro_disabled,
            interrupt_moderation_disabled: net_state.interrupt_moderation_disabled,
            amd_ulps_disabled: gpu_state.amd_ulps_disabled,
            amd_deep_sleep_disabled: gpu_state.amd_deep_sleep_disabled,
            amd_antilag_enabled: gpu_state.amd_antilag_enabled,
            amd_shader_cache_always_on: gpu_state.amd_shader_cache_always_on,
            amd_flip_queue_1: gpu_state.amd_flip_queue_1,
        };

        if let Ok(json) = serde_json::to_string_pretty(&snap) {
            let _ = fs::write(&path, json);
        }
        snap
    }

    pub fn restore_all(&self, hw: &SystemHardwareReport) -> Vec<String> {
        let mut logs = Vec::new();

        if let Ok(m) = windows_reg::set_game_mode(self.game_mode_on) {
            logs.push(m);
        }
        if let Ok(m) = windows_reg::set_windowed_optimizations(self.windowed_opt_on, &hw.league_exe_paths) {
            logs.push(m);
        }
        if let Ok(m) = windows_reg::set_hags_disabled(self.hags_disabled) {
            logs.push(m);
        }
        if let Ok(m) = windows_reg::set_no_lazy_mode(self.no_lazy_mode_on) {
            logs.push(m);
        }
        if let Ok(m) = windows_reg::set_system_responsiveness(self.system_responsiveness) {
            logs.push(m);
        }
        if let Ok(m) = windows_reg::set_network_throttling_index(self.network_throttling_index) {
            logs.push(m);
        }
        if let Ok(m) = windows_reg::set_global_timer_resolution_requests(self.global_timer_requests_on) {
            logs.push(m);
        }
        if let Ok(m) = windows_reg::set_bcd_disable_dynamic_tick(self.dynamic_tick_disabled) {
            logs.push(m);
        }
        if let Ok(m) = scheduler::set_short_thread_policy(self.short_thread_policy_index) {
            logs.push(m);
        }
        if let Ok(m) = scheduler::set_ifeo_priorities(false) {
            logs.push(m);
        }
        if let Ok(m) = network::set_uro_disabled(self.uro_disabled) {
            logs.push(m);
        }

        // Remove PCI Interrupt Affinity overrides
        if !hw.gpu.pnp_device_id.is_empty() {
            if let Ok(m) = interrupts::set_device_interrupt_affinity(&hw.gpu.pnp_device_id, &hw.gpu.name, false, 0) {
                logs.push(m);
            }
        }
        for u in &hw.usb_controllers {
            if !u.pnp_device_id.is_empty() {
                if let Ok(m) = interrupts::set_device_interrupt_affinity(&u.pnp_device_id, &u.name, false, 0) {
                    logs.push(m);
                }
            }
        }
        for nvme in &hw.nvme_controllers {
            if !nvme.pnp_device_id.is_empty() {
                if let Ok(m) = interrupts::set_device_interrupt_affinity(&nvme.pnp_device_id, &nvme.name, false, 0) {
                    logs.push(m);
                }
            }
        }

        // Restore AMD GPU settings if AMD hardware is active
        if hw.gpu.vendor == crate::hardware::GpuVendor::Amd {
            let sub = &hw.gpu.driver_class_subkey;
            let _ = gpu::set_amd_ulps_disabled(sub, self.amd_ulps_disabled);
            let _ = gpu::set_amd_deep_sleep_disabled(sub, self.amd_deep_sleep_disabled);
            let _ = gpu::set_amd_antilag(sub, self.amd_antilag_enabled);
            let _ = gpu::set_amd_shader_cache_always_on(sub, self.amd_shader_cache_always_on);
            let _ = gpu::set_amd_flip_queue_size_1(sub, self.amd_flip_queue_1);
            logs.push("AMD Radeon driver parameters restored to baseline snapshot.".to_string());
        }

        logs.push("Complete system rollback to baseline snapshot (backup_original_state.json) finished.".to_string());
        logs
    }
}

pub fn create_windows_restore_point() -> Result<String, String> {
    let script = r#"
Enable-ComputerRestore -Drive "$env:SystemDrive\" -ErrorAction SilentlyContinue
Checkpoint-Computer -Description "League Unlagged Pre-Optimization Backup" -RestorePointType "MODIFY_SETTINGS"
"#;
    let out = Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-Command", script])
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .map_err(|e| format!("Failed to execute Checkpoint-Computer: {e}"))?;

    if out.status.success() {
        Ok("Windows System Restore Point ('League Unlagged Pre-Optimization Backup') created.".to_string())
    } else {
        Err(format!(
            "Failed to create System Restore Point (Windows enforces a 24-hour frequency limit or Administrator privileges are required): {}",
            String::from_utf8_lossy(&out.stderr).trim()
        ))
    }
}
