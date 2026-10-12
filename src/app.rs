//! Main GUI Application — System Latency & Hardware Topology Utility.
//! Features:
//! - Flat layered glass surfaces with generous 22px padding.
//! - Clean 2-column executive dashboard: Visual CPU Core Infographic on the left,
//!   3 high-level status cards (Peripherals, Interrupts & MSI, Process Isolation) on the right.
//! - Raw hardware IDs (`PCI\VEN_...`, `VID_...`, hex masks, registry paths) accessible via
//!   collapsible "Technical Diagnostics" spoilers and hover tooltips.
//! - Strictly 0% Unicode emojis and 100% technical English localization.

use crate::backup::{create_windows_restore_point, SystemBackupSnapshot};
use crate::hardware::{
    compute_optimal_cpu_plan_with_mode, CoreInfo, CoreIsolationMode, CpuArchClass, GpuVendor,
    SystemHardwareReport,
};
use crate::icons::{draw_lucide_icon, load_unlagged_brand_texture, LucideIcon};
use crate::theme::{
    apply_zen_visuals, configure_zen_fonts, zen_card_frame, zen_cta_button, zen_legend_swatch,
    zen_pill_button, zen_pill_button_with_icons, zen_semantic_badge, zen_semantic_badge_with_icon,
    zen_status_badge, zen_status_dot, zen_subtle_frame, zen_toggle_switch, zen_underline_tab,
    zen_window_control_button, BadgeTone, ZenThemeMode,
};
use crate::tweaks::{
    display_cru, gpu,
    interrupts::{self, MsiDeviceCategory},
    network, scheduler, windows_reg,
};
use egui::{
    pos2, Align, ComboBox, Context, Frame, Label, Layout, Margin, RichText, Rounding, ScrollArea,
    Sense, Stroke, TextureHandle, Ui, Vec2,
};

use serde::{Deserialize, Serialize};
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::Instant;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UiPreferences {
    pub theme_mode: ZenThemeMode,
    #[serde(default)]
    pub onboarding_completed: bool,
    #[serde(default)]
    pub league_folder_path: Option<String>,
    #[serde(default)]
    pub riot_client_folder_path: Option<String>,
}

impl Default for UiPreferences {
    fn default() -> Self {
        Self {
            theme_mode: ZenThemeMode::Light,
            onboarding_completed: false,
            league_folder_path: None,
            riot_client_folder_path: None,
        }
    }
}

impl UiPreferences {
    pub fn file_path() -> std::path::PathBuf {
        if let Ok(exe) = std::env::current_exe() {
            if let Some(dir) = exe.parent() {
                return dir.join("ui_preferences.json");
            }
        }
        std::env::current_dir()
            .unwrap_or_else(|_| std::path::PathBuf::from("."))
            .join("ui_preferences.json")
    }

    pub fn load_or_default() -> Self {
        if let Ok(content) = std::fs::read_to_string(Self::file_path()) {
            if let Ok(prefs) = serde_json::from_str::<UiPreferences>(&content) {
                return prefs;
            }
        }
        Self::default()
    }

    pub fn save(&self) {
        if let Ok(json) = serde_json::to_string_pretty(self) {
            let _ = std::fs::write(Self::file_path(), json);
        }
    }
}

pub struct RefreshedStates {
    pub is_admin: bool,
    pub win_state: windows_reg::WindowsRegState,
    pub sched_state: scheduler::SchedulerState,
    pub irq_state: interrupts::InterruptsState,
    pub net_state: network::NetworkState,
    pub gpu_state: gpu::GpuTweaksState,
    pub cru_state: display_cru::DisplayCruState,
}

impl RefreshedStates {
    pub fn collect(hw: &SystemHardwareReport) -> Self {
        Self {
            is_admin: scheduler::is_running_as_admin(),
            win_state: windows_reg::WindowsRegState::refresh(&hw.league_exe_paths),
            sched_state: scheduler::SchedulerState::refresh(&hw.cpu_plan),
            irq_state: interrupts::InterruptsState::refresh(hw),
            net_state: network::NetworkState::refresh(&hw.nic.name),
            gpu_state: gpu::GpuTweaksState::refresh(&hw.gpu),
            cru_state: display_cru::DisplayCruState::refresh(),
        }
    }
}

pub enum WorkerEvent {
    Step {
        step_label: String,
        progress: f32,
        log_line: Option<String>,
    },
    Finished {
        logs: Vec<String>,
        states: Box<RefreshedStates>,
    },
}

pub struct ActiveJob {
    pub title: String,
    pub step_label: String,
    pub target_progress: f32,
    pub visual_progress: f32,
    pub started_at: Instant,
    pub rx: Receiver<WorkerEvent>,
    pub show_driver_restart_action: bool,
    pub is_finished: bool,
    pub driver_restart_queued: bool,
    pub driver_restarted_status: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActiveTab {
    Overview,
    WindowsReg,
    SchedulerVanguard,
    Interrupts,
    NetworkGpu,
    TimerDisplayBackup,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SimCpuPreset {
    LiveHardware,
    Intel12100F,
    SixCoreNoSmt,
    Ryzen5600,
    Ryzen5800X3D,
    Ryzen5900X,
    Ryzen7950X3D,
    Intel13400F,
    Intel13600K,
    Intel14700K,
    Intel14900K,
}

impl SimCpuPreset {
    pub fn all() -> [SimCpuPreset; 11] {
        [
            SimCpuPreset::LiveHardware,
            SimCpuPreset::Intel12100F,
            SimCpuPreset::SixCoreNoSmt,
            SimCpuPreset::Ryzen5600,
            SimCpuPreset::Ryzen5800X3D,
            SimCpuPreset::Ryzen5900X,
            SimCpuPreset::Ryzen7950X3D,
            SimCpuPreset::Intel13400F,
            SimCpuPreset::Intel13600K,
            SimCpuPreset::Intel14700K,
            SimCpuPreset::Intel14900K,
        ]
    }

    pub fn label(self) -> &'static str {
        match self {
            SimCpuPreset::LiveHardware => "Live Hardware (Win32 Kernel Auto-Detection)",
            SimCpuPreset::Intel12100F => "Preview: 4 Cores / 8 Threads (Core i3 / Ryzen 3)",
            SimCpuPreset::SixCoreNoSmt => "Preview: 6 Cores / 6 Threads (No SMT / HT Disabled)",
            SimCpuPreset::Ryzen5600 => "Preview: 6 Cores / 12 Threads (Ryzen 5 / Core i5)",
            SimCpuPreset::Ryzen5800X3D => "Preview: 8 Cores / 16 Threads (Ryzen 7 / 7800X3D / 9800X3D)",
            SimCpuPreset::Ryzen5900X => "Preview: 12 Cores / 24 Threads Dual-CCD (Ryzen 9 5900X / 7900X3D)",
            SimCpuPreset::Ryzen7950X3D => "Preview: 16 Cores / 32 Threads Dual-CCD (Ryzen 9 7950X3D / 9950X3D)",
            SimCpuPreset::Intel13400F => "Preview: 10 Cores / 16 Threads Hybrid (6P + 4E, Core i5-13400F/14400F)",
            SimCpuPreset::Intel13600K => "Preview: 14 Cores / 20 Threads Hybrid (6P + 8E, Core i5-13600K/14600K)",
            SimCpuPreset::Intel14700K => "Preview: 20 Cores / 28 Threads Hybrid (8P + 12E, Core i7-14700K)",
            SimCpuPreset::Intel14900K => "Preview: 24 Cores / 32 Threads Hybrid (8P + 16E, Core i9-13900K/14900K)",
        }
    }
}

/// Configuration profile for the 1-Click Optimization action.
/// Hardware-safe tweaks and VESA DisplayID 2.0 Tiled Topology are enabled by default;
/// kernel timer overrides (0.505 ms) are disabled by default unless explicitly selected.
#[derive(Debug, Clone)]
pub struct OneClickConfig {
    pub windows_game_and_dx: bool,
    pub cpu_scheduler_and_power: bool,
    pub vanguard_and_client_isolation: bool,
    pub interrupt_affinity: bool,
    pub network_stack: bool,
    pub gpu_driver_tweaks: bool,
    /// Disabled by default so 1-Click does not alter global kernel timer resolution unless requested.
    pub timer_resolution_505: bool,
    /// Writes VESA DisplayID 2.0 Tiled Topology block to monitor EDID_OVERRIDE.
    pub displayid_topology_override: bool,
}

impl Default for OneClickConfig {
    fn default() -> Self {
        Self {
            windows_game_and_dx: true,
            cpu_scheduler_and_power: true,
            vanguard_and_client_isolation: true,
            interrupt_affinity: true,
            network_stack: true,
            gpu_driver_tweaks: true,
            timer_resolution_505: false,
            displayid_topology_override: true,
        }
    }
}

pub struct LeagueUnlaggedApp {
    pub theme_mode: ZenThemeMode,
    pub brand_logo: Option<TextureHandle>,
    pub is_admin: bool,
    pub show_admin_prompt_modal: bool,
    pub show_onboarding_screen: bool,
    pub onboarding_completed: bool,
    pub restore_point_created_in_session: bool,
    pub active_tab: ActiveTab,
    pub hw: SystemHardwareReport,
    pub preferred_lol_core_idx: usize,
    pub isolation_mode: CoreIsolationMode,
    pub league_folder_path: String,
    pub riot_client_folder_path: String,
    pub one_click_cfg: OneClickConfig,
    pub show_one_click_modal: bool,
    pub win_state: windows_reg::WindowsRegState,
    pub sched_state: scheduler::SchedulerState,
    pub irq_state: interrupts::InterruptsState,
    pub net_state: network::NetworkState,
    pub gpu_state: gpu::GpuTweaksState,
    pub cru_state: display_cru::DisplayCruState,
    pub backup: SystemBackupSnapshot,
    pub timer_505_enabled: bool,
    pub timer_desired_100ns: u32,
    pub nvidia_ultra_mode: bool,
    pub show_all_gpu_vendors: bool,
    pub show_tech_details: bool,
    pub show_log_drawer: bool,
    pub sim_preset: SimCpuPreset,
    pub active_job: Option<ActiveJob>,
    pub completion_toast: Option<(Instant, String)>,
    pub export_screenshot_frame: Option<u32>,
    pub logs: Vec<String>,
}

impl LeagueUnlaggedApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let export_screenshots = std::env::args().any(|a| a == "--export-screenshots");
        let prefs = UiPreferences::load_or_default();
        let theme_mode = if export_screenshots {
            ZenThemeMode::Light
        } else {
            prefs.theme_mode
        };
        configure_zen_fonts(&cc.egui_ctx);
        apply_zen_visuals(&cc.egui_ctx, theme_mode);
        let brand_logo = load_unlagged_brand_texture(&cc.egui_ctx);

        let is_admin = if export_screenshots {
            true
        } else {
            scheduler::is_running_as_admin()
        };
        let mut hw = SystemHardwareReport::scan_system();
        let preferred_lol_core_idx = hw.cpu_plan.dedicated_lol_core_idx;
        let isolation_mode = hw.cpu_plan.isolation_mode;

        let detected_league_folder = hw
            .league_exe_paths
            .first()
            .and_then(|p| std::path::Path::new(p).parent())
            .and_then(|p| p.parent())
            .map(|p| p.display().to_string())
            .unwrap_or_else(|| {
                if std::path::Path::new(r"D:\Riot Games\League of Legends").exists() {
                    r"D:\Riot Games\League of Legends".to_string()
                } else {
                    r"C:\Riot Games\League of Legends".to_string()
                }
            });

        let detected_riot_folder = if std::path::Path::new(r"D:\Riot Games\Riot Client").exists() {
            r"D:\Riot Games\Riot Client".to_string()
        } else {
            r"C:\Riot Games\Riot Client".to_string()
        };

        let league_folder_path = prefs
            .league_folder_path
            .clone()
            .filter(|s| !s.trim().is_empty())
            .unwrap_or(detected_league_folder);
        let riot_client_folder_path = prefs
            .riot_client_folder_path
            .clone()
            .filter(|s| !s.trim().is_empty())
            .unwrap_or(detected_riot_folder);

        let custom_game_exe = std::path::Path::new(&league_folder_path)
            .join("Game")
            .join("League of Legends.exe")
            .display()
            .to_string();
        if !hw.league_exe_paths.contains(&custom_game_exe) {
            hw.league_exe_paths.insert(0, custom_game_exe);
        }

        let win_state = windows_reg::WindowsRegState::refresh(&hw.league_exe_paths);
        let sched_state = scheduler::SchedulerState::refresh(&hw.cpu_plan);
        let irq_state = interrupts::InterruptsState::refresh(&hw);
        let net_state = network::NetworkState::refresh(&hw.nic.name);
        let gpu_state = gpu::GpuTweaksState::refresh(&hw.gpu);
        let cru_state = display_cru::DisplayCruState::refresh();
        let backup = SystemBackupSnapshot::ensure_initial_snapshot(
            &win_state,
            &sched_state,
            &net_state,
            &gpu_state,
        );

        let mut logs = Vec::new();
        logs.push(format!(
            "Hardware scan completed: {} ({} Cores / {} Threads) | {} | {}",
            hw.cpu_name, hw.physical_cores, hw.logical_threads, hw.gpu.name, hw.os_caption
        ));
        if !is_admin {
            logs.push("[WARNING] Running without Administrator privileges. Please relaunch as Administrator to modify HKLM registry keys and Process Lasso rules.".to_string());
        }
        logs.push(format!(
            "Process Lasso topology profile: {} (Dedicated Primary Core -> Core #{}, Game -> CPU [{}], Client/Vanguard -> CPU [{}])",
            hw.cpu_plan.arch_class.display_name(),
            hw.cpu_plan.dedicated_lol_core_idx,
            hw.cpu_plan.game_cpus_lasso,
            hw.cpu_plan.vanguard_cpus_lasso
        ));

        let onboarding_completed = if export_screenshots {
            true
        } else {
            prefs.onboarding_completed
        };

        Self {
            theme_mode,
            brand_logo,
            is_admin,
            show_admin_prompt_modal: !is_admin && !export_screenshots,
            show_onboarding_screen: !onboarding_completed,
            onboarding_completed,
            restore_point_created_in_session: false,
            active_tab: ActiveTab::Overview,
            hw,
            preferred_lol_core_idx,
            isolation_mode,
            league_folder_path,
            riot_client_folder_path,
            one_click_cfg: OneClickConfig::default(),
            show_one_click_modal: export_screenshots,
            win_state,
            sched_state,
            irq_state,
            net_state,
            gpu_state,
            cru_state,
            backup,
            timer_505_enabled: false,
            timer_desired_100ns: 5050,
            nvidia_ultra_mode: true,
            show_all_gpu_vendors: false,
            show_tech_details: false,
            show_log_drawer: false,
            sim_preset: SimCpuPreset::LiveHardware,
            active_job: None,
            completion_toast: None,
            export_screenshot_frame: if export_screenshots { Some(0) } else { None },
            logs,
        }
    }

    pub fn save_preferences(&self) {
        UiPreferences {
            theme_mode: self.theme_mode,
            onboarding_completed: self.onboarding_completed,
            league_folder_path: Some(self.league_folder_path.clone()),
            riot_client_folder_path: Some(self.riot_client_folder_path.clone()),
        }
        .save();
    }

    pub fn log(&mut self, msg: impl Into<String>) {
        let s = msg.into();
        self.logs.push(s);
        if self.logs.len() > 150 {
            self.logs.remove(0);
        }
    }

    pub fn sync_custom_folder_paths(&mut self) {
        let game_exe = std::path::Path::new(&self.league_folder_path)
            .join("Game")
            .join("League of Legends.exe");
        let game_exe_str = game_exe.display().to_string();
        if !self.hw.league_exe_paths.contains(&game_exe_str) {
            self.hw.league_exe_paths.insert(0, game_exe_str);
        }
        self.save_preferences();
    }

    pub fn pick_folder_dialog(initial_dir: &str) -> Option<String> {
        let script = format!(
            "Add-Type -AssemblyName System.Windows.Forms; $f = New-Object System.Windows.Forms.FolderBrowserDialog; $f.SelectedPath = '{}'; if ($f.ShowDialog() -eq 'OK') {{ Write-Output $f.SelectedPath }}",
            initial_dir.replace('\'', "''")
        );
        let out = std::process::Command::new("powershell")
            .args(["-NoProfile", "-Sta", "-Command", &script])
            .output()
            .ok()?;
        let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
        if s.is_empty() {
            None
        } else {
            Some(s)
        }
    }

    pub fn set_preferred_lol_core(&mut self, core_idx: usize) {
        if core_idx == 0 || core_idx >= self.hw.cores.len() {
            return;
        }
        self.preferred_lol_core_idx = core_idx;
        self.hw.cpu_plan = compute_optimal_cpu_plan_with_mode(
            &self.hw.cores,
            &self.hw.cpu_name,
            Some(core_idx),
            self.isolation_mode,
        );
        self.log(format!(
            "Dedicated primary core reassigned to Core #{} (Game -> CPU [{}], Client/Vanguard -> CPU [{}])",
            core_idx,
            self.hw.cpu_plan.game_cpus_lasso,
            self.hw.cpu_plan.vanguard_cpus_lasso
        ));
        let lasso_active = self.sched_state.process_lasso_rules_applied;
        if lasso_active {
            self.spawn_background_task(
                "Updating CPU Core Topology",
                format!("Rebinding Process Lasso rules for Primary Core #{core_idx}..."),
                move |_tx, hw| {
                    let mut logs = Vec::new();
                    if let Ok(m) = scheduler::apply_process_lasso_rules(&hw.cpu_plan) {
                        logs.push(m);
                    }
                    logs
                },
            );
        } else {
            self.refresh_all_states();
        }
    }

    pub fn set_core_isolation_mode(&mut self, mode: CoreIsolationMode) {
        if self.isolation_mode == mode {
            return;
        }
        self.isolation_mode = mode;
        self.hw.cpu_plan = compute_optimal_cpu_plan_with_mode(
            &self.hw.cores,
            &self.hw.cpu_name,
            Some(self.preferred_lol_core_idx),
            mode,
        );
        self.log(format!(
            "Core isolation mode switched to '{}': Game -> CPU [{}], Client/Vanguard -> CPU [{}]",
            mode.label(),
            self.hw.cpu_plan.game_cpus_lasso,
            self.hw.cpu_plan.vanguard_cpus_lasso
        ));
        let lasso_active = self.sched_state.process_lasso_rules_applied;
        if lasso_active {
            self.spawn_background_task(
                "Switching Core Isolation Mode",
                format!("Applying '{}' topology to Process Lasso...", mode.label()),
                move |_tx, hw| {
                    let mut logs = Vec::new();
                    if let Ok(m) = scheduler::apply_process_lasso_rules(&hw.cpu_plan) {
                        logs.push(m);
                    }
                    logs
                },
            );
        } else {
            self.refresh_all_states();
        }
    }

    pub fn spawn_background_task<F>(
        &mut self,
        title: impl Into<String>,
        initial_step: impl Into<String>,
        task_fn: F,
    ) where
        F: FnOnce(&Sender<WorkerEvent>, &SystemHardwareReport) -> Vec<String> + Send + 'static,
    {
        let (tx, rx) = mpsc::channel();
        let hw_clone = self.hw.clone();
        let title_str = title.into();
        let init_step_str = initial_step.into();

        std::thread::spawn(move || {
            let started = Instant::now();
            let logs = match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                task_fn(&tx, &hw_clone)
            })) {
                Ok(l) => l,
                Err(_) => vec!["[ERROR] Background worker encountered an unexpected panic and recovered safely.".to_string()],
            };
            let _ = tx.send(WorkerEvent::Step {
                step_label: "Verifying hardware & registry configuration...".to_string(),
                progress: 0.95,
                log_line: None,
            });
            let states = RefreshedStates::collect(&hw_clone);
            // Ensure at least 260ms so the progress overlay animates smoothly without a 1-frame flicker
            let elapsed = started.elapsed();
            if elapsed < std::time::Duration::from_millis(260) {
                std::thread::sleep(std::time::Duration::from_millis(260) - elapsed);
            }
            let _ = tx.send(WorkerEvent::Finished {
                logs,
                states: Box::new(states),
            });
        });

        self.active_job = Some(ActiveJob {
            title: title_str,
            step_label: init_step_str,
            target_progress: 0.25,
            visual_progress: 0.06,
            started_at: Instant::now(),
            rx,
            show_driver_restart_action: false,
            is_finished: false,
            driver_restart_queued: false,
            driver_restarted_status: None,
        });
    }

    pub fn refresh_all_states(&mut self) {
        // Update fast local registry states immediately (< 1ms) so switches reflect instantly,
        // and offload slow WMI/powercfg/PowerShell adapter queries to the background worker thread!
        self.win_state = windows_reg::WindowsRegState::refresh(&self.hw.league_exe_paths);
        self.irq_state = interrupts::InterruptsState::refresh(&self.hw);
        self.gpu_state = gpu::GpuTweaksState::refresh(&self.hw.gpu);
        self.cru_state = display_cru::DisplayCruState::refresh();

        self.spawn_background_task(
            "Applying System Configuration",
            "Synchronizing scheduler & driver states...",
            |_tx, _hw| Vec::new(),
        );
    }

    /// Count how many core optimizations are currently active for the summary status indicator.
    fn count_active_optimizations(&self) -> (usize, usize) {
        let mut active = 0usize;
        let total = 8usize;
        if self.win_state.game_mode_on {
            active += 1;
        }
        if self.win_state.windowed_opt_on {
            active += 1;
        }
        if self.win_state.hags_disabled {
            active += 1;
        }
        if self.win_state.no_lazy_mode_on {
            active += 1;
        }
        if self.sched_state.short_thread_policy_optimal {
            active += 1;
        }
        if self.sched_state.process_lasso_rules_applied {
            active += 1;
        }
        if self.net_state.uro_disabled {
            active += 1;
        }
        let gpu_irq_ok = self
            .irq_state
            .gpu_irq
            .as_ref()
            .map(|g| g.is_pinned_to_recommended || g.msi_supported)
            .unwrap_or(false);
        if gpu_irq_ok {
            active += 1;
        }
        (active, total)
    }

    fn compact_cpu_name(raw: &str) -> String {
        let mut s = raw.to_string();
        for token in [
            "16-Core Processor",
            "12-Core Processor",
            "8-Core Processor",
            "6-Core Processor",
            "4-Core Processor",
            "16-Core",
            "12-Core",
            "8-Core",
            "6-Core",
            "4-Core",
            "Processor",
            "(R)",
            "(TM)",
        ] {
            s = s.replace(token, "");
        }
        s.split_whitespace().collect::<Vec<_>>().join(" ")
    }

    fn compact_gpu_name(raw: &str) -> String {
        let mut s = raw.to_string();
        for token in ["(TM)", "(R)", "Graphics"] {
            s = s.replace(token, "");
        }
        s.split_whitespace().collect::<Vec<_>>().join(" ")
    }

    fn render_header(&mut self, ui: &mut Ui, ctx: &Context) {
        let p = self.theme_mode.palette();

        // Allocate a fixed 40px header row so Left Brand, Center Hardware Pill, and Right Controls share the exact vertical center
        let header_h = 40.0;
        let (header_rect, _) =
            ui.allocate_exact_size(Vec2::new(ui.available_width(), header_h), Sense::hover());
        let cy = header_rect.center().y;

        // 1. Paint Left Brand Logo + Title + Subtitle vertically centered around cy
        let mut left_cursor_x = header_rect.left();
        {
            let painter = ui.painter();
            if let Some(tex) = &self.brand_logo {
                let logo_rect = egui::Rect::from_center_size(
                    pos2(left_cursor_x + 18.0, cy),
                    Vec2::splat(36.0),
                );
                painter.image(
                    tex.id(),
                    logo_rect,
                    egui::Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
                    egui::Color32::WHITE,
                );
                left_cursor_x += 44.0;
            }

            let title_galley = painter.layout_no_wrap(
                "LEAGUE UNLAGGED".to_string(),
                egui::FontId::new(18.0, egui::FontFamily::Proportional),
                p.text_heading,
            );
            let sub_galley = painter.layout_no_wrap(
                "Input Latency & Topology Utility".to_string(),
                egui::FontId::new(11.2, egui::FontFamily::Proportional),
                p.text_muted,
            );
            let total_text_h = title_galley.size().y + 2.0 + sub_galley.size().y;
            let text_top = cy - total_text_h * 0.5;
            let block_w = title_galley.size().x.max(sub_galley.size().x);

            painter.galley(pos2(left_cursor_x, text_top), title_galley, p.text_heading);
            painter.galley(
                pos2(left_cursor_x, text_top + 20.0),
                sub_galley,
                p.text_muted,
            );
            left_cursor_x += block_w + 16.0;
        }

        // 2. Render Right-Aligned Action Buttons vertically centered inside header_rect
        let mut right_buttons_left_x = header_rect.right();
        ui.allocate_new_ui(egui::UiBuilder::new().max_rect(header_rect), |ui| {
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = 6.0;

                // Config button for 1-Click Optimization
                let cfg_chevron = if self.show_one_click_modal {
                    LucideIcon::ChevronUp
                } else {
                    LucideIcon::ChevronDown
                };
                if zen_pill_button_with_icons(
                    ui,
                    self.theme_mode,
                    Some(LucideIcon::Sliders),
                    "1-Click Profile",
                    Some(cfg_chevron),
                    self.show_one_click_modal,
                )
                .on_hover_text("Configure which modules are applied by 'Optimize (1-Click)'. Kernel Timer (0.505 ms) and EDID overrides are disabled by default.")
                .clicked()
                {
                    self.show_one_click_modal = !self.show_one_click_modal;
                }

                if zen_cta_button(
                    ui,
                    self.theme_mode,
                    Some(LucideIcon::Zap),
                    "Optimize (1-Click)",
                )
                .on_hover_text("Apply all selected hardware-adaptive optimizations for your system.")
                .clicked()
                {
                    self.apply_all_recommended();
                }

                let (theme_icon, theme_label) = match self.theme_mode {
                    ZenThemeMode::Dark => (LucideIcon::Sun, "Light"),
                    ZenThemeMode::Light => (LucideIcon::Moon, "Dark"),
                };
                if zen_pill_button_with_icons(
                    ui,
                    self.theme_mode,
                    Some(theme_icon),
                    theme_label,
                    None,
                    false,
                )
                .clicked()
                {
                    self.theme_mode = match self.theme_mode {
                        ZenThemeMode::Dark => ZenThemeMode::Light,
                        ZenThemeMode::Light => ZenThemeMode::Dark,
                    };
                    apply_zen_visuals(ctx, self.theme_mode);
                    self.save_preferences();
                }

                let wiz_resp = zen_pill_button(
                    ui,
                    self.theme_mode,
                    "Setup Wizard",
                    self.show_onboarding_screen,
                )
                .on_hover_text("Open the Initial System Setup screen (Restore Point & Riot/League installation directories).");
                right_buttons_left_x = wiz_resp.rect.left();
                if wiz_resp.clicked() {
                    self.show_onboarding_screen = !self.show_onboarding_screen;
                }

                if !self.is_admin {
                    let adm_resp = zen_pill_button_with_icons(
                        ui,
                        self.theme_mode,
                        Some(LucideIcon::Shield),
                        "Run as Admin",
                        None,
                        true,
                    )
                    .on_hover_text("Relaunch with Administrator privileges (required for HKLM registry writes: HAGS, MSI Mode, Interrupt Affinity, IFEO, DisplayID 2.0).");
                    right_buttons_left_x = adm_resp.rect.left();
                    if adm_resp.clicked() {
                        if let Err(e) = scheduler::relaunch_as_admin() {
                            self.log(format!("[ERROR] {e}"));
                        } else {
                            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                        }
                    }
                }
            });
        });

        // 3. Paint Hardware Summary Pill cleanly in the available gap between Left Brand and Right Buttons (never overlapping!)
        {
            let avail_gap = right_buttons_left_x - left_cursor_x - 12.0;
            if avail_gap > 160.0 {
                let painter = ui.painter();
                let (active_cnt, total_cnt) = self.count_active_optimizations();
                let dot_col = if active_cnt >= 6 {
                    p.emerald_optimal
                } else {
                    p.gold_priority
                };
                let short_cpu = Self::compact_cpu_name(&self.hw.cpu_name);
                let short_gpu = Self::compact_gpu_name(&self.hw.gpu.name);
                let hw_text = format!(
                    "{short_cpu} ({}C/{}T)  |  {short_gpu}",
                    self.hw.physical_cores, self.hw.logical_threads
                );
                let cnt_text = format!("({active_cnt}/{total_cnt})");

                let hw_galley = painter.layout_no_wrap(
                    hw_text,
                    egui::FontId::new(11.8, egui::FontFamily::Proportional),
                    p.text_body,
                );
                let cnt_galley = painter.layout_no_wrap(
                    cnt_text,
                    egui::FontId::new(11.2, egui::FontFamily::Proportional),
                    p.text_muted,
                );

                let pill_w = 26.0 + hw_galley.size().x + 6.0 + cnt_galley.size().x + 14.0;
                if pill_w <= avail_gap {
                    let pill_h = 28.0;
                    let pill_rect = egui::Rect::from_center_size(
                        pos2(left_cursor_x + pill_w * 0.5, cy),
                        Vec2::new(pill_w, pill_h),
                    );
                    painter.rect_filled(pill_rect, Rounding::same(14.0), p.subtle_glass_panel);

                    let dot_center = pos2(pill_rect.left() + 14.0, cy);
                    painter.circle_filled(dot_center, 6.5, dot_col.gamma_multiply(0.18));
                    painter.circle_filled(dot_center, 3.8, dot_col);

                    let text_x = pill_rect.left() + 25.0;
                    painter.galley(
                        pos2(text_x, cy - hw_galley.size().y * 0.5),
                        hw_galley.clone(),
                        p.text_body,
                    );
                    painter.galley(
                        pos2(
                            text_x + hw_galley.size().x + 6.0,
                            cy - cnt_galley.size().y * 0.5,
                        ),
                        cnt_galley,
                        p.text_muted,
                    );
                }
            }
        }

        // Expandable Smart 1-Click Configuration Drawer
        if self.show_one_click_modal {
            ui.add_space(10.0);
            zen_card_frame(self.theme_mode, false).show(ui, |ui| {
                ui.horizontal(|ui| {
                    draw_lucide_icon(ui, LucideIcon::Sliders, 15.5, p.gold_priority);
                    ui.label(
                        RichText::new("1-Click Optimization Profile Configuration")
                            .size(13.8)
                            .strong()
                            .color(p.text_heading),
                    );
                    ui.label(
                        RichText::new("| Kernel Timer (0.505 ms) and DisplayID 2.0 are opt-in by default")
                            .size(11.5)
                            .color(p.text_muted),
                    );
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if zen_pill_button_with_icons(
                            ui,
                            self.theme_mode,
                            Some(LucideIcon::RotateCcw),
                            "Revert All Tweaks",
                            None,
                            false,
                        )
                        .clicked()
                        {
                            self.disable_all_tweaks();
                        }
                    });
                });
                ui.add_space(8.0);
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing = Vec2::new(16.0, 8.0);
                    ui.checkbox(
                        &mut self.one_click_cfg.windows_game_and_dx,
                        RichText::new("Windows, DX Flip & HAGS Disabled").size(12.0).color(p.text_body),
                    );
                    ui.checkbox(
                        &mut self.one_click_cfg.cpu_scheduler_and_power,
                        RichText::new("CPU Scheduler & Bus Power Management").size(12.0).color(p.text_body),
                    );
                    ui.checkbox(
                        &mut self.one_click_cfg.vanguard_and_client_isolation,
                        RichText::new("Game, Client & Vanguard Core Isolation").size(12.0).color(p.text_body),
                    );
                    ui.checkbox(
                        &mut self.one_click_cfg.interrupt_affinity,
                        RichText::new("MSI Mode & Interrupt Affinity (GPU / USB / NVMe / NIC)").size(12.0).color(p.text_body),
                    );
                    ui.checkbox(
                        &mut self.one_click_cfg.network_stack,
                        RichText::new("Network Stack (URO Disabled & NIC Buffers)").size(12.0).color(p.text_body),
                    );
                    ui.checkbox(
                        &mut self.one_click_cfg.gpu_driver_tweaks,
                        RichText::new("GPU Driver (Anti-Lag / Flip Queue / ULPS / DeepSleep)").size(12.0).color(p.text_body),
                    );
                    ui.checkbox(
                        &mut self.one_click_cfg.timer_resolution_505,
                        RichText::new("0.505 ms Kernel Timer + Disable Dynamic Tick (Optional)").size(12.0).color(p.gold_priority),
                    );
                    ui.checkbox(
                        &mut self.one_click_cfg.displayid_topology_override,
                        RichText::new("DisplayID 2.0 Tiled Topology Override (Optional)").size(12.0).color(p.gold_priority),
                    );
                });
            });
        }

        if !self.show_onboarding_screen {
            ui.add_space(14.0);

            // Horizontal Navigation Tab Bar
            ui.horizontal(|ui| {
                let tabs = [
                    (ActiveTab::Overview, "System Overview"),
                    (ActiveTab::WindowsReg, "OS & Registry"),
                    (ActiveTab::SchedulerVanguard, "Scheduler & Process Isolation"),
                    (ActiveTab::Interrupts, "Interrupts & MSI"),
                    (ActiveTab::NetworkGpu, "Network & GPU Driver"),
                    (ActiveTab::TimerDisplayBackup, "Timer, Display & Rollback"),
                ];
                for (tab, title) in tabs {
                    if zen_underline_tab(ui, self.theme_mode, title, self.active_tab == tab)
                        .clicked()
                    {
                        self.active_tab = tab;
                    }
                    ui.add_space(4.0);
                }
            });
        } else {
            ui.add_space(10.0);
        }

        // Subtle full-width divider line under the Header / Tab Bar
        let rect = ui.available_rect_before_wrap();
        ui.painter().line_segment(
            [pos2(rect.left(), rect.top()), pos2(rect.right(), rect.top())],
            Stroke::new(1.0_f32, p.border_subtle),
        );
        ui.add_space(18.0);
    }

    fn apply_all_recommended(&mut self) {
        let cfg = self.one_click_cfg.clone();
        let timer_desired = self.timer_desired_100ns;
        let nv_ultra = self.nvidia_ultra_mode;
        if cfg.timer_resolution_505 {
            self.timer_505_enabled = true;
        }

        self.spawn_background_task(
            "Applying 1-Click Optimization Profile",
            "Checking required external utilities (Process Lasso / Driver Tools)...",
            move |tx, hw| {
                let mut logs = vec!["=== Executing 1-Click Hardware Optimization Profile ===".to_string()];

                // 1. Pre-flight: Download & install any required external utilities FIRST while network is untouched
                if cfg.vanguard_and_client_isolation && !scheduler::is_process_lasso_installed() {
                    let _ = tx.send(WorkerEvent::Step {
                        step_label: "Downloading & installing Process Lasso via winget...".to_string(),
                        progress: 0.10,
                        log_line: None,
                    });
                    match scheduler::install_process_lasso_via_winget() {
                        Ok(m) => logs.push(m),
                        Err(e) => logs.push(format!("[INFO] {e}")),
                    }
                }

                // 2. Windows Core, DirectX Flip Model, HAGS & MMCSS
                if cfg.windows_game_and_dx {
                    let _ = tx.send(WorkerEvent::Step {
                        step_label: "Configuring Windows Game Mode, DX Flip Model & MMCSS...".to_string(),
                        progress: 0.22,
                        log_line: None,
                    });
                    for res in [
                        windows_reg::set_game_mode(true),
                        windows_reg::set_windowed_optimizations(true, &hw.league_exe_paths),
                        windows_reg::set_hags_disabled(true),
                        windows_reg::set_no_lazy_mode(true),
                        windows_reg::set_system_responsiveness(10),
                        windows_reg::set_network_throttling_index(10),
                    ] {
                        match res {
                            Ok(m) => logs.push(m),
                            Err(e) => logs.push(format!("[ERROR] {e}")),
                        }
                    }
                }

                // 3. CPU Short-Thread Policy & PCIe/USB Power Management
                if cfg.cpu_scheduler_and_power {
                    let _ = tx.send(WorkerEvent::Step {
                        step_label: "Applying CPU Short-Thread Policy & PCIe/USB Power Rules...".to_string(),
                        progress: 0.36,
                        log_line: None,
                    });
                    for res in [
                        scheduler::set_short_thread_policy(hw.cpu_plan.short_thread_policy_index),
                        scheduler::set_power_saving_features_disabled(true),
                    ] {
                        match res {
                            Ok(m) => logs.push(m),
                            Err(e) => logs.push(format!("[ERROR] {e}")),
                        }
                    }
                }

                // 4. Process Lasso CPU Affinity & Vanguard Isolation
                if cfg.vanguard_and_client_isolation {
                    let _ = tx.send(WorkerEvent::Step {
                        step_label: "Applying Process Lasso CPU Affinity & Vanguard Isolation...".to_string(),
                        progress: 0.50,
                        log_line: None,
                    });
                    for res in [
                        scheduler::set_ifeo_priorities(true),
                        scheduler::apply_process_lasso_rules(&hw.cpu_plan),
                    ] {
                        match res {
                            Ok(m) => logs.push(m),
                            Err(e) => logs.push(format!("[ERROR] {e}")),
                        }
                    }
                }

                // 5. GPU Driver Low-Latency Parameters (runs before Network Stack in case NVIDIA Profile Inspector is downloaded)
                if cfg.gpu_driver_tweaks {
                    let _ = tx.send(WorkerEvent::Step {
                        step_label: "Applying GPU Driver Low-Latency Parameters...".to_string(),
                        progress: 0.64,
                        log_line: None,
                    });
                    if hw.gpu.vendor == GpuVendor::Amd {
                        let sub = &hw.gpu.driver_class_subkey;
                        for res in [
                            gpu::set_amd_ulps_disabled(sub, true),
                            gpu::set_amd_deep_sleep_disabled(sub, true),
                            gpu::set_amd_antilag(sub, true),
                            gpu::set_amd_shader_cache_always_on(sub, true),
                            gpu::set_amd_flip_queue_size_1(sub, true),
                        ] {
                            match res {
                                Ok(m) => logs.push(m),
                                Err(e) => logs.push(format!("[ERROR] {e}")),
                            }
                        }
                    } else if hw.gpu.vendor == GpuVendor::Nvidia {
                        if let Ok(m) = gpu::apply_nvidia_profile(true, nv_ultra) {
                            logs.push(m);
                        }
                    }
                }

                // 6. MSI Mode & Interrupt Affinity
                if cfg.interrupt_affinity {
                    let _ = tx.send(WorkerEvent::Step {
                        step_label: "Configuring MSI Mode & Interrupt Affinity (GPU/USB/NVMe/NIC)...".to_string(),
                        progress: 0.78,
                        log_line: None,
                    });
                    logs.extend(Self::apply_interrupts_batch_sync(hw, true));
                }

                // 7. Optional Kernel Timer & DisplayID 2.0 Topology Override
                if cfg.timer_resolution_505 {
                    let _ = windows_reg::set_global_timer_resolution_requests(true);
                    if let Ok(m) = scheduler::set_kernel_timer_resolution(true, timer_desired) {
                        logs.push(m);
                    }
                    let _ = windows_reg::set_bcd_disable_dynamic_tick(true);
                } else {
                    logs.push("[INFO] 0.505 ms Kernel Timer and Dynamic Tick skipped (disabled in 1-Click profile to preserve default OS timer behavior).".to_string());
                }

                if cfg.displayid_topology_override {
                    match display_cru::set_displayid2_tiled_topology_override(true) {
                        Ok(m) => logs.push(m),
                        Err(e) => logs.push(format!("[ERROR] {e}")),
                    }
                }

                // 8. Network Stack (URO Disabled, Interrupt Moderation & EEE) — executed LAST with a single adapter restart
                if cfg.network_stack {
                    let _ = tx.send(WorkerEvent::Step {
                        step_label: "Optimizing Network Stack (URO Disabled & NIC Moderation)...".to_string(),
                        progress: 0.90,
                        log_line: None,
                    });
                    logs.extend(network::apply_network_stack_batch(&hw.nic.name, true));
                }

                logs
            },
        );
        if let Some(job) = &mut self.active_job {
            job.show_driver_restart_action = true;
        }
    }

    fn disable_all_tweaks(&mut self) {
        let backup_clone = self.backup.clone();
        let was_timer_on = self.timer_505_enabled;
        let timer_desired = self.timer_desired_100ns;
        self.timer_505_enabled = false;

        self.spawn_background_task(
            "Restoring System Baseline Snapshot",
            "Reverting Process Lasso, IFEO & Kernel Timer rules...",
            move |tx, hw| {
                let mut logs = vec!["=== Reverting All Active Tweaks to Baseline ===".to_string()];
                if let Ok(m) = scheduler::remove_process_lasso_rules() {
                    logs.push(m);
                }
                let _ = scheduler::set_ifeo_priorities(false);
                if was_timer_on {
                    let _ = scheduler::set_kernel_timer_resolution(false, timer_desired);
                }
                let _ = display_cru::set_displayid2_tiled_topology_override(false);

                let _ = tx.send(WorkerEvent::Step {
                    step_label: "Removing PCI Interrupt Affinity overrides...".to_string(),
                    progress: 0.45,
                    log_line: None,
                });
                logs.extend(Self::apply_interrupts_batch_sync(hw, false));

                let _ = tx.send(WorkerEvent::Step {
                    step_label: "Restoring baseline registry & driver settings...".to_string(),
                    progress: 0.75,
                    log_line: None,
                });
                logs.extend(backup_clone.restore_all(hw));
                logs
            },
        );
    }

    /// 2-Column Executive Overview Screen:
    /// - Left Column: Visual CPU Core Topology Infographic (System -> Client/Vanguard -> Primary Game Core)
    /// - Right Column: 3 Executive Status Cards (Peripherals, Interrupts & MSI, Process Isolation)
    fn render_overview_tab(&mut self, ui: &mut Ui) {
        let p = self.theme_mode.palette();
        let total_w = ui.available_width();
        let col_gap = 20.0;
        let left_col_w = ((total_w - col_gap) * 0.50).floor().max(440.0);
        let right_col_w = (total_w - col_gap - left_col_w).floor().max(420.0);
        let mut clicked_preferred_core: Option<usize> = None;
        let mut clicked_mode: Option<CoreIsolationMode> = None;

        ui.horizontal_top(|ui| {
            ui.spacing_mut().item_spacing.x = col_gap;

            // ================= LEFT COLUMN: VISUAL CPU CORE INFOGRAPHIC =================
            ui.allocate_ui_with_layout(
                Vec2::new(left_col_w, 0.0),
                Layout::top_down(Align::Min),
                |left_ui| {
                    left_ui.set_width(left_col_w);
                    left_ui.set_max_width(left_col_w);

                    let card_inner_w = (left_col_w - 46.0).max(360.0);
                    zen_card_frame(self.theme_mode, false).show(left_ui, |ui| {
                        ui.set_width(card_inner_w);
                        ui.set_max_width(card_inner_w);

                        ui.horizontal(|ui| {
                            let header_left_w = (card_inner_w - 135.0).max(180.0);
                            ui.vertical(|ui| {
                                ui.set_max_width(header_left_w);
                                ui.horizontal(|ui| {
                                    ui.spacing_mut().item_spacing.x = 7.0;
                                    draw_lucide_icon(ui, LucideIcon::Cpu, 17.0, p.text_heading);
                                    ui.label(
                                        RichText::new("CPU Core Topology & Thread Allocation")
                                            .size(15.5)
                                            .strong()
                                            .color(p.text_heading),
                                    );
                                });
                                ui.label(
                                    RichText::new(format!(
                                        "{}  |  Dedicated Primary Core: Core #{}",
                                        self.hw.cpu_plan.arch_class.display_name(),
                                        self.hw.cpu_plan.dedicated_lol_core_idx
                                    ))
                                    .size(11.8)
                                    .color(p.text_muted),
                                );
                            });

                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                zen_semantic_badge_with_icon(
                                    ui,
                                    self.theme_mode,
                                    BadgeTone::Priority,
                                    Some(LucideIcon::Crosshair),
                                    "PRIMARY CORE",
                                );
                            });
                        });

                        ui.add_space(10.0);

                        // Mode Selector: Dedicated Primary Core vs Strict Zero-Overlap
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = 6.0;
                            ui.label(
                                RichText::new("Topology Mode:")
                                    .size(11.5)
                                    .strong()
                                    .color(p.text_secondary),
                            );
                            if zen_pill_button(
                                ui,
                                self.theme_mode,
                                "Dedicated Primary Core (SMT Partitioned)",
                                self.isolation_mode == CoreIsolationMode::DedicatedPrimaryCore,
                            )
                            .on_hover_text("Reserves the selected primary core 100% exclusively for the main render thread (0% Client/Vanguard). On adjacent cores, physical threads run game workers while SMT threads host Client/Vanguard.")
                            .clicked()
                            {
                                clicked_mode = Some(CoreIsolationMode::DedicatedPrimaryCore);
                            }
                            if zen_pill_button(
                                ui,
                                self.theme_mode,
                                "Strict Core Isolation (Zero Overlap)",
                                self.isolation_mode == CoreIsolationMode::StrictZeroOverlap,
                            )
                            .on_hover_text("Complete physical separation: dedicated game cores run only League of Legends.exe, while background cores run only Client and Vanguard processes.")
                            .clicked()
                            {
                                clicked_mode = Some(CoreIsolationMode::StrictZeroOverlap);
                            }
                        });

                        ui.add_space(10.0);

                        let (sim_cores, sim_name) = self.get_simulated_or_live_cores();
                        let sim_plan = if self.sim_preset == SimCpuPreset::LiveHardware {
                            self.hw.cpu_plan.clone()
                        } else {
                            compute_optimal_cpu_plan_with_mode(
                                &sim_cores,
                                &sim_name,
                                None,
                                self.isolation_mode,
                            )
                        };

                        // Single-row Visual Infographic Legend with vector swatches
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = 6.0;
                            zen_legend_swatch(ui, p.text_muted);
                            ui.label(
                                RichText::new("OS (Core 0)")
                                    .size(11.2)
                                    .color(p.text_secondary),
                            );
                            ui.add_space(6.0);
                            zen_legend_swatch(ui, p.emerald_optimal);
                            ui.label(
                                RichText::new("Game Thread (Physical)")
                                    .size(11.2)
                                    .color(p.text_secondary),
                            );
                            ui.add_space(6.0);
                            zen_legend_swatch(ui, p.cyan_client);
                            ui.label(
                                RichText::new("Client & Vanguard")
                                    .size(11.2)
                                    .color(p.text_secondary),
                            );
                            ui.add_space(6.0);
                            draw_lucide_icon(ui, LucideIcon::Crosshair, 12.0, p.gold_priority);
                            ui.label(
                                RichText::new("Primary Render Core (Click)")
                                    .size(11.2)
                                    .strong()
                                    .color(p.gold_priority),
                            );
                        });

                        ui.add_space(10.0);

                        // Draw Graphical CPU Silicon Die Blocks strictly bounded inside card_inner_w
                        let cols_per_row = if sim_cores.len() > 8 { 4 } else { 3 };
                        let spacing = 10.0;
                        let tile_outer_w = ((card_inner_w - spacing * (cols_per_row as f32 - 1.0))
                            / cols_per_row as f32)
                            .floor();

                        for chunk in sim_cores.chunks(cols_per_row) {
                            ui.horizontal(|ui| {
                                ui.spacing_mut().item_spacing.x = spacing;
                                for core in chunk {
                                    if self.render_graphical_core_tile(
                                        ui,
                                        core,
                                        &sim_plan,
                                        tile_outer_w,
                                    ) {
                                        clicked_preferred_core = Some(core.core_index);
                                    }
                                }
                            });
                            ui.add_space(10.0);
                        }

                        ui.add_space(4.0);

                        // Concise technical summary inside card_inner_w
                        zen_subtle_frame(self.theme_mode).show(ui, |ui| {
                            let inner_note_w = (card_inner_w - 32.0).max(200.0);
                            ui.set_width(inner_note_w);
                            ui.set_max_width(inner_note_w);
                            let note_text = if sim_plan.isolation_mode == CoreIsolationMode::StrictZeroOverlap {
                                format!(
                                    "Strict Core Isolation: Game cores [{}] and Client/Vanguard cores [{}] are physically separated with zero overlap. Primary render core: Core #{}.",
                                    sim_plan.game_cpus_lasso,
                                    sim_plan.vanguard_cpus_lasso,
                                    sim_plan.dedicated_lol_core_idx
                                )
                            } else {
                                format!(
                                    "Dedicated Primary Core Mode: Core #{} is reserved exclusively for the main render thread (0% Client/Vanguard). On secondary cores, physical threads execute game worker tasks while SMT threads isolate Client and Vanguard processes.",
                                    sim_plan.dedicated_lol_core_idx
                                )
                            };
                            ui.label(
                                RichText::new(note_text)
                                    .size(11.5)
                                    .color(p.text_secondary),
                            );
                        });
                    });
                },
            );

            // ================= RIGHT COLUMN: 3 EXECUTIVE STATUS CARDS =================
            ui.allocate_ui_with_layout(
                Vec2::new(right_col_w, 0.0),
                Layout::top_down(Align::Min),
                |right_ui| {
                    right_ui.set_width(right_col_w);
                    right_ui.set_max_width(right_col_w);
                    let right_inner_w = (right_col_w - 46.0).max(340.0);

                    // Card 1: Peripherals & Direct CPU SoC USB Bus
                    let has_soc_periph = self
                        .hw
                        .usb_controllers
                        .iter()
                        .any(|u| u.is_cpu_soc_direct && !u.connected_peripherals.is_empty());
                    zen_card_frame(self.theme_mode, false).show(right_ui, |ui| {
                        ui.set_width(right_inner_w);
                        ui.set_max_width(right_inner_w);

                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = 7.0;
                            draw_lucide_icon(ui, LucideIcon::Usb, 16.5, p.text_heading);
                            ui.label(
                                RichText::new("Input Peripherals & USB xHCI Topology")
                                    .size(15.0)
                                    .strong()
                                    .color(p.text_heading),
                            );
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                if has_soc_periph {
                                    zen_semantic_badge(
                                        ui,
                                        self.theme_mode,
                                        BadgeTone::Optimal,
                                        "STATUS: OPTIMAL",
                                    );
                                } else {
                                    zen_semantic_badge(
                                        ui,
                                        self.theme_mode,
                                        BadgeTone::Info,
                                        "CHIPSET BUS",
                                    );
                                }
                            });
                        });
                        ui.add_space(8.0);
                        for u in &self.hw.usb_controllers {
                            for periph in &u.connected_peripherals {
                                ui.horizontal(|ui| {
                                    let bus_txt = if u.is_cpu_soc_direct {
                                        "Direct CPU SoC Root Complex"
                                    } else {
                                        "Motherboard Chipset (PCH) Controller"
                                    };
                                    let dot_c = if u.is_cpu_soc_direct {
                                        p.emerald_optimal
                                    } else {
                                        p.cyan_client
                                    };
                                    zen_status_dot(ui, dot_c, 3.5);
                                    ui.add(
                                        Label::new(
                                            RichText::new(format!("{periph} — {bus_txt}"))
                                                .size(12.5)
                                                .color(p.text_body),
                                        )
                                        .wrap(),
                                    );
                                });
                                ui.add_space(2.0);
                            }
                        }
                        ui.add_space(4.0);
                        ui.label(
                            RichText::new(
                                "Direct CPU SoC xHCI routing minimizes high-polling-rate (1000-4000 Hz) input latency by bypassing the chipset bridge.",
                            )
                            .size(11.8)
                            .color(p.text_secondary),
                        );
                    });

                    right_ui.add_space(12.0);

                    // Card 2: Interrupt Isolation & MSI Mode (GPU, USB, NVMe & NIC)
                    let gpu_pinned = self
                        .irq_state
                        .gpu_irq
                        .as_ref()
                        .map(|g| g.is_pinned_to_recommended)
                        .unwrap_or(false);
                    let usb_pinned = self
                        .irq_state
                        .usb_irqs
                        .first()
                        .map(|u| u.is_pinned_to_recommended)
                        .unwrap_or(false);
                    let irq_all_ok = gpu_pinned && usb_pinned;

                    zen_card_frame(self.theme_mode, irq_all_ok).show(right_ui, |ui| {
                        ui.set_width(right_inner_w);
                        ui.set_max_width(right_inner_w);

                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = 7.0;
                            draw_lucide_icon(ui, LucideIcon::Zap, 16.5, p.text_heading);
                            ui.label(
                                RichText::new("Message Signaled Interrupts & Affinity")
                                    .size(15.0)
                                    .strong()
                                    .color(p.text_heading),
                            );
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                let mut toggle_val = irq_all_ok;
                                if zen_toggle_switch(ui, self.theme_mode, &mut toggle_val).changed()
                                {
                                    self.toggle_all_interrupts(toggle_val);
                                }
                                if irq_all_ok {
                                    zen_semantic_badge(
                                        ui,
                                        self.theme_mode,
                                        BadgeTone::Optimal,
                                        "STATUS: ACTIVE",
                                    );
                                } else {
                                    zen_semantic_badge(
                                        ui,
                                        self.theme_mode,
                                        BadgeTone::Neutral,
                                        "STATUS: DEFAULT",
                                    );
                                }
                            });
                        });
                        ui.add_space(6.0);
                        ui.label(
                            RichText::new(format!(
                                "GPU (Core #{}), USB xHCI (Core #{}) & NIC (Core #{}) offloaded from Core 0",
                                self.hw.cpu_plan.gpu_irq_core_idx,
                                self.hw.cpu_plan.usb_primary_irq_core_idx,
                                self.hw.cpu_plan.nic_irq_core_idx
                            ))
                            .size(12.8)
                            .color(p.text_body),
                        );
                        ui.add_space(3.0);
                        ui.label(
                            RichText::new(
                                "Configures Message Signaled Interrupts (MSISupported = 1) and isolates ISR/DPC execution across dedicated physical cores.",
                            )
                            .size(11.8)
                            .color(p.text_secondary),
                        );
                    });

                    right_ui.add_space(12.0);

                    // Card 3: Riot Vanguard & League Client Isolation (Process Lasso)
                    let vg_isolated = self.sched_state.process_lasso_rules_applied;
                    zen_card_frame(self.theme_mode, vg_isolated).show(right_ui, |ui| {
                        ui.set_width(right_inner_w);
                        ui.set_max_width(right_inner_w);

                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = 7.0;
                            draw_lucide_icon(ui, LucideIcon::Shield, 16.5, p.text_heading);
                            ui.label(
                                RichText::new("Process Lasso: Game, Client & Vanguard")
                                    .size(15.0)
                                    .strong()
                                    .color(p.text_heading),
                            );
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                let mut toggle_vg = vg_isolated;
                                if zen_toggle_switch(ui, self.theme_mode, &mut toggle_vg).changed()
                                {
                                    self.spawn_background_task(
                                        "Updating Process Lasso Profile",
                                        "Writing prolasso.ini & restarting ProcessGovernor...",
                                        move |_tx, hw| {
                                            let mut logs = Vec::new();
                                            if toggle_vg {
                                                match scheduler::apply_process_lasso_rules(&hw.cpu_plan) {
                                                    Ok(m) => logs.push(m),
                                                    Err(e) => logs.push(format!("[ERROR] {e}")),
                                                }
                                                let _ = scheduler::set_ifeo_priorities(true);
                                            } else {
                                                match scheduler::remove_process_lasso_rules() {
                                                    Ok(m) => logs.push(m),
                                                    Err(e) => logs.push(format!("[ERROR] {e}")),
                                                }
                                                let _ = scheduler::set_ifeo_priorities(false);
                                            }
                                            logs
                                        },
                                    );
                                }
                                if vg_isolated {
                                    zen_semantic_badge(
                                        ui,
                                        self.theme_mode,
                                        BadgeTone::Optimal,
                                        "STATUS: ISOLATED",
                                    );
                                } else {
                                    zen_semantic_badge(
                                        ui,
                                        self.theme_mode,
                                        BadgeTone::Neutral,
                                        "STATUS: DEFAULT",
                                    );
                                }
                            });
                        });
                        ui.add_space(6.0);
                        ui.label(
                            RichText::new(format!(
                                "Process Lasso (prolasso.ini): Game [{}], Client & Vanguard [{}] (Core #{} dedicated)",
                                self.hw.cpu_plan.game_cpus_lasso,
                                self.hw.cpu_plan.vanguard_cpus_lasso,
                                self.hw.cpu_plan.dedicated_lol_core_idx
                            ))
                            .size(12.8)
                            .color(p.text_body),
                        );
                        ui.add_space(3.0);
                        ui.label(
                            RichText::new(
                                "Binds all 12 RiotClient*, LeagueClient*, and Vanguard (vgc.exe/vgtray.exe) executables with ProBalance disabled (OocOn=false).",
                            )
                            .size(11.8)
                            .color(p.text_secondary),
                        );
                    });
                },
            );
        });

        if let Some(mode) = clicked_mode {
            self.set_core_isolation_mode(mode);
        }
        if let Some(new_core) = clicked_preferred_core {
            if self.sim_preset == SimCpuPreset::LiveHardware {
                self.set_preferred_lol_core(new_core);
            }
        }

        ui.add_space(16.0);

        // Collapsible Spoiler for Technical Diagnostics & Architecture Simulator
        self.render_technical_details_spoiler(ui);
    }

    /// Draws an interactive graphical Silicon Core Tile inside the CPU Infographic.
    /// Returns `true` if the user clicked on this core tile to make it the dedicated Primary Game Core.
    fn render_graphical_core_tile(
        &self,
        ui: &mut Ui,
        core: &CoreInfo,
        plan: &crate::hardware::CpuAffinityPlan,
        width: f32,
    ) -> bool {
        let p = self.theme_mode.palette();
        let phys_t = core.primary_thread();
        let is_game = (plan.game_mask & (1u64 << phys_t)) != 0;
        let is_client_vg = (plan.vanguard_mask & core.mask) != 0;
        let is_dedicated_lol_core = core.core_index == plan.dedicated_lol_core_idx;
        let is_gpu_irq = plan.gpu_irq_core_idx == core.core_index;
        let is_usb_irq = plan.usb_primary_irq_core_idx == core.core_index;
        let is_nic_irq = plan.nic_irq_core_idx == core.core_index;

        let (bg, border, title_col, badge_icon, badge_text, badge_col) = if is_dedicated_lol_core {
            (
                p.gold_bg,
                p.gold_priority,
                p.gold_priority,
                Some(LucideIcon::Crosshair),
                "EXCLUSIVE GAME",
                p.gold_priority,
            )
        } else if core.core_index == 0 && !is_game {
            (
                p.subtle_glass_panel,
                p.border_subtle,
                p.text_secondary,
                Some(LucideIcon::Shield),
                "SYSTEM / OS",
                p.text_muted,
            )
        } else if is_game && !is_client_vg {
            (
                p.subtle_glass_panel,
                p.emerald_optimal,
                p.text_heading,
                Some(LucideIcon::Zap),
                "GAME ONLY",
                p.emerald_optimal,
            )
        } else if is_game && is_client_vg {
            (
                p.subtle_glass_panel,
                p.border_selected,
                p.text_heading,
                Some(LucideIcon::Zap),
                "WORKERS + CLIENT",
                p.cyan_client,
            )
        } else if is_client_vg {
            (
                p.subtle_glass_panel,
                p.border_subtle,
                p.text_body,
                None,
                "CLIENT / VANGUARD",
                p.cyan_client,
            )
        } else {
            (
                p.subtle_glass_panel,
                p.border_subtle,
                p.text_muted,
                None,
                "STANDBY",
                p.text_muted,
            )
        };

        let tile_inner_w = (width - 26.0).max(60.0);
        let inner_resp = Frame::none()
            .fill(bg)
            .stroke(Stroke::new(
                if is_dedicated_lol_core {
                    1.5_f32
                } else {
                    1.0_f32
                },
                border,
            ))
            .rounding(Rounding::same(10.0))
            .inner_margin(Margin::symmetric(12.0, 10.0))
            .show(ui, |ui| {
                ui.set_width(tile_inner_w);
                ui.set_max_width(tile_inner_w);
                ui.vertical(|ui| {
                    let prefix = if plan.arch_class == CpuArchClass::IntelHybridPE {
                        if core.efficiency_class > 0 {
                            "P-Core"
                        } else {
                            "E-Core"
                        }
                    } else {
                        "Core"
                    };
                    ui.add(
                        Label::new(
                            RichText::new(format!("{prefix} {}", core.core_index))
                                .size(13.0)
                                .strong()
                                .color(title_col),
                        )
                        .truncate(),
                    );

                    ui.add_space(2.0);
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 4.0;
                        if let Some(ic) = badge_icon {
                            draw_lucide_icon(ui, ic, 11.0, badge_col);
                        }
                        ui.add(
                            Label::new(
                                RichText::new(badge_text)
                                    .size(9.8)
                                    .strong()
                                    .color(badge_col),
                            )
                            .truncate(),
                        );
                    });

                    ui.add_space(6.0);

                    // Graphical Thread Bars (Physical thread vs SMT logical thread)
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 4.0;
                        let n_threads = core.logical_threads.len().max(1);
                        let bar_w =
                            ((tile_inner_w - 4.0 * (n_threads as f32 - 1.0)) / n_threads as f32)
                                .clamp(18.0, 34.0);
                        for (t_idx, &_thread_num) in core.logical_threads.iter().enumerate() {
                            let is_phys_thread = t_idx == 0;
                            let bar_col = if is_dedicated_lol_core && is_phys_thread {
                                p.gold_priority
                            } else if is_game && is_phys_thread {
                                p.emerald_optimal
                            } else if is_client_vg {
                                p.cyan_client.gamma_multiply(0.65)
                            } else {
                                p.border_selected
                            };
                            let (rect, _) =
                                ui.allocate_exact_size(Vec2::new(bar_w, 5.0), Sense::hover());
                            ui.painter()
                                .rect_filled(rect, Rounding::same(2.0), bar_col);
                        }
                    });

                    ui.add_space(4.0);
                    let irq_note = if is_dedicated_lol_core {
                        "Primary Render (0% Client)"
                    } else if is_gpu_irq {
                        "IRQ: Display Adapter (GPU)"
                    } else if is_usb_irq {
                        "IRQ: USB xHCI (SoC)"
                    } else if is_nic_irq {
                        "IRQ: Network Controller"
                    } else if core.core_index == 0 {
                        "OS Kernel (DPC/ISR)"
                    } else if is_game && is_client_vg {
                        "Phys: Game | SMT: Client"
                    } else if is_game {
                        "Game Worker (No SMT)"
                    } else {
                        "Background Client Threads"
                    };
                    ui.add(
                        Label::new(RichText::new(irq_note).size(10.3).color(p.text_muted))
                            .truncate(),
                    );
                });
            });

        let click_resp = ui.interact(
            inner_resp.response.rect,
            ui.id().with(("core_tile_click", core.core_index)),
            Sense::click(),
        );
        if core.core_index > 0 {
            click_resp.clone().on_hover_text(format!(
                "Core #{} (Logical Threads {:?}):\n- Click to assign Core #{} as the dedicated primary core for League of Legends (0% Client/Vanguard).\n- First bar: physical thread T0 (no SMT); second bar: logical SMT thread T1.",
                core.core_index,
                core.logical_threads,
                core.core_index
            ));
        }
        click_resp.clicked() && core.core_index > 0
    }

    pub fn apply_interrupts_batch_sync(hw: &SystemHardwareReport, enable: bool) -> Vec<String> {
        let mut logs = Vec::new();
        if !hw.gpu.pnp_device_id.is_empty() {
            if enable {
                if let Ok(m) = interrupts::set_device_msi_mode(
                    MsiDeviceCategory::DiscreteGpu,
                    &hw.gpu.pnp_device_id,
                    &hw.gpu.name,
                    true,
                ) {
                    logs.push(m);
                }
            }
            match interrupts::set_device_interrupt_affinity(
                &hw.gpu.pnp_device_id,
                &hw.gpu.name,
                enable,
                hw.cpu_plan.gpu_irq_mask,
            ) {
                Ok(m) => logs.push(m),
                Err(e) => logs.push(format!("[ERROR] {e}")),
            }
        }
        let m1 = hw.cpu_plan.usb_primary_irq_mask;
        let m2 = hw.cpu_plan.usb_secondary_irq_mask;
        for (idx, u) in hw.usb_controllers.iter().enumerate() {
            if !u.pnp_device_id.is_empty() {
                if enable {
                    let _ = interrupts::set_device_msi_mode(
                        MsiDeviceCategory::UsbXhci,
                        &u.pnp_device_id,
                        &u.name,
                        true,
                    );
                }
                let mask = if idx == 0 { m1 } else { m2 };
                match interrupts::set_device_interrupt_affinity(
                    &u.pnp_device_id,
                    &u.name,
                    enable,
                    mask,
                ) {
                    Ok(m) => logs.push(m),
                    Err(e) => logs.push(format!("[ERROR] {e}")),
                }
            }
        }
        for nvme in &hw.nvme_controllers {
            if !nvme.pnp_device_id.is_empty() {
                if enable {
                    let _ = interrupts::set_device_msi_mode(
                        MsiDeviceCategory::NvmeStorage,
                        &nvme.pnp_device_id,
                        &nvme.name,
                        true,
                    );
                }
                if let Ok(m) = interrupts::set_device_interrupt_affinity(
                    &nvme.pnp_device_id,
                    &nvme.name,
                    enable,
                    hw.cpu_plan.usb_secondary_irq_mask,
                ) {
                    logs.push(m);
                }
            }
        }
        if !hw.nic.pnp_device_id.is_empty() {
            if enable {
                let _ = interrupts::set_device_msi_mode(
                    MsiDeviceCategory::NetworkAdapter,
                    &hw.nic.pnp_device_id,
                    &hw.nic.description,
                    true,
                );
            }
            match interrupts::set_device_interrupt_affinity(
                &hw.nic.pnp_device_id,
                &hw.nic.description,
                enable,
                hw.cpu_plan.nic_irq_mask,
            ) {
                Ok(m) => logs.push(m),
                Err(e) => logs.push(format!("[ERROR] {e}")),
            }
        }
        logs
    }

    fn toggle_all_interrupts(&mut self, enable: bool) {
        self.spawn_background_task(
            "Configuring Hardware Interrupts",
            "Updating MSI Mode & Interrupt Affinity policies...",
            move |_tx, hw| Self::apply_interrupts_batch_sync(hw, enable),
        );
    }

    /// Subtle collapsible accordion at the bottom of Overview for users who want raw PnP IDs or CPU topology simulation.
    fn render_technical_details_spoiler(&mut self, ui: &mut Ui) {
        let p = self.theme_mode.palette();
        ui.horizontal(|ui| {
            let (toggle_text, trail_icon) = if self.show_tech_details {
                (
                    "Hide Technical Diagnostics & Visual Topology Preview",
                    LucideIcon::ChevronUp,
                )
            } else {
                (
                    "Technical Diagnostics (Live Win32 Hardware Data, Bitmasks & Visual Layout Preview)",
                    LucideIcon::ChevronDown,
                )
            };
            if zen_pill_button_with_icons(
                ui,
                self.theme_mode,
                Some(LucideIcon::Sliders),
                toggle_text,
                Some(trail_icon),
                self.show_tech_details,
            )
            .clicked()
            {
                self.show_tech_details = !self.show_tech_details;
            }
        });

        if self.show_tech_details {
            ui.add_space(10.0);
            zen_card_frame(self.theme_mode, false).show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new("Visual Topology Layout Preview:")
                            .size(12.5)
                            .strong()
                            .color(p.text_body),
                    );
                    ComboBox::from_id_salt("cpu_sim_selector")
                        .width(420.0)
                        .selected_text(self.sim_preset.label())
                        .show_ui(ui, |ui| {
                            for preset in SimCpuPreset::all() {
                                ui.selectable_value(&mut self.sim_preset, preset, preset.label());
                            }
                        });
                });
                ui.add_space(4.0);
                ui.label(
                    RichText::new(
                        "Note: Real CPU affinity and interrupt rules are 100% dynamically auto-detected from your hardware via Win32 GetLogicalProcessorInformationEx (physical cores, SMT sibling masks, Intel P/E EfficiencyClass, and L3 cache domains). The selector above is strictly a visual preview for the diagram.",
                    )
                    .size(11.2)
                    .color(p.text_muted),
                );

                ui.add_space(8.0);
                ui.label(
                    RichText::new(format!(
                        "Live Detected CPU: {} ({} Physical Cores / {} Logical Threads | Profile: {})",
                        self.hw.cpu_name,
                        self.hw.physical_cores,
                        self.hw.logical_threads,
                        self.hw.cpu_plan.arch_class.display_name()
                    ))
                    .monospace()
                    .size(11.5)
                    .color(p.emerald_optimal),
                );
                ui.label(
                    RichText::new(format!(
                        "Affinity Bitmasks: League of Legends.exe = 0x{:04X} (CPU {}) | Client & Vanguard = 0x{:04X} (CPU {})",
                        self.hw.cpu_plan.game_mask,
                        self.hw.cpu_plan.game_cpus_lasso,
                        self.hw.cpu_plan.vanguard_mask,
                        self.hw.cpu_plan.vanguard_cpus_lasso
                    ))
                    .monospace()
                    .size(11.5)
                    .color(p.text_secondary),
                );
                ui.label(
                    RichText::new(format!(
                        "GPU PnP ID: {} (Driver Class Subkey: {})",
                        self.hw.gpu.pnp_device_id, self.hw.gpu.driver_class_subkey
                    ))
                    .monospace()
                    .size(11.0)
                    .color(p.text_muted),
                );
                for (i, u) in self.hw.usb_controllers.iter().enumerate() {
                    ui.label(
                        RichText::new(format!(
                            "USB xHCI #{}: {} -> {:?}",
                            i + 1,
                            u.pnp_device_id,
                            u.connected_peripherals
                        ))
                        .monospace()
                        .size(11.0)
                        .color(p.text_muted),
                    );
                }
                for (i, nvme) in self.hw.nvme_controllers.iter().enumerate() {
                    ui.label(
                        RichText::new(format!(
                            "NVMe #{}: {} ({})",
                            i + 1,
                            nvme.pnp_device_id,
                            nvme.name
                        ))
                        .monospace()
                        .size(11.0)
                        .color(p.text_muted),
                    );
                }
            });
        }
    }

    fn build_homogeneous_cores(num_cores: usize, smt: bool, dual_ccd: bool) -> Vec<CoreInfo> {
        let half = (num_cores / 2).max(1);
        (0..num_cores)
            .map(|i| {
                let (mask, logical_threads) = if smt {
                    (
                        (1u64 << (i * 2)) | (1u64 << (i * 2 + 1)),
                        vec![i * 2, i * 2 + 1],
                    )
                } else {
                    (1u64 << i, vec![i])
                };
                CoreInfo {
                    core_index: i,
                    mask,
                    logical_threads,
                    efficiency_class: 0,
                    l3_group: if dual_ccd && i >= half { 1 } else { 0 },
                }
            })
            .collect()
    }

    fn build_intel_hybrid_cores(p_count: usize, e_count: usize) -> Vec<CoreInfo> {
        let mut v = Vec::with_capacity(p_count + e_count);
        for i in 0..p_count {
            v.push(CoreInfo {
                core_index: i,
                mask: (1u64 << (i * 2)) | (1u64 << (i * 2 + 1)),
                logical_threads: vec![i * 2, i * 2 + 1],
                efficiency_class: 1,
                l3_group: 0,
            });
        }
        let base_t = p_count * 2;
        for e in 0..e_count {
            let t = base_t + e;
            v.push(CoreInfo {
                core_index: p_count + e,
                mask: 1u64 << t,
                logical_threads: vec![t],
                efficiency_class: 0,
                l3_group: 0,
            });
        }
        v
    }

    fn get_simulated_or_live_cores(&self) -> (Vec<CoreInfo>, String) {
        match self.sim_preset {
            SimCpuPreset::LiveHardware => (self.hw.cores.clone(), self.hw.cpu_name.clone()),
            SimCpuPreset::Intel12100F => (
                Self::build_homogeneous_cores(4, true, false),
                "Intel Core i3-12100F (4C/8T)".to_string(),
            ),
            SimCpuPreset::SixCoreNoSmt => (
                Self::build_homogeneous_cores(6, false, false),
                "Intel Core i5-9400F / 6C6T (No SMT)".to_string(),
            ),
            SimCpuPreset::Ryzen5600 => (
                Self::build_homogeneous_cores(6, true, false),
                "AMD Ryzen 5 5600 / 7600X (6C/12T)".to_string(),
            ),
            SimCpuPreset::Ryzen5800X3D => (
                Self::build_homogeneous_cores(8, true, false),
                "AMD Ryzen 7 7800X3D / 9800X3D (8C/16T)".to_string(),
            ),
            SimCpuPreset::Ryzen5900X => (
                Self::build_homogeneous_cores(12, true, true),
                "AMD Ryzen 9 5900X / 7900X3D (12C/24T Dual-CCD)".to_string(),
            ),
            SimCpuPreset::Ryzen7950X3D => (
                Self::build_homogeneous_cores(16, true, true),
                "AMD Ryzen 9 7950X3D / 9950X3D (16C/32T Dual-CCD)".to_string(),
            ),
            SimCpuPreset::Intel13400F => (
                Self::build_intel_hybrid_cores(6, 4),
                "Intel Core i5-13400F / 14400F (6P + 4E)".to_string(),
            ),
            SimCpuPreset::Intel13600K => (
                Self::build_intel_hybrid_cores(6, 8),
                "Intel Core i5-13600K / 14600K (6P + 8E)".to_string(),
            ),
            SimCpuPreset::Intel14700K => (
                Self::build_intel_hybrid_cores(8, 12),
                "Intel Core i7-14700K (8P + 12E)".to_string(),
            ),
            SimCpuPreset::Intel14900K => (
                Self::build_intel_hybrid_cores(8, 16),
                "Intel Core i9-13900K / 14900K (8P + 16E)".to_string(),
            ),
        }
    }

    fn render_windows_reg_tab(&mut self, ui: &mut Ui) {
        let paths = self.hw.league_exe_paths.clone();

        let mut gm = self.win_state.game_mode_on;
        if self.render_tweak_row(
            ui,
            "Windows Game Mode",
            "Prioritizes foreground game thread scheduling and suppresses background Windows Update tasks during active sessions.",
            "HKCU\\Software\\Microsoft\\GameBar (AllowAutoGameMode=1, AutoGameModeEnabled=1)",
            &mut gm,
        ) {
            match windows_reg::set_game_mode(gm) {
                Ok(m) => self.log(m),
                Err(e) => self.log(format!("[ERROR] {e}")),
            }
            self.refresh_all_states();
        }

        let mut wo = self.win_state.windowed_opt_on;
        if self.render_tweak_row(
            ui,
            "Optimizations for Windowed Games (DirectX Flip Model)",
            "Enables DirectX Independent Flip presentation in Borderless Windowed mode globally and for League of Legends.exe, bypassing DWM composition queuing.",
            "HKCU\\Software\\Microsoft\\DirectX\\UserGpuPreferences (SwapEffectUpgradeEnable=1)",
            &mut wo,
        ) {
            match windows_reg::set_windowed_optimizations(wo, &paths) {
                Ok(m) => self.log(m),
                Err(e) => self.log(format!("[ERROR] {e}")),
            }
            self.refresh_all_states();
        }

        let mut hags_off = self.win_state.hags_disabled;
        if self.render_tweak_row(
            ui,
            "Disable Hardware-Accelerated GPU Scheduling (HAGS: Disabled)",
            "Eliminates VRAM scheduler micro-stutters and frame-pacing variance in DX11 engines when background Chromium/CEF clients are active.",
            "HKLM\\SYSTEM\\CurrentControlSet\\Control\\GraphicsDrivers (HwSchMode=1)",
            &mut hags_off,
        ) {
            match windows_reg::set_hags_disabled(hags_off) {
                Ok(m) => self.log(m),
                Err(e) => self.log(format!("[ERROR] {e}")),
            }
            self.refresh_all_states();
        }

        let mut nlm = self.win_state.no_lazy_mode_on;
        if self.render_tweak_row(
            ui,
            "MMCSS Immediate Scheduling Mode (NoLazyMode)",
            "Disables lazy batching in the Windows Multimedia Class Scheduler Service (MMCSS) to reduce input-to-photon latency.",
            "HKLM\\SOFTWARE\\Microsoft\\Windows NT\\CurrentVersion\\Multimedia\\SystemProfile (NoLazyMode=1)",
            &mut nlm,
        ) {
            match windows_reg::set_no_lazy_mode(nlm) {
                Ok(m) => self.log(m),
                Err(e) => self.log(format!("[ERROR] {e}")),
            }
            self.refresh_all_states();
        }

        let mut sr10 = self.win_state.system_responsiveness_10;
        let cur_sr = self.win_state.current_system_responsiveness;
        if self.render_tweak_row(
            ui,
            &format!("MMCSS Real-Time CPU Reservation (SystemResponsiveness = 10, Current: {cur_sr})"),
            "Allocates up to 90% of CPU execution time to real-time multimedia and game threads instead of the default 80%.",
            "HKLM\\SOFTWARE\\Microsoft\\Windows NT\\CurrentVersion\\Multimedia\\SystemProfile (SystemResponsiveness)",
            &mut sr10,
        ) {
            let target = if sr10 { 10 } else { 20 };
            match windows_reg::set_system_responsiveness(target) {
                Ok(m) => self.log(m),
                Err(e) => self.log(format!("[ERROR] {e}")),
            }
            self.refresh_all_states();
        }

        let mut nti10 = self.win_state.network_throttling_10;
        let cur_nti = self.win_state.current_network_throttling;
        if self.render_tweak_row(
            ui,
            &format!("MMCSS Network Packet Throttling Balance (NetworkThrottlingIndex = 10, Current: {cur_nti})"),
            "Prevents NDIS packet bursts from starving the main render thread during high-activity teamfights.",
            "HKLM\\SOFTWARE\\Microsoft\\Windows NT\\CurrentVersion\\Multimedia\\SystemProfile (NetworkThrottlingIndex=10)",
            &mut nti10,
        ) {
            let target = if nti10 { 10 } else { 0xFFFFFFFF };
            match windows_reg::set_network_throttling_index(target) {
                Ok(m) => self.log(m),
                Err(e) => self.log(format!("[ERROR] {e}")),
            }
            self.refresh_all_states();
        }
    }

    fn render_scheduler_vanguard_tab(&mut self, ui: &mut Ui) {
        let p = self.theme_mode.palette();
        let plan = self.hw.cpu_plan.clone();

        let mut stp = self.sched_state.short_thread_policy_optimal;
        if self.render_tweak_row(
            ui,
            "Processor Short-Running Thread Scheduling Policy",
            "Unlocks the hidden Windows power scheme attribute and distributes short-lived worker threads across high-performance cores.",
            "powercfg SUB_PROCESSOR SHORTSCHEDPOLICY",
            &mut stp,
        ) {
            let target = if stp {
                plan.short_thread_policy_index
            } else {
                5
            };
            self.spawn_background_task(
                "Updating CPU Scheduler Policy",
                "Applying powercfg short-running thread policy...",
                move |_tx, _hw| match scheduler::set_short_thread_policy(target) {
                    Ok(m) => vec![m],
                    Err(e) => vec![format!("[ERROR] {e}")],
                },
            );
        }

        let mut psd = self.sched_state.power_saving_disabled;
        if self.render_tweak_row(
            ui,
            "Disable Bus Power Saving (USB Selective Suspend, PCIe ASPM, Core Parking)",
            "Prevents USB host controller state transitions, PCIe link power management delays, and OS core parking while preserving hardware boost clocks.",
            "powercfg USB / PCIe ASPM / CPMINCORES=100%",
            &mut psd,
        ) {
            self.spawn_background_task(
                "Updating Bus & Power Saving Rules",
                "Configuring USB Selective Suspend, PCIe ASPM & Core Parking...",
                move |_tx, _hw| match scheduler::set_power_saving_features_disabled(psd) {
                    Ok(m) => vec![m],
                    Err(e) => vec![format!("[ERROR] {e}")],
                },
            );
        }

        let mut ifeo = self.sched_state.ifeo_priorities_applied;
        if self.render_tweak_row(
            ui,
            "Kernel Process Priority Registration (IFEO PerfOptions)",
            "Assigns High CPU priority to League of Legends.exe and Below Normal priority to background Client and Vanguard processes.",
            "HKLM\\...\\Image File Execution Options\\*\\PerfOptions",
            &mut ifeo,
        ) {
            match scheduler::set_ifeo_priorities(ifeo) {
                Ok(m) => self.log(m),
                Err(e) => self.log(format!("[ERROR] {e}")),
            }
            self.refresh_all_states();
        }

        // Process Lasso Auto-Integration Card (Exclusively manages all CPU Affinity & ProBalance)
        zen_card_frame(self.theme_mode, self.sched_state.process_lasso_rules_applied).show(
            ui,
            |ui| {
                let row_w = ui.available_width();
                let left_w = (row_w - 420.0).max(300.0);
                ui.horizontal(|ui| {
                    ui.vertical(|ui| {
                        ui.set_width(left_w);
                        ui.set_max_width(left_w);
                        ui.horizontal(|ui| {
                            ui.label(
                                RichText::new("Process Lasso Profile (Game + LeagueClient + RiotClient + Vanguard)")
                                    .size(14.5)
                                    .strong()
                                    .color(p.text_heading),
                            );
                            zen_status_badge(
                                ui,
                                self.theme_mode,
                                self.sched_state.process_lasso_rules_applied,
                                "STATUS: ACTIVE",
                                "STATUS: INACTIVE",
                            );
                        });
                        ui.add_space(4.0);
                        ui.label(
                            RichText::new(format!(
                                "Writes UTF-16LE configuration to C:\\ProgramData\\ProcessLasso\\config\\prolasso.ini with ProBalance disabled (OocOn=false) and DefaultAffinitiesEx rules: League of Legends.exe -> [{}], LeagueClient*/RiotClient* -> [{}], vgc.exe/vgtray.exe -> [{}]. Automatically restarts ProcessGovernor.",
                                scheduler::to_prolasso_core_spec(&plan.game_cpus_lasso),
                                scheduler::to_prolasso_core_spec(&plan.client_cpus_lasso),
                                scheduler::to_prolasso_core_spec(&plan.vanguard_cpus_lasso),
                            ))
                            .size(12.0)
                            .color(p.text_secondary),
                        );
                    });

                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        let mut lasso_on = self.sched_state.process_lasso_rules_applied;
                        if zen_toggle_switch(ui, self.theme_mode, &mut lasso_on).changed() {
                            self.spawn_background_task(
                                "Updating Process Lasso Profile",
                                "Writing prolasso.ini & restarting ProcessGovernor...",
                                move |_tx, hw| {
                                    if lasso_on {
                                        match scheduler::apply_process_lasso_rules(&hw.cpu_plan) {
                                            Ok(m) => vec![m],
                                            Err(e) => vec![format!("[ERROR] {e}")],
                                        }
                                    } else {
                                        match scheduler::remove_process_lasso_rules() {
                                            Ok(m) => vec![m],
                                            Err(e) => vec![format!("[ERROR] {e}")],
                                        }
                                    }
                                },
                            );
                        }

                        if zen_pill_button(ui, self.theme_mode, "Reapply Profile", false)
                            .on_hover_text("Force-update C:\\ProgramData\\ProcessLasso\\config\\prolasso.ini and restart ProcessGovernor")
                            .clicked()
                        {
                            self.spawn_background_task(
                                "Reapplying Process Lasso Profile",
                                "Writing UTF-16LE prolasso.ini & restarting ProcessGovernor...",
                                move |_tx, hw| match scheduler::apply_process_lasso_rules(&hw.cpu_plan) {
                                    Ok(m) => vec![m],
                                    Err(e) => vec![format!("[ERROR] {e}")],
                                },
                            );
                        }

                        if !self.sched_state.process_lasso_installed {
                            if zen_pill_button(
                                ui,
                                self.theme_mode,
                                "Install Process Lasso",
                                false,
                            )
                            .clicked()
                            {
                                self.log("Installing Process Lasso via winget...");
                                self.spawn_background_task(
                                    "Installing Process Lasso",
                                    "Running winget install Bitsum.ProcessLasso...",
                                    move |_tx, _hw| match scheduler::install_process_lasso_via_winget() {
                                        Ok(m) => vec![m],
                                        Err(e) => vec![format!("[ERROR] {e}")],
                                    },
                                );
                            }
                        }
                    });
                });
            },
        );

        ui.add_space(10.0);

        // League of Legends & Riot Client Folder Paths Card
        zen_card_frame(self.theme_mode, false).show(ui, |ui| {
            ui.horizontal(|ui| {
                draw_lucide_icon(ui, LucideIcon::Sliders, 16.0, p.text_heading);
                ui.label(
                    RichText::new("League of Legends & Riot Client Installation Directories")
                        .size(14.5)
                        .strong()
                        .color(p.text_heading),
                );
            });
            ui.add_space(4.0);
            ui.label(
                RichText::new(
                    "Process Lasso affinity rules match executable names directly across all drives (league of legends.exe, leagueclient*.exe, riotclient*.exe, vgc.exe, vgtray.exe). The installation paths below are used to register DirectX Flip Model profiles (UserGpuPreferences).",
                )
                .size(11.8)
                .color(p.text_secondary),
            );
            ui.add_space(8.0);

            ui.horizontal(|ui| {
                ui.label(
                    RichText::new("Game Directory (League of Legends):")
                        .size(12.0)
                        .color(p.text_body),
                );
                let resp = ui.add(
                    egui::TextEdit::singleline(&mut self.league_folder_path).desired_width(360.0),
                );
                if resp.changed() {
                    self.sync_custom_folder_paths();
                }
                if zen_pill_button(ui, self.theme_mode, "Browse...", false).clicked() {
                    let current = self.league_folder_path.clone();
                    if let Some(picked) = Self::pick_folder_dialog(&current) {
                        self.league_folder_path = picked;
                        self.sync_custom_folder_paths();
                        let paths = self.hw.league_exe_paths.clone();
                        if self.win_state.windowed_opt_on {
                            let _ = windows_reg::set_windowed_optimizations(true, &paths);
                        }
                        self.log(format!(
                            "League of Legends directory set to: {}",
                            self.league_folder_path
                        ));
                        self.refresh_all_states();
                    }
                }
            });

            ui.add_space(6.0);

            ui.horizontal(|ui| {
                ui.label(
                    RichText::new("Client Directory (Riot Client):           ")
                        .size(12.0)
                        .color(p.text_body),
                );
                ui.add(
                    egui::TextEdit::singleline(&mut self.riot_client_folder_path)
                        .desired_width(360.0),
                );
                if zen_pill_button(ui, self.theme_mode, "Browse...", false).clicked() {
                    let current = self.riot_client_folder_path.clone();
                    if let Some(picked) = Self::pick_folder_dialog(&current) {
                        self.riot_client_folder_path = picked;
                        self.log(format!(
                            "Riot Client directory set to: {}",
                            self.riot_client_folder_path
                        ));
                    }
                }
            });
        });
    }

    fn render_interrupts_tab(&mut self, ui: &mut Ui) {
        let p = self.theme_mode.palette();

        // Section 1: Message Signaled Interrupts (MSI Mode)
        ui.horizontal(|ui| {
            draw_lucide_icon(ui, LucideIcon::Zap, 16.0, p.text_heading);
            ui.label(
                RichText::new("Message Signaled Interrupts (MSI Mode)")
                    .size(14.5)
                    .strong()
                    .color(p.text_heading),
            );
            ui.label(
                RichText::new("| Hardware-verified MSISupported = 1 & MessageNumberLimit (Legacy Audio/SATA/Timers excluded)")
                    .size(11.5)
                    .color(p.text_muted),
            );
        });
        ui.add_space(8.0);

        if let Some(gpu_irq) = self.irq_state.gpu_irq.clone() {
            if gpu_irq.hardware_msi_capable {
                let mut msi_on = gpu_irq.msi_supported;
                if self.render_tweak_row(
                    ui,
                    &format!("MSI Mode: Discrete Display Adapter ({})", self.hw.gpu.name),
                    "Switches GPU interrupt delivery from legacy line-based IRQ to low-latency Message Signaled Interrupts (MSISupported = 1).",
                    &format!("HKLM\\SYSTEM\\CurrentControlSet\\Enum\\{}\\Device Parameters\\Interrupt Management\\MessageSignaledInterruptProperties", gpu_irq.pnp_id),
                    &mut msi_on,
                ) {
                    match interrupts::set_device_msi_mode(
                        MsiDeviceCategory::DiscreteGpu,
                        &gpu_irq.pnp_id,
                        &self.hw.gpu.name,
                        msi_on,
                    ) {
                        Ok(m) => self.log(m),
                        Err(e) => self.log(format!("[ERROR] {e}")),
                    }
                    self.refresh_all_states();
                }
            }
        }

        let usb_msi_list = self.irq_state.usb_irqs.clone();
        for (idx, u_irq) in usb_msi_list.iter().enumerate() {
            if u_irq.hardware_msi_capable {
                let mut msi_on = u_irq.msi_supported;
                if self.render_tweak_row(
                    ui,
                    &format!("MSI Mode: USB 3.0 xHCI Host Controller #{} ({})", idx + 1, u_irq.label),
                    "Enables Message Signaled Interrupts (MSISupported = 1, MessageNumberLimit = 8) on the USB eXtensible Host Controller.",
                    &format!("HKLM\\SYSTEM\\CurrentControlSet\\Enum\\{}\\...\\MessageSignaledInterruptProperties", u_irq.pnp_id),
                    &mut msi_on,
                ) {
                    match interrupts::set_device_msi_mode(
                        MsiDeviceCategory::UsbXhci,
                        &u_irq.pnp_id,
                        &u_irq.label,
                        msi_on,
                    ) {
                        Ok(m) => self.log(m),
                        Err(e) => self.log(format!("[ERROR] {e}")),
                    }
                    self.refresh_all_states();
                }
            }
        }

        let nvme_msi_list = self.irq_state.nvme_irqs.clone();
        for (idx, nvme_irq) in nvme_msi_list.iter().enumerate() {
            if nvme_irq.hardware_msi_capable {
                let mut msi_on = nvme_irq.msi_supported;
                if self.render_tweak_row(
                    ui,
                    &format!("MSI Mode: NVMe Storage Controller #{} ({})", idx + 1, nvme_irq.label),
                    "Enables Message Signaled Interrupts (MSISupported = 1, MessageNumberLimit = 16) for non-volatile PCIe storage I/O.",
                    &format!("HKLM\\SYSTEM\\CurrentControlSet\\Enum\\{}\\...\\MessageSignaledInterruptProperties", nvme_irq.pnp_id),
                    &mut msi_on,
                ) {
                    match interrupts::set_device_msi_mode(
                        MsiDeviceCategory::NvmeStorage,
                        &nvme_irq.pnp_id,
                        &nvme_irq.label,
                        msi_on,
                    ) {
                        Ok(m) => self.log(m),
                        Err(e) => self.log(format!("[ERROR] {e}")),
                    }
                    self.refresh_all_states();
                }
            }
        }

        if let Some(nic_irq) = self.irq_state.nic_irq.clone() {
            if nic_irq.hardware_msi_capable {
                let mut msi_on = nic_irq.msi_supported;
                if self.render_tweak_row(
                    ui,
                    &format!("MSI Mode: Network Interface Controller ({})", self.hw.nic.description),
                    "Enables Message Signaled Interrupts (MSISupported = 1, MessageNumberLimit = 8) on the active network adapter.",
                    &format!("HKLM\\SYSTEM\\CurrentControlSet\\Enum\\{}\\...\\MessageSignaledInterruptProperties", nic_irq.pnp_id),
                    &mut msi_on,
                ) {
                    match interrupts::set_device_msi_mode(
                        MsiDeviceCategory::NetworkAdapter,
                        &nic_irq.pnp_id,
                        &self.hw.nic.description,
                        msi_on,
                    ) {
                        Ok(m) => self.log(m),
                        Err(e) => self.log(format!("[ERROR] {e}")),
                    }
                    self.refresh_all_states();
                }
            }
        }

        ui.add_space(8.0);
        ui.horizontal(|ui| {
            draw_lucide_icon(ui, LucideIcon::Cpu, 16.0, p.text_heading);
            ui.label(
                RichText::new("Interrupt Affinity Policy (Core Pinning)")
                    .size(14.5)
                    .strong()
                    .color(p.text_heading),
            );
        });
        ui.add_space(8.0);

        // Section 2: CPU Interrupt Affinity Policy
        if let Some(gpu_irq) = self.irq_state.gpu_irq.clone() {
            let mut pinned = gpu_irq.is_pinned_to_recommended;
            let rec_core = self.hw.cpu_plan.gpu_irq_core_idx;
            if self.render_tweak_row(
                ui,
                &format!(
                    "Interrupt Affinity: Discrete GPU ({}) -> Core #{rec_core}",
                    self.hw.gpu.name
                ),
                "Offloads display adapter ISR/DPC processing from Core 0 to a dedicated physical core.",
                &gpu_irq.pnp_id,
                &mut pinned,
            ) {
                match interrupts::set_device_interrupt_affinity(
                    &gpu_irq.pnp_id,
                    &self.hw.gpu.name,
                    pinned,
                    gpu_irq.recommended_mask,
                ) {
                    Ok(m) => self.log(m),
                    Err(e) => self.log(format!("[ERROR] {e}")),
                }
                self.refresh_all_states();
            }
        }

        let usb_list = self.irq_state.usb_irqs.clone();
        for (idx, u_irq) in usb_list.into_iter().enumerate() {
            let mut pinned = u_irq.is_pinned_to_recommended;
            let rec_core = if idx == 0 {
                self.hw.cpu_plan.usb_primary_irq_core_idx
            } else {
                self.hw.cpu_plan.usb_secondary_irq_core_idx
            };
            let is_soc = self
                .hw
                .usb_controllers
                .get(idx)
                .map(|u| u.is_cpu_soc_direct)
                .unwrap_or(false);
            let bus_desc = if is_soc {
                "Direct CPU SoC Root Complex | Connected input peripherals"
            } else {
                "Motherboard Chipset (PCH) Controller | Secondary peripherals"
            };

            if self.render_tweak_row(
                ui,
                &format!(
                    "Interrupt Affinity: USB xHCI Controller #{} -> Core #{rec_core}",
                    idx + 1
                ),
                &format!(
                    "{bus_desc}. Prevents DPC contention during high-polling-rate (1000-4000 Hz) mouse input."
                ),
                &u_irq.pnp_id,
                &mut pinned,
            ) {
                match interrupts::set_device_interrupt_affinity(
                    &u_irq.pnp_id,
                    &u_irq.label,
                    pinned,
                    u_irq.recommended_mask,
                ) {
                    Ok(m) => self.log(m),
                    Err(e) => self.log(format!("[ERROR] {e}")),
                }
                self.refresh_all_states();
            }
        }

        let nvme_list = self.irq_state.nvme_irqs.clone();
        for (idx, nvme_irq) in nvme_list.into_iter().enumerate() {
            let mut pinned = nvme_irq.is_pinned_to_recommended;
            let rec_core = self.hw.cpu_plan.usb_secondary_irq_core_idx;
            if self.render_tweak_row(
                ui,
                &format!(
                    "Interrupt Affinity: NVMe Storage Controller #{} ({}) -> Core #{rec_core}",
                    idx + 1,
                    nvme_irq.label
                ),
                "Routes NVMe storage controller interrupts away from Core 0 and the primary render core.",
                &nvme_irq.pnp_id,
                &mut pinned,
            ) {
                match interrupts::set_device_interrupt_affinity(
                    &nvme_irq.pnp_id,
                    &nvme_irq.label,
                    pinned,
                    nvme_irq.recommended_mask,
                ) {
                    Ok(m) => self.log(m),
                    Err(e) => self.log(format!("[ERROR] {e}")),
                }
                self.refresh_all_states();
            }
        }

        if let Some(nic_irq) = self.irq_state.nic_irq.clone() {
            let mut pinned = nic_irq.is_pinned_to_recommended;
            let rec_core = self.hw.cpu_plan.nic_irq_core_idx;
            if self.render_tweak_row(
                ui,
                &format!(
                    "Interrupt Affinity: Network Interface Controller ({}) -> Core #{rec_core}",
                    self.hw.nic.description
                ),
                "Pins NDIS/UDP network interface hardware interrupts to a secondary core isolated from the primary render thread.",
                &nic_irq.pnp_id,
                &mut pinned,
            ) {
                match interrupts::set_device_interrupt_affinity(
                    &nic_irq.pnp_id,
                    &self.hw.nic.description,
                    pinned,
                    nic_irq.recommended_mask,
                ) {
                    Ok(m) => self.log(m),
                    Err(e) => self.log(format!("[ERROR] {e}")),
                }
                self.refresh_all_states();
            }
        }
    }

    fn render_network_gpu_tab(&mut self, ui: &mut Ui) {
        let p = self.theme_mode.palette();
        let nic_name = self.hw.nic.name.clone();
        let gpu_subkey = self.hw.gpu.driver_class_subkey.clone();

        let mut uro_off = self.net_state.uro_disabled;
        if self.render_tweak_row(
            ui,
            "Disable UDP Receive Segment Coalescing Offload (URO: Disabled)",
            "Prevents the Windows TCP/IP stack from batching incoming UDP game packets, ensuring immediate per-packet delivery.",
            "netsh int udp set global uro=disabled",
            &mut uro_off,
        ) {
            self.spawn_background_task(
                "Configuring UDP Network Stack",
                "Updating netsh UDP Receive Segment Coalescing Offload (URO)...",
                move |_tx, _hw| match network::set_uro_disabled(uro_off) {
                    Ok(m) => vec![m],
                    Err(e) => vec![format!("[ERROR] {e}")],
                },
            );
        }

        let mut im_off = self.net_state.interrupt_moderation_disabled;
        if self.render_tweak_row(
            ui,
            &format!(
                "Disable NIC Interrupt Moderation ({})",
                self.hw.nic.description
            ),
            "Disables hardware interrupt coalescing on the network controller so incoming packets trigger immediate CPU processing.",
            &format!("Adapter '{}' -> *InterruptModeration = 0", nic_name),
            &mut im_off,
        ) {
            let nic_clone = nic_name.clone();
            self.spawn_background_task(
                "Configuring Network Adapter",
                format!("Updating Interrupt Moderation on '{nic_clone}'..."),
                move |_tx, _hw| match network::set_nic_interrupt_moderation_disabled(&nic_clone, im_off) {
                    Ok(m) => vec![m],
                    Err(e) => vec![format!("[ERROR] {e}")],
                },
            );
        }

        let mut eee_off = self.net_state.green_ethernet_disabled;
        if self.render_tweak_row(
            ui,
            "Disable Energy-Efficient Ethernet (EEE / Green Ethernet)",
            "Prevents low-power PHY link state transitions between network packets during active gameplay.",
            &format!("Adapter '{}' -> *EEE / GreenEthernet = 0", nic_name),
            &mut eee_off,
        ) {
            let nic_clone = nic_name.clone();
            self.spawn_background_task(
                "Configuring Network Adapter",
                format!("Updating Energy-Efficient Ethernet (EEE) on '{nic_clone}'..."),
                move |_tx, _hw| match network::set_nic_green_ethernet_disabled(&nic_clone, eee_off) {
                    Ok(m) => vec![m],
                    Err(e) => vec![format!("[ERROR] {e}")],
                },
            );
        }

        ui.add_space(10.0);
        ui.horizontal(|ui| {
            let subkeys_note = if !self.gpu_state.detected_amd_subkeys.is_empty() {
                format!(
                    " [Active Class Instances: {}]",
                    self.gpu_state.detected_amd_subkeys.join(", ")
                )
            } else {
                String::new()
            };
            ui.label(
                RichText::new(format!(
                    "Display Driver Configuration: {}{}",
                    self.hw.gpu.name, subkeys_note
                ))
                .size(14.0)
                .strong()
                .color(p.text_heading),
            );
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let lbl = if self.show_all_gpu_vendors {
                    "Hide Non-Detected GPU Vendors"
                } else {
                    "Show All GPU Vendors (AMD / NVIDIA)"
                };
                if zen_pill_button(ui, self.theme_mode, lbl, self.show_all_gpu_vendors).clicked() {
                    self.show_all_gpu_vendors = !self.show_all_gpu_vendors;
                }
                if self.hw.gpu.vendor == GpuVendor::Amd {
                    if zen_pill_button_with_icons(
                        ui,
                        self.theme_mode,
                        Some(LucideIcon::RotateCcw),
                        "Restore AMD Driver Defaults",
                        None,
                        false,
                    )
                    .on_hover_text("Roll back all AMD Radeon registry keys (Anti-Lag, FlipQueueSize, Main3D, ShaderCache, ULPS, DeepSleep) to factory defaults.")
                    .clicked()
                    {
                        match gpu::restore_amd_driver_defaults(&gpu_subkey) {
                            Ok(m) => self.log(m),
                            Err(e) => self.log(format!("[ERROR] {e}")),
                        }
                        self.refresh_all_states();
                    }
                }
            });
        });
        ui.add_space(8.0);

        if self.hw.gpu.vendor == GpuVendor::Amd || self.show_all_gpu_vendors {
            let mut al = self.gpu_state.amd_antilag_enabled;
            if self.render_tweak_row(
                ui,
                "AMD Radeon Anti-Lag (CPU/GPU Pre-Render Synchronization)",
                "Dynamically enables AMD Anti-Lag across all active display driver instances (KMD_DeLagEnabled, UMD\\AntiLag_DevMode, UMD\\DXC\\AllowDelag, and live ADL2 driver sync).",
                &format!("HKLM\\SYSTEM\\CurrentControlSet\\Control\\Class\\{{4d36e968-e325-11ce-bfc1-08002be10318}}\\{gpu_subkey} (KMD_DeLagEnabled=1, UMD\\AntiLag_DevMode=0x3100, UMD\\Dlg_PFEnable=0x3100)"),
                &mut al,
            ) {
                match gpu::set_amd_antilag(&gpu_subkey, al) {
                    Ok(m) => self.log(m),
                    Err(e) => self.log(format!("[ERROR] {e}")),
                }
                self.refresh_all_states();
            }

            let mut fq1 = self.gpu_state.amd_flip_queue_1;
            if self.render_tweak_row(
                ui,
                "AMD Flip Queue Size = 1 (Minimum Pre-Render Queue)",
                "Reduces the DirectX/OpenGL driver pre-render frame queue from 3 frames to 1 frame (FlipQueueSize = 1, Main3D = 1) to minimize input latency.",
                &format!("HKLM\\...\\{gpu_subkey}\\UMD (FlipQueueSize=0x3100, Main3D=0x3100, Main3D_DEF='1')"),
                &mut fq1,
            ) {
                match gpu::set_amd_flip_queue_size_1(&gpu_subkey, fq1) {
                    Ok(m) => self.log(m),
                    Err(e) => self.log(format!("[ERROR] {e}")),
                }
                self.refresh_all_states();
            }

            let mut ulps = self.gpu_state.amd_ulps_disabled;
            if self.render_tweak_row(
                ui,
                "AMD Radeon: Disable Ultra-Low Power State (ULPS)",
                "Prevents deep GPU power state transitions that cause frame-time spikes during sudden scene changes.",
                &format!("HKLM\\...\\{gpu_subkey}\\EnableUlps=0"),
                &mut ulps,
            ) {
                match gpu::set_amd_ulps_disabled(&gpu_subkey, ulps) {
                    Ok(m) => self.log(m),
                    Err(e) => self.log(format!("[ERROR] {e}")),
                }
                self.refresh_all_states();
            }

            let mut ds = self.gpu_state.amd_deep_sleep_disabled;
            if self.render_tweak_row(
                ui,
                "AMD Radeon: Disable Core Clock Deep Sleep (PP_SclkDeepSleepDisable)",
                "Prevents aggressive SCLK downclocking during low-load frames to maintain consistent frame pacing.",
                &format!("HKLM\\...\\{gpu_subkey}\\PP_SclkDeepSleepDisable=1"),
                &mut ds,
            ) {
                match gpu::set_amd_deep_sleep_disabled(&gpu_subkey, ds) {
                    Ok(m) => self.log(m),
                    Err(e) => self.log(format!("[ERROR] {e}")),
                }
                self.refresh_all_states();
            }

            let mut sc = self.gpu_state.amd_shader_cache_always_on;
            if self.render_tweak_row(
                ui,
                "AMD Radeon: Force Shader Cache Always On",
                "Forces persistent UMD shader caching (0x3200) to eliminate shader compilation stutters during ability animations.",
                &format!("HKLM\\...\\{gpu_subkey}\\UMD\\ShaderCache=0x3200"),
                &mut sc,
            ) {
                match gpu::set_amd_shader_cache_always_on(&gpu_subkey, sc) {
                    Ok(m) => self.log(m),
                    Err(e) => self.log(format!("[ERROR] {e}")),
                }
                self.refresh_all_states();
            }
        }

        if self.hw.gpu.vendor == GpuVendor::Nvidia || self.show_all_gpu_vendors {
            let mut nv = self.gpu_state.nvidia_profile_imported;
            if self.render_tweak_row(
                ui,
                "NVIDIA Driver Profile: Prefer Maximum Performance + Ultra Low Latency",
                "Applies hardware-validated NVIDIA driver parameters via NVIDIA Profile Inspector.",
                "nvidiaProfileInspector.exe -silentImport league_unlagged.nip",
                &mut nv,
            ) {
                match gpu::apply_nvidia_profile(nv, self.nvidia_ultra_mode) {
                    Ok(m) => self.log(m),
                    Err(e) => self.log(format!("[ERROR] {e}")),
                }
                self.refresh_all_states();
            }
        }
    }

    fn render_timer_display_backup_tab(&mut self, ui: &mut Ui) {
        let p = self.theme_mode.palette();

        let mut gtr = self.win_state.global_timer_requests_on;
        if self.render_tweak_row(
            ui,
            "Windows 11 Global Timer Resolution Requests",
            "Forces the Windows 11 kernel to honor high-resolution timer requests globally across all threads (opt-in).",
            "HKLM\\SYSTEM\\CurrentControlSet\\Control\\Session Manager\\kernel\\GlobalTimerResolutionRequests",
            &mut gtr,
        ) {
            match windows_reg::set_global_timer_resolution_requests(gtr) {
                Ok(m) => self.log(m),
                Err(e) => self.log(format!("[ERROR] {e}")),
            }
            self.refresh_all_states();
        }

        let cur_ms = self.sched_state.current_timer_resolution_100ns as f64 / 10_000.0;
        zen_card_frame(self.theme_mode, self.timer_505_enabled).show(ui, |ui| {
            let row_w = ui.available_width();
            let left_w = (row_w - 70.0).max(320.0);
            ui.horizontal(|ui| {
                ui.vertical(|ui| {
                    ui.set_width(left_w);
                    ui.set_max_width(left_w);
                    ui.horizontal(|ui| {
                        ui.label(
                            RichText::new(format!(
                                "Kernel Timer Resolution: 0.505 ms (Current Quantum: {cur_ms:.3} ms)"
                            ))
                            .size(14.5)
                            .strong()
                            .color(p.text_heading),
                        );
                        zen_status_badge(
                            ui,
                            self.theme_mode,
                            self.timer_505_enabled,
                            "STATUS: 0.505 MS ACTIVE",
                            "STATUS: OS DEFAULT",
                        );
                    });
                    ui.add_space(4.0);
                    ui.label(
                        RichText::new(
                            "Maintains a 0.505 ms system timer quantum via NtSetTimerResolution (manual opt-in control; excluded from default 1-Click profile).",
                        )
                        .size(12.0)
                        .color(p.text_secondary),
                    );
                });

                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if zen_toggle_switch(ui, self.theme_mode, &mut self.timer_505_enabled).changed()
                    {
                        match scheduler::set_kernel_timer_resolution(
                            self.timer_505_enabled,
                            self.timer_desired_100ns,
                        ) {
                            Ok(m) => self.log(m),
                            Err(e) => self.log(format!("[ERROR] {e}")),
                        }
                        self.refresh_all_states();
                    }
                });
            });
        });

        ui.add_space(10.0);

        let mut ddt = self.win_state.dynamic_tick_disabled;
        if self.render_tweak_row(
            ui,
            "Disable Kernel Dynamic Tick (disabledynamictick)",
            "Prevents the Windows kernel from suppressing timer interrupts during brief idle states.",
            "bcdedit /set disabledynamictick yes",
            &mut ddt,
        ) {
            match windows_reg::set_bcd_disable_dynamic_tick(ddt) {
                Ok(m) => self.log(m),
                Err(e) => self.log(format!("[ERROR] {e}")),
            }
            self.refresh_all_states();
        }

        // Display Topology (Direct 1-Click DisplayID 2.0 Tiled Topology + Optional CRU)
        zen_card_frame(self.theme_mode, self.cru_state.edid_override_present).show(ui, |ui| {
            let row_w = ui.available_width();
            let left_w = (row_w - 430.0).max(300.0);
            ui.horizontal(|ui| {
                ui.vertical(|ui| {
                    ui.set_width(left_w);
                    ui.set_max_width(left_w);
                    ui.horizontal(|ui| {
                        ui.label(
                            RichText::new("Monitor Topology Optimization (VESA DisplayID 2.0 Tiled Topology)")
                                .size(14.5)
                                .strong()
                                .color(p.text_heading),
                        );
                        zen_status_badge(
                            ui,
                            self.theme_mode,
                            self.cru_state.edid_override_present,
                            "DISPLAYID 2.0: ACTIVE",
                            "STANDARD EDID",
                        );
                    });
                    ui.add_space(4.0);
                    let mon_info = if self.cru_state.active_displays.is_empty() {
                        "Active Display: Standard EDID".to_string()
                    } else {
                        format!("Active Display: {}", self.cru_state.active_displays.join(", "))
                    };
                    ui.label(
                        RichText::new(format!(
                            "Writes a VESA DisplayID 2.0 (Tiled Display Topology 1x1) extension block directly to monitor EDID_OVERRIDE while preserving existing CEA-861 high-refresh-rate blocks ({mon_info})."
                        ))
                        .size(12.0)
                        .color(p.text_secondary),
                    );
                });

                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    let mut edid_on = self.cru_state.edid_override_present;
                    if zen_toggle_switch(ui, self.theme_mode, &mut edid_on)
                        .on_hover_text("Apply or remove the DisplayID 2.0 (EDID_OVERRIDE) extension block in the Windows Registry")
                        .changed()
                    {
                        match display_cru::set_displayid2_tiled_topology_override(edid_on) {
                            Ok(m) => self.log(m),
                            Err(e) => self.log(format!("[ERROR] {e}")),
                        }
                        self.refresh_all_states();
                    }

                    let gpu_pnp = self.hw.gpu.pnp_device_id.clone();
                    if zen_pill_button(ui, self.theme_mode, "Restart Display Driver", false)
                        .on_hover_text("Restarts the active display driver via pnputil or restart64 to apply EDID changes immediately")
                        .clicked()
                    {
                        match display_cru::run_cru_restart64(&gpu_pnp) {
                            Ok(m) => self.log(m),
                            Err(e) => self.log(format!("[ERROR] {e}")),
                        }
                    }

                    if zen_pill_button(ui, self.theme_mode, "Launch CRU (Optional)", false)
                        .on_hover_text("Download and launch ToastyX Custom Resolution Utility (CRU 1.5.3)")
                        .clicked()
                    {
                        match display_cru::download_and_launch_cru() {
                            Ok(m) => self.log(m),
                            Err(e) => self.log(format!("[ERROR] {e}")),
                        }
                        self.refresh_all_states();
                    }
                });
            });
        });

        ui.add_space(12.0);

        // Backup & Rollback Card
        zen_card_frame(self.theme_mode, false).show(ui, |ui| {
            let row_w = ui.available_width();
            let left_w = (row_w - 490.0).max(300.0);
            ui.horizontal(|ui| {
                ui.vertical(|ui| {
                    ui.set_width(left_w);
                    ui.set_max_width(left_w);
                    ui.label(
                        RichText::new("System Baseline Snapshot & Complete Rollback")
                            .size(14.5)
                            .strong()
                            .color(p.text_heading),
                    );
                    ui.add_space(4.0);
                    ui.label(
                        RichText::new(
                            "Your baseline configuration is automatically saved in backup_original_state.json. You can revert all registry, driver, and scheduler modifications in one click.",
                        )
                        .size(12.0)
                        .color(p.text_secondary),
                    );
                });

                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if zen_pill_button_with_icons(
                        ui,
                        self.theme_mode,
                        Some(LucideIcon::RotateCcw),
                        "Restore Baseline Snapshot",
                        None,
                        true,
                    )
                    .clicked()
                    {
                        self.disable_all_tweaks();
                    }

                    if zen_pill_button(ui, self.theme_mode, "Create OS Restore Point", false)
                        .clicked()
                    {
                        self.spawn_background_task(
                            "Creating Windows Restore Point",
                            "Executing Checkpoint-Computer via PowerShell...",
                            move |_tx, _hw| match create_windows_restore_point() {
                                Ok(m) => vec![m],
                                Err(e) => vec![format!("[INFO] {e}")],
                            },
                        );
                    }
                });
            });
        });
    }

    /// Clean, clutter-free tweak card: hides raw registry paths inside hover tooltip (or when `show_tech_details` is enabled).
    fn render_tweak_row(
        &self,
        ui: &mut Ui,
        title: &str,
        description: &str,
        tech_tooltip: &str,
        value: &mut bool,
    ) -> bool {
        let p = self.theme_mode.palette();
        let mut changed = false;

        zen_card_frame(self.theme_mode, *value).show(ui, |ui| {
            let row_w = ui.available_width();
            let left_w = (row_w - 68.0).max(320.0);
            ui.horizontal(|ui| {
                ui.vertical(|ui| {
                    ui.set_width(left_w);
                    ui.set_max_width(left_w);
                    ui.horizontal(|ui| {
                        let resp = ui.label(
                            RichText::new(title)
                                .size(14.5)
                                .strong()
                                .color(p.text_heading),
                        );
                        resp.on_hover_text(format!("Registry / System Target: {tech_tooltip}"));

                        zen_status_badge(
                            ui,
                            self.theme_mode,
                            *value,
                            "STATUS: ACTIVE",
                            "STATUS: DEFAULT",
                        );
                    });
                    ui.add_space(4.0);
                    ui.label(RichText::new(description).size(12.2).color(p.text_secondary));
                    if self.show_tech_details {
                        ui.add_space(2.0);
                        ui.label(
                            RichText::new(tech_tooltip)
                                .monospace()
                                .size(10.5)
                                .color(p.text_muted),
                        );
                    }
                });

                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if zen_toggle_switch(ui, self.theme_mode, value).changed() {
                        changed = true;
                    }
                });
            });
        });
        ui.add_space(10.0);
        changed
    }

    /// Single-line bottom Status Bar + on-demand Sliding Log Drawer.
    fn render_bottom_status_bar(&mut self, ctx: &Context) {
        let p = self.theme_mode.palette();
        let (active_cnt, total_cnt) = self.count_active_optimizations();

        // Optional sliding drawer when "System Log" is toggled open
        if self.show_log_drawer {
            egui::TopBottomPanel::bottom("zen_sliding_log_drawer")
                .resizable(true)
                .default_height(150.0)
                .frame(
                    Frame::none()
                        .fill(p.elevated_modal)
                        .stroke(Stroke::new(1.0_f32, p.border_subtle))
                        .inner_margin(Margin::symmetric(20.0, 12.0)),
                )
                .show(ctx, |ui| {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 6.0;
                        draw_lucide_icon(ui, LucideIcon::Terminal, 15.0, p.text_heading);
                        ui.label(
                            RichText::new("System Execution Log")
                                .size(12.0)
                                .strong()
                                .color(p.text_heading),
                        );
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            if zen_pill_button_with_icons(
                                ui,
                                self.theme_mode,
                                None,
                                "Close",
                                Some(LucideIcon::ChevronDown),
                                false,
                            )
                            .clicked()
                            {
                                self.show_log_drawer = false;
                            }
                            if zen_pill_button(ui, self.theme_mode, "Clear", false).clicked() {
                                self.logs.clear();
                            }
                        });
                    });
                    ui.add_space(6.0);
                    ScrollArea::vertical()
                        .stick_to_bottom(true)
                        .show(ui, |ui| {
                            for line in &self.logs {
                                ui.label(
                                    RichText::new(line)
                                        .monospace()
                                        .size(11.0)
                                        .color(p.text_secondary),
                                );
                            }
                        });
                });
        }

        // 1-row Footer Status Bar (40px height, zero vertical clipping, exact vertical centerline cy)
        egui::TopBottomPanel::bottom("zen_luxury_footer_bar")
            .exact_height(40.0)
            .frame(
                Frame::none()
                    .fill(p.main_glass_panel)
                    .stroke(Stroke::new(1.0_f32, p.border_subtle))
                    .inner_margin(Margin::symmetric(20.0, 0.0)),
            )
            .show(ctx, |ui| {
                let full_rect = ui.max_rect();
                let cy = full_rect.center().y;

                // Paint left status dot + status text at exact vertical center cy
                {
                    let painter = ui.painter();
                    let dot_color = if active_cnt >= 6 {
                        p.emerald_optimal
                    } else {
                        p.gold_priority
                    };
                    let dot_center = pos2(full_rect.left() + 6.0, cy);
                    painter.circle_filled(dot_center, 6.8, dot_color.gamma_multiply(0.18));
                    painter.circle_filled(dot_center, 4.0, dot_color);

                    let short_cpu = Self::compact_cpu_name(&self.hw.cpu_name);
                    let status_state = if active_cnt >= 6 { "Active" } else { "Ready" };
                    let status_msg = format!(
                        "Status: {status_state}  |  Hardware Profile: {short_cpu} ({}C/{}T)  |  Active Modules: {active_cnt}/{total_cnt}",
                        self.hw.physical_cores,
                        self.hw.logical_threads
                    );
                    let galley = painter.layout_no_wrap(
                        status_msg,
                        egui::FontId::new(12.0, egui::FontFamily::Proportional),
                        p.text_secondary,
                    );
                    painter.galley(
                        pos2(full_rect.left() + 18.0, cy - galley.size().y * 0.5),
                        galley,
                        p.text_secondary,
                    );
                }

                // Right-aligned System Log button centered vertically inside the 40px footer bar
                ui.allocate_new_ui(egui::UiBuilder::new().max_rect(full_rect), |ui| {
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        let log_btn_text = format!("System Log ({})", self.logs.len());
                        let trail_ic = if self.show_log_drawer {
                            LucideIcon::ChevronDown
                        } else {
                            LucideIcon::ChevronUp
                        };
                        if zen_pill_button_with_icons(
                            ui,
                            self.theme_mode,
                            Some(LucideIcon::Terminal),
                            &log_btn_text,
                            Some(trail_ic),
                            self.show_log_drawer,
                        )
                        .clicked()
                        {
                            self.show_log_drawer = !self.show_log_drawer;
                        }
                    });
                });
            });
    }

    /// Custom Frameless Window Titlebar with horizontally & vertically centered title block, native drag, double-click maximize, and vector window controls.
    fn render_custom_titlebar(&mut self, ctx: &Context) {
        let p = self.theme_mode.palette();
        let is_maximized = ctx.input(|i| i.viewport().maximized.unwrap_or(false));

        egui::TopBottomPanel::top("zen_custom_titlebar")
            .exact_height(36.0)
            .frame(
                Frame::none()
                    .fill(p.main_glass_panel)
                    .stroke(Stroke::new(1.0_f32, p.border_subtle))
                    .inner_margin(Margin::symmetric(16.0, 0.0)),
            )
            .show(ctx, |ui| {
                let full_rect = ui.max_rect();
                let cy = full_rect.center().y;

                // Reserve the right 126px for the 3 window control buttons so drag never steals their clicks
                let drag_rect = egui::Rect::from_min_max(
                    full_rect.min,
                    pos2((full_rect.right() - 126.0).max(full_rect.left()), full_rect.bottom()),
                );
                let drag_resp = ui.interact(
                    drag_rect,
                    ui.id().with("zen_titlebar_drag_zone"),
                    Sense::click_and_drag(),
                );
                if drag_resp.drag_started_by(egui::PointerButton::Primary) {
                    ctx.send_viewport_cmd(egui::ViewportCommand::StartDrag);
                }
                if drag_resp.double_clicked() {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(!is_maximized));
                }

                // Paint the centered title group: [Logo] LEAGUE UNLAGGED | System Latency & Hardware Topology Utility [ADMIN]
                {
                    let painter = ui.painter();
                    let title_galley = painter.layout_no_wrap(
                        "LEAGUE UNLAGGED".to_string(),
                        egui::FontId::new(11.5, egui::FontFamily::Proportional),
                        p.text_heading,
                    );
                    let sub_galley = painter.layout_no_wrap(
                        "|  System Latency & Hardware Topology Utility".to_string(),
                        egui::FontId::new(11.0, egui::FontFamily::Proportional),
                        p.text_muted,
                    );
                    let badge_label = if self.is_admin { "ADMIN" } else { "USER MODE" };
                    let (badge_fg, badge_bg) = if self.is_admin {
                        (p.emerald_optimal, p.emerald_bg)
                    } else {
                        (p.text_muted, p.subtle_glass_panel)
                    };
                    let badge_galley = painter.layout_no_wrap(
                        badge_label.to_string(),
                        egui::FontId::new(10.0, egui::FontFamily::Proportional),
                        badge_fg,
                    );

                    let logo_w = if self.brand_logo.is_some() { 18.0 } else { 0.0 };
                    let gap_logo = if self.brand_logo.is_some() { 8.0 } else { 0.0 };
                    let gap_sub = 8.0;
                    let gap_badge = 10.0;
                    let badge_w = badge_galley.size().x + 14.0;
                    let badge_h = 19.0;

                    let total_group_w = logo_w
                        + gap_logo
                        + title_galley.size().x
                        + gap_sub
                        + sub_galley.size().x
                        + gap_badge
                        + badge_w;

                    let mut cursor_x = (full_rect.center().x - total_group_w * 0.5)
                        .max(full_rect.left() + 8.0);

                    if let Some(tex) = &self.brand_logo {
                        let logo_rect = egui::Rect::from_center_size(
                            pos2(cursor_x + logo_w * 0.5, cy),
                            Vec2::splat(18.0),
                        );
                        painter.image(
                            tex.id(),
                            logo_rect,
                            egui::Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
                            egui::Color32::WHITE,
                        );
                        cursor_x += logo_w + gap_logo;
                    }

                    painter.galley(
                        pos2(cursor_x, cy - title_galley.size().y * 0.5),
                        title_galley.clone(),
                        p.text_heading,
                    );
                    cursor_x += title_galley.size().x + gap_sub;

                    painter.galley(
                        pos2(cursor_x, cy - sub_galley.size().y * 0.5),
                        sub_galley.clone(),
                        p.text_muted,
                    );
                    cursor_x += sub_galley.size().x + gap_badge;

                    let badge_rect = egui::Rect::from_center_size(
                        pos2(cursor_x + badge_w * 0.5, cy),
                        Vec2::new(badge_w, badge_h),
                    );
                    painter.rect(
                        badge_rect,
                        Rounding::same(5.0),
                        badge_bg,
                        Stroke::new(1.0_f32, badge_fg.gamma_multiply(0.38)),
                    );
                    painter.galley(
                        pos2(
                            badge_rect.center().x - badge_galley.size().x * 0.5,
                            cy - badge_galley.size().y * 0.5,
                        ),
                        badge_galley,
                        badge_fg,
                    );
                }

                // Right-aligned window control buttons, vertically centered in the 36px bar
                ui.allocate_new_ui(egui::UiBuilder::new().max_rect(full_rect), |ui| {
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.spacing_mut().item_spacing.x = 4.0;

                        if zen_window_control_button(
                            ui,
                            self.theme_mode,
                            LucideIcon::Close,
                            true,
                        )
                        .on_hover_text("Close")
                        .clicked()
                        {
                            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                        }

                        let max_icon = if is_maximized {
                            LucideIcon::Restore
                        } else {
                            LucideIcon::Maximize
                        };
                        let max_tip = if is_maximized { "Restore" } else { "Maximize" };
                        if zen_window_control_button(ui, self.theme_mode, max_icon, false)
                            .on_hover_text(max_tip)
                            .clicked()
                        {
                            ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(!is_maximized));
                        }

                        if zen_window_control_button(
                            ui,
                            self.theme_mode,
                            LucideIcon::Minus,
                            false,
                        )
                        .on_hover_text("Minimize")
                        .clicked()
                        {
                            ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(true));
                        }
                    });
                });
            });
    }

    fn execute_modal_display_driver_restart(&mut self) -> String {
        if !self.cru_state.edid_override_present {
            if let Ok(m) = display_cru::set_displayid2_tiled_topology_override(true) {
                self.log(m);
            }
        }
        let gpu_pnp = self.hw.gpu.pnp_device_id.clone();
        let status = match display_cru::run_cru_restart64(&gpu_pnp) {
            Ok(m) => {
                self.log(m);
                "Display Driver Restarted (DisplayID 2.0 Active)".to_string()
            }
            Err(e) => {
                self.log(format!("[ERROR] {e}"));
                "DisplayID 2.0 Saved (Reboot to Apply)".to_string()
            }
        };
        self.cru_state = display_cru::DisplayCruState::refresh();
        status
    }

    /// Polls any active background worker thread and advances progress animation smoothly at 60 FPS.
    fn poll_background_job(&mut self, ctx: &Context) {
        let mut finished_payload: Option<(String, Vec<String>, Box<RefreshedStates>)> = None;

        if let Some(job) = &mut self.active_job {
            ctx.request_repaint();

            while let Ok(ev) = job.rx.try_recv() {
                match ev {
                    WorkerEvent::Step {
                        step_label,
                        progress,
                        log_line,
                    } => {
                        job.step_label = step_label;
                        job.target_progress = progress.clamp(0.05, 0.96);
                        if let Some(l) = log_line {
                            self.logs.push(l);
                        }
                    }
                    WorkerEvent::Finished { logs, states } => {
                        finished_payload = Some((job.title.clone(), logs, states));
                    }
                }
            }

            if !job.is_finished {
                // Smooth exponential interpolation + gentle time-based creep so progress never looks stalled
                let elapsed_s = job.started_at.elapsed().as_secs_f32();
                let auto_target = (job.target_progress + (elapsed_s * 0.18).min(0.22)).min(0.94);
                let diff = auto_target - job.visual_progress;
                job.visual_progress += diff * 0.16;
            } else {
                job.visual_progress = 1.0;
            }
        }

        if let Some((title, logs, states)) = finished_payload {
            self.win_state = states.win_state;
            self.sched_state = states.sched_state;
            self.irq_state = states.irq_state;
            self.net_state = states.net_state;
            self.gpu_state = states.gpu_state;
            self.cru_state = states.cru_state;
            for line in logs {
                self.log(line);
            }

            let keep_modal_for_driver = self
                .active_job
                .as_ref()
                .map(|j| j.show_driver_restart_action)
                .unwrap_or(false);

            if keep_modal_for_driver {
                let queued = self
                    .active_job
                    .as_ref()
                    .map(|j| j.driver_restart_queued)
                    .unwrap_or(false);
                let auto_status = if queued {
                    Some(self.execute_modal_display_driver_restart())
                } else {
                    None
                };
                if let Some(job) = &mut self.active_job {
                    job.is_finished = true;
                    job.target_progress = 1.0;
                    job.visual_progress = 1.0;
                    job.step_label =
                        "All optimizations applied. Restart display driver to activate CRU DisplayID 2.0 & GPU changes."
                            .to_string();
                    if let Some(st) = auto_status {
                        job.driver_restarted_status = Some(st);
                    }
                }
            } else {
                self.active_job = None;
                self.completion_toast = Some((Instant::now(), format!("{title} — Applied")));
            }
            ctx.request_repaint();
        }
    }

    /// Renders a 60 FPS animated vector progress modal while a background tweak task is running,
    /// including an inline CRU / DisplayID 2.0 Display Driver Restart button during 1-Click Optimization.
    fn render_active_job_overlay(&mut self, ctx: &Context) {
        let p = self.theme_mode.palette();
        let screen_rect = ctx.screen_rect();

        let mut click_restart_driver = false;
        let mut click_close_modal = false;

        if let Some(job) = &self.active_job {
            ctx.request_repaint();
            let time = ctx.input(|i| i.time) as f32;

            let overlay_layer = egui::LayerId::new(
                egui::Order::Middle,
                egui::Id::new("zen_active_job_backdrop"),
            );
            let backdrop_col = match self.theme_mode {
                ZenThemeMode::Light => egui::Color32::from_rgba_unmultiplied(15, 23, 42, 72),
                ZenThemeMode::Dark => egui::Color32::from_rgba_unmultiplied(4, 6, 12, 148),
            };
            let painter = ctx.layer_painter(overlay_layer);
            painter.rect_filled(screen_rect, Rounding::ZERO, backdrop_col);

            let has_driver_bar = job.show_driver_restart_action;
            let card_w = if has_driver_bar { 520.0_f32 } else { 450.0_f32 }
                .min(screen_rect.width() - 40.0);
            let card_h = if has_driver_bar { 188.0_f32 } else { 132.0_f32 };
            let card_rect =
                egui::Rect::from_center_size(screen_rect.center(), Vec2::new(card_w, card_h));

            // Soft drop shadow + elevated card surface
            painter.rect_filled(
                card_rect.translate(Vec2::new(0.0, 6.0)).expand(2.0),
                Rounding::same(14.0),
                egui::Color32::from_black_alpha(45),
            );
            painter.rect(
                card_rect,
                Rounding::same(14.0),
                p.elevated_modal,
                Stroke::new(
                    1.5_f32,
                    if job.is_finished {
                        p.emerald_optimal
                    } else {
                        p.border_selected
                    },
                ),
            );

            // Animated dual-arc vector spinner (or Emerald Checkmark when finished at 100%)
            let spinner_center = pos2(card_rect.left() + 42.0, card_rect.top() + 46.0);
            let spinner_radius = 15.5_f32;
            if job.is_finished {
                painter.circle_filled(
                    spinner_center,
                    spinner_radius,
                    p.emerald_optimal.gamma_multiply(0.16),
                );
                painter.circle_stroke(
                    spinner_center,
                    spinner_radius,
                    Stroke::new(2.0_f32, p.emerald_optimal),
                );
                let ck = Stroke::new(2.4_f32, p.emerald_optimal);
                painter.line_segment(
                    [
                        pos2(spinner_center.x - 5.0, spinner_center.y + 0.5),
                        pos2(spinner_center.x - 1.2, spinner_center.y + 4.2),
                    ],
                    ck,
                );
                painter.line_segment(
                    [
                        pos2(spinner_center.x - 1.2, spinner_center.y + 4.2),
                        pos2(spinner_center.x + 5.8, spinner_center.y - 3.6),
                    ],
                    ck,
                );
            } else {
                painter.circle_stroke(
                    spinner_center,
                    spinner_radius,
                    Stroke::new(2.2_f32, p.border_subtle),
                );

                let segs = 20usize;
                let start_angle = time * 4.4;
                let sweep = std::f32::consts::TAU * 0.62;
                let pts: Vec<egui::Pos2> = (0..=segs)
                    .map(|i| {
                        let a = start_angle + sweep * (i as f32 / segs as f32);
                        pos2(
                            spinner_center.x + spinner_radius * a.cos(),
                            spinner_center.y + spinner_radius * a.sin(),
                        )
                    })
                    .collect();
                for pair in pts.windows(2) {
                    painter.line_segment([pair[0], pair[1]], Stroke::new(2.8_f32, p.gold_priority));
                }
                let inner_angle = -time * 3.2;
                let dot_pos = pos2(
                    spinner_center.x + (spinner_radius * 0.52) * inner_angle.cos(),
                    spinner_center.y + (spinner_radius * 0.52) * inner_angle.sin(),
                );
                painter.circle_filled(dot_pos, 3.0, p.emerald_optimal);
            }

            // Title & live percentage
            let text_left = card_rect.left() + 74.0;
            let display_title = if job.is_finished {
                "1-Click Optimization Complete".to_string()
            } else {
                job.title.clone()
            };
            let title_galley = painter.layout_no_wrap(
                display_title,
                egui::FontId::new(14.5, egui::FontFamily::Proportional),
                p.text_heading,
            );
            painter.galley(
                pos2(text_left, card_rect.top() + 24.0),
                title_galley,
                p.text_heading,
            );

            let pct_val = if job.is_finished {
                100.0
            } else {
                (job.visual_progress * 100.0).clamp(6.0, 99.0)
            };
            let pct_col = if job.is_finished {
                p.emerald_optimal
            } else {
                p.gold_priority
            };
            let pct_str = format!("{pct_val:.0}%");
            let pct_galley = painter.layout_no_wrap(
                pct_str,
                egui::FontId::new(12.5, egui::FontFamily::Monospace),
                pct_col,
            );
            painter.galley(
                pos2(
                    card_rect.right() - 24.0 - pct_galley.size().x,
                    card_rect.top() + 26.0,
                ),
                pct_galley,
                pct_col,
            );

            // Step description
            let step_galley = painter.layout(
                job.step_label.clone(),
                egui::FontId::new(11.8, egui::FontFamily::Proportional),
                p.text_secondary,
                card_w - 98.0,
            );
            painter.galley(
                pos2(text_left, card_rect.top() + 48.0),
                step_galley,
                p.text_secondary,
            );

            // Animated progress bar track & shimmer fill
            let bar_top = card_rect.top() + 92.0;
            let bar_rect = egui::Rect::from_min_max(
                pos2(card_rect.left() + 24.0, bar_top),
                pos2(card_rect.right() - 24.0, bar_top + 10.0),
            );
            painter.rect_filled(bar_rect, Rounding::same(5.0), p.subtle_glass_panel);

            let fill_w = (bar_rect.width() * job.visual_progress.clamp(0.06, 1.0)).max(12.0);
            let fill_rect = egui::Rect::from_min_max(
                bar_rect.min,
                pos2(bar_rect.left() + fill_w, bar_rect.bottom()),
            );
            let bar_col = if job.is_finished {
                p.emerald_optimal
            } else {
                p.border_selected
            };
            painter.rect_filled(fill_rect, Rounding::same(5.0), bar_col);

            if !job.is_finished {
                let shimmer_phase = (time * 1.4).fract();
                let shimmer_w = (fill_w * 0.35).clamp(16.0, 80.0);
                let shimmer_x = fill_rect.left() + (fill_w - shimmer_w) * shimmer_phase;
                let shimmer_rect = egui::Rect::from_min_max(
                    pos2(shimmer_x, fill_rect.top()),
                    pos2((shimmer_x + shimmer_w).min(fill_rect.right()), fill_rect.bottom()),
                );
                painter.rect_filled(
                    shimmer_rect,
                    Rounding::same(5.0),
                    egui::Color32::from_white_alpha(65),
                );
            }

            // Inline CRU / DisplayID 2.0 Display Driver Restart Action Row inside the progress modal
            if has_driver_bar {
                let div_y = card_rect.top() + 120.0;
                painter.line_segment(
                    [
                        pos2(card_rect.left() + 20.0, div_y),
                        pos2(card_rect.right() - 20.0, div_y),
                    ],
                    Stroke::new(1.0_f32, p.border_subtle),
                );

                let action_rect = egui::Rect::from_min_max(
                    pos2(card_rect.left() + 22.0, div_y + 12.0),
                    pos2(card_rect.right() - 22.0, card_rect.bottom() - 12.0),
                );
                let is_done = job.is_finished;
                let is_queued = job.driver_restart_queued;
                let restarted_status = job.driver_restarted_status.clone();

                egui::Area::new(egui::Id::new("zen_active_job_driver_action_area"))
                    .order(egui::Order::Foreground)
                    .fixed_pos(action_rect.min)
                    .show(ctx, |ui| {
                        ui.set_min_size(action_rect.size());
                        ui.set_max_size(action_rect.size());
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = 6.0;
                            draw_lucide_icon(ui, LucideIcon::Monitor, 15.0, p.text_heading);
                            let left_note = if let Some(st) = &restarted_status {
                                st.as_str()
                            } else {
                                "CRU / DisplayID 2.0 Block:"
                            };
                            ui.label(
                                RichText::new(left_note)
                                    .size(11.8)
                                    .strong()
                                    .color(if restarted_status.is_some() {
                                        p.emerald_optimal
                                    } else {
                                        p.text_body
                                    }),
                            );

                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                ui.spacing_mut().item_spacing.x = 8.0;
                                if is_done {
                                    if zen_pill_button(ui, self.theme_mode, "Close", false).clicked()
                                    {
                                        click_close_modal = true;
                                    }
                                }

                                let (btn_icon, btn_label) = if restarted_status.is_some() {
                                    (LucideIcon::Check, "Restart Display Driver Again")
                                } else if is_queued && !is_done {
                                    (LucideIcon::RotateCcw, "Driver Restart Queued (100%)")
                                } else {
                                    (LucideIcon::RotateCcw, "Restart Display Driver (CRU)")
                                };

                                if zen_cta_button(ui, self.theme_mode, Some(btn_icon), btn_label)
                                    .on_hover_text("Applies the VESA DisplayID 2.0 (CRU 1x1 Tiled Topology) block and immediately restarts the display driver via restart64 / pnputil.")
                                    .clicked()
                                {
                                    click_restart_driver = true;
                                }
                            });
                        });
                    });
            }
        } else if let Some((when, msg)) = &self.completion_toast {
            let elapsed = when.elapsed().as_secs_f32();
            if elapsed < 1.8 {
                ctx.request_repaint();
                let alpha = ((1.8_f32 - elapsed) / 0.45_f32).clamp(0.0, 1.0);
                let toast_layer = egui::LayerId::new(
                    egui::Order::Foreground,
                    egui::Id::new("zen_completion_toast"),
                );
                let painter = ctx.layer_painter(toast_layer);
                let galley = painter.layout_no_wrap(
                    msg.clone(),
                    egui::FontId::new(12.2, egui::FontFamily::Proportional),
                    p.text_heading.gamma_multiply(alpha),
                );
                let toast_w = galley.size().x + 48.0;
                let toast_h = 34.0;
                let toast_rect = egui::Rect::from_center_size(
                    pos2(screen_rect.center().x, screen_rect.bottom() - 62.0),
                    Vec2::new(toast_w, toast_h),
                );
                painter.rect(
                    toast_rect,
                    Rounding::same(17.0),
                    p.elevated_modal.gamma_multiply(alpha),
                    Stroke::new(1.2_f32, p.emerald_optimal.gamma_multiply(alpha)),
                );
                // Vector checkmark inside the toast
                let c_pos = pos2(toast_rect.left() + 18.0, toast_rect.center().y);
                let ck_stroke = Stroke::new(2.0_f32, p.emerald_optimal.gamma_multiply(alpha));
                painter.line_segment(
                    [pos2(c_pos.x - 4.5, c_pos.y + 0.5), pos2(c_pos.x - 1.2, c_pos.y + 3.8)],
                    ck_stroke,
                );
                painter.line_segment(
                    [pos2(c_pos.x - 1.2, c_pos.y + 3.8), pos2(c_pos.x + 5.0, c_pos.y - 3.2)],
                    ck_stroke,
                );
                painter.galley(
                    pos2(toast_rect.left() + 32.0, toast_rect.center().y - galley.size().y * 0.5),
                    galley,
                    p.text_heading.gamma_multiply(alpha),
                );
            } else {
                self.completion_toast = None;
            }
        }

        if click_restart_driver {
            let is_done = self
                .active_job
                .as_ref()
                .map(|j| j.is_finished)
                .unwrap_or(false);
            if is_done {
                let st = self.execute_modal_display_driver_restart();
                if let Some(job) = &mut self.active_job {
                    job.driver_restarted_status = Some(st);
                }
            } else if let Some(job) = &mut self.active_job {
                job.driver_restart_queued = true;
            }
        }

        if click_close_modal {
            if let Some(job) = self.active_job.take() {
                self.completion_toast = Some((Instant::now(), format!("{} — Applied", job.title)));
            }
        }
    }
    /// First-Launch Onboarding / Intro Screen:
    /// Guides the user through Administrator privilege verification, creating a Windows System Restore Point,
    /// configuring League of Legends & Riot Client installation paths, and reviewing auto-detected hardware.
    fn render_onboarding_screen(&mut self, ui: &mut Ui, ctx: &Context) {
        let p = self.theme_mode.palette();
        let avail_w = ui.available_width();
        let card_w = avail_w.min(860.0).max(520.0);
        let side_pad = ((avail_w - card_w) * 0.5).max(0.0);

        ui.horizontal(|ui| {
            ui.add_space(side_pad);
            ui.vertical(|ui| {
                ui.set_width(card_w);
                ui.set_max_width(card_w);

                zen_card_frame(self.theme_mode, true).show(ui, |ui| {
                    let inner_w = (card_w - 48.0).max(460.0);
                    ui.set_width(inner_w);
                    ui.set_max_width(inner_w);

                    // Intro Header
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 10.0;
                        if let Some(tex) = &self.brand_logo {
                            ui.add(
                                egui::Image::from_texture(tex)
                                    .fit_to_exact_size(Vec2::splat(32.0))
                                    .rounding(Rounding::same(7.0)),
                            );
                        }
                        ui.vertical(|ui| {
                            ui.label(
                                RichText::new("INITIAL SYSTEM SETUP & SAFETY CONFIGURATION")
                                    .size(16.0)
                                    .strong()
                                    .color(p.text_heading),
                            );
                            ui.label(
                                RichText::new("Verify Administrator access, create a Windows System Restore Point, and confirm your Riot Games installation paths before applying hardware optimizations.")
                                    .size(12.0)
                                    .color(p.text_secondary),
                            );
                        });
                    });

                    ui.add_space(14.0);

                    // Step 1: Administrator Privilege Verification
                    zen_subtle_frame(self.theme_mode).show(ui, |ui| {
                        ui.set_width(inner_w - 32.0);
                        ui.horizontal(|ui| {
                            draw_lucide_icon(ui, LucideIcon::Shield, 16.0, p.text_heading);
                            ui.label(
                                RichText::new("Step 1 — Administrator Privilege Verification")
                                    .size(13.8)
                                    .strong()
                                    .color(p.text_heading),
                            );
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                if self.is_admin {
                                    zen_semantic_badge(
                                        ui,
                                        self.theme_mode,
                                        BadgeTone::Optimal,
                                        "STATUS: ADMINISTRATOR",
                                    );
                                } else {
                                    if zen_cta_button(
                                        ui,
                                        self.theme_mode,
                                        Some(LucideIcon::Shield),
                                        "Relaunch as Administrator",
                                    )
                                    .clicked()
                                    {
                                        if let Err(e) = scheduler::relaunch_as_admin() {
                                            self.log(format!("[ERROR] {e}"));
                                        } else {
                                            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                                        }
                                    }
                                    zen_semantic_badge(
                                        ui,
                                        self.theme_mode,
                                        BadgeTone::Priority,
                                        "ELEVATION REQUIRED",
                                    );
                                }
                            });
                        });
                        ui.add_space(4.0);
                        let admin_desc = if self.is_admin {
                            "Elevated Administrator session verified. Full read/write access to HKLM hardware interrupt keys, power schemes, and Process Lasso configuration is enabled."
                        } else {
                            "Warning: Application was launched without Administrator privileges. Please click 'Relaunch as Administrator' so HKLM registry keys (MSI Mode, Interrupt Affinity, HAGS, Anti-Lag) and Process Lasso rules can be applied."
                        };
                        ui.label(RichText::new(admin_desc).size(11.8).color(p.text_secondary));
                    });

                    ui.add_space(10.0);

                    // Step 2: Windows System Restore Point & Baseline Snapshot
                    zen_subtle_frame(self.theme_mode).show(ui, |ui| {
                        ui.set_width(inner_w - 32.0);
                        ui.horizontal(|ui| {
                            draw_lucide_icon(ui, LucideIcon::RotateCcw, 16.0, p.text_heading);
                            ui.label(
                                RichText::new("Step 2 — Windows System Restore Point & Baseline Backup")
                                    .size(13.8)
                                    .strong()
                                    .color(p.text_heading),
                            );
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                if zen_pill_button_with_icons(
                                    ui,
                                    self.theme_mode,
                                    Some(LucideIcon::Shield),
                                    "Create OS Restore Point",
                                    None,
                                    !self.restore_point_created_in_session,
                                )
                                .clicked()
                                {
                                    self.restore_point_created_in_session = true;
                                    self.spawn_background_task(
                                        "Creating Windows Restore Point",
                                        "Executing Checkpoint-Computer via PowerShell...",
                                        move |_tx, _hw| match create_windows_restore_point() {
                                            Ok(m) => vec![m],
                                            Err(e) => vec![format!("[INFO] {e}")],
                                        },
                                    );
                                }
                                if self.restore_point_created_in_session {
                                    zen_semantic_badge(
                                        ui,
                                        self.theme_mode,
                                        BadgeTone::Optimal,
                                        "RESTORE POINT REQUESTED",
                                    );
                                } else {
                                    zen_semantic_badge(
                                        ui,
                                        self.theme_mode,
                                        BadgeTone::Info,
                                        "SNAPSHOT SAVED",
                                    );
                                }
                            });
                        });
                        ui.add_space(4.0);
                        ui.label(
                            RichText::new(
                                "Your original registry and driver values have been automatically backed up to backup_original_state.json. Creating a Windows System Restore Point is recommended before applying system-wide changes.",
                            )
                            .size(11.8)
                            .color(p.text_secondary),
                        );
                    });

                    ui.add_space(10.0);

                    // Step 3: League of Legends & Riot Client Installation Directories
                    zen_subtle_frame(self.theme_mode).show(ui, |ui| {
                        ui.set_width(inner_w - 32.0);
                        ui.horizontal(|ui| {
                            draw_lucide_icon(ui, LucideIcon::Sliders, 16.0, p.text_heading);
                            ui.label(
                                RichText::new("Step 3 — League of Legends & Riot Client Installation Directories")
                                    .size(13.8)
                                    .strong()
                                    .color(p.text_heading),
                            );
                        });
                        ui.add_space(4.0);
                        ui.label(
                            RichText::new(
                                "Specify your League of Legends and Riot Client installation folders. These paths are persisted in ui_preferences.json and used to register DirectX Independent Flip (SwapEffectUpgradeEnable=1) GPU profiles.",
                            )
                            .size(11.8)
                            .color(p.text_secondary),
                        );
                        ui.add_space(8.0);

                        let lol_exists = std::path::Path::new(&self.league_folder_path).exists();
                        ui.horizontal(|ui| {
                            ui.label(
                                RichText::new("League of Legends Folder:")
                                    .size(12.0)
                                    .strong()
                                    .color(p.text_body),
                            );
                            let resp = ui.add(
                                egui::TextEdit::singleline(&mut self.league_folder_path)
                                    .desired_width(380.0),
                            );
                            if resp.changed() {
                                self.sync_custom_folder_paths();
                            }
                            if zen_pill_button(ui, self.theme_mode, "Browse...", false).clicked() {
                                let current = self.league_folder_path.clone();
                                if let Some(picked) = Self::pick_folder_dialog(&current) {
                                    self.league_folder_path = picked;
                                    self.sync_custom_folder_paths();
                                }
                            }
                            if lol_exists {
                                zen_semantic_badge(ui, self.theme_mode, BadgeTone::Optimal, "VERIFIED");
                            } else {
                                zen_semantic_badge(ui, self.theme_mode, BadgeTone::Neutral, "CUSTOM PATH");
                            }
                        });

                        ui.add_space(6.0);

                        let riot_exists = std::path::Path::new(&self.riot_client_folder_path).exists();
                        ui.horizontal(|ui| {
                            ui.label(
                                RichText::new("Riot Client Folder:             ")
                                    .size(12.0)
                                    .strong()
                                    .color(p.text_body),
                            );
                            let resp = ui.add(
                                egui::TextEdit::singleline(&mut self.riot_client_folder_path)
                                    .desired_width(380.0),
                            );
                            if resp.changed() {
                                self.save_preferences();
                            }
                            if zen_pill_button(ui, self.theme_mode, "Browse...", false).clicked() {
                                let current = self.riot_client_folder_path.clone();
                                if let Some(picked) = Self::pick_folder_dialog(&current) {
                                    self.riot_client_folder_path = picked;
                                    self.save_preferences();
                                }
                            }
                            if riot_exists {
                                zen_semantic_badge(ui, self.theme_mode, BadgeTone::Optimal, "VERIFIED");
                            } else {
                                zen_semantic_badge(ui, self.theme_mode, BadgeTone::Neutral, "CUSTOM PATH");
                            }
                        });
                    });

                    ui.add_space(10.0);

                    // Step 4: Live Win32 Hardware Auto-Detection Summary
                    zen_subtle_frame(self.theme_mode).show(ui, |ui| {
                        ui.set_width(inner_w - 32.0);
                        ui.horizontal(|ui| {
                            draw_lucide_icon(ui, LucideIcon::Cpu, 16.0, p.text_heading);
                            ui.label(
                                RichText::new("Step 4 — Automatic Hardware & Topology Detection (Win32 Kernel API)")
                                    .size(13.8)
                                    .strong()
                                    .color(p.text_heading),
                            );
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                zen_semantic_badge(
                                    ui,
                                    self.theme_mode,
                                    BadgeTone::Optimal,
                                    "AUTO-DETECTED",
                                );
                            });
                        });
                        ui.add_space(4.0);
                        ui.label(
                            RichText::new(format!(
                                "Processor: {} ({} Physical Cores / {} Logical Threads — {})",
                                self.hw.cpu_name,
                                self.hw.physical_cores,
                                self.hw.logical_threads,
                                self.hw.cpu_plan.arch_class.display_name()
                            ))
                            .size(12.0)
                            .color(p.text_body),
                        );
                        ui.label(
                            RichText::new(format!(
                                "Graphics Adapter: {} (Active Driver Registry Instance: {})  |  Network: {}",
                                self.hw.gpu.name,
                                self.hw.gpu.driver_class_subkey,
                                self.hw.nic.description
                            ))
                            .size(11.8)
                            .color(p.text_secondary),
                        );
                        ui.label(
                            RichText::new(format!(
                                "Calculated Thread Plan: Primary Core #{} | Game Threads [{}] | Client & Vanguard Threads [{}]",
                                self.hw.cpu_plan.dedicated_lol_core_idx,
                                self.hw.cpu_plan.game_cpus_lasso,
                                self.hw.cpu_plan.vanguard_cpus_lasso
                            ))
                            .size(11.5)
                            .color(p.text_muted),
                        );
                    });

                    ui.add_space(16.0);

                    // Bottom CTA Row
                    ui.horizontal(|ui| {
                        ui.label(
                            RichText::new("You can reopen this Setup Wizard at any time from the top navigation bar.")
                                .size(11.5)
                                .color(p.text_muted),
                        );
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            if zen_cta_button(
                                ui,
                                self.theme_mode,
                                Some(LucideIcon::Check),
                                "Complete Setup & Open Dashboard",
                            )
                            .clicked()
                            {
                                self.onboarding_completed = true;
                                self.show_onboarding_screen = false;
                                self.sync_custom_folder_paths();
                                self.save_preferences();
                            }
                        });
                    });
                });
            });
        });
    }

    /// Startup Administrator Privilege Check Modal:
    /// Shown automatically if the application is launched without Administrator privileges.
    fn render_admin_required_modal(&mut self, ctx: &Context) {
        if self.is_admin || !self.show_admin_prompt_modal || self.active_job.is_some() {
            return;
        }
        let p = self.theme_mode.palette();
        let screen_rect = ctx.screen_rect();

        let backdrop_layer = egui::LayerId::new(
            egui::Order::Middle,
            egui::Id::new("zen_admin_prompt_backdrop"),
        );
        let backdrop_col = match self.theme_mode {
            ZenThemeMode::Light => egui::Color32::from_rgba_unmultiplied(15, 23, 42, 95),
            ZenThemeMode::Dark => egui::Color32::from_rgba_unmultiplied(4, 6, 12, 165),
        };
        ctx.layer_painter(backdrop_layer)
            .rect_filled(screen_rect, Rounding::ZERO, backdrop_col);

        egui::Area::new(egui::Id::new("zen_admin_required_modal_area"))
            .order(egui::Order::Foreground)
            .anchor(egui::Align2::CENTER_CENTER, Vec2::ZERO)
            .show(ctx, |ui| {
                let modal_w = 520.0_f32.min(screen_rect.width() - 40.0);
                Frame::none()
                    .fill(p.elevated_modal)
                    .rounding(Rounding::same(14.0))
                    .stroke(Stroke::new(1.5_f32, p.border_selected))
                    .inner_margin(Margin::symmetric(24.0, 20.0))
                    .show(ui, |ui| {
                        ui.set_width(modal_w);
                        ui.set_max_width(modal_w);

                        ui.horizontal(|ui| {
                            draw_lucide_icon(ui, LucideIcon::Shield, 18.0, p.gold_priority);
                            ui.label(
                                RichText::new("Administrator Privileges Required")
                                    .size(16.0)
                                    .strong()
                                    .color(p.text_heading),
                            );
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                zen_semantic_badge(
                                    ui,
                                    self.theme_mode,
                                    BadgeTone::Priority,
                                    "USER MODE",
                                );
                            });
                        });

                        ui.add_space(10.0);
                        ui.label(
                            RichText::new(
                                "League Unlagged was launched without Administrator privileges. Please run this application as Administrator.",
                            )
                            .size(13.0)
                            .strong()
                            .color(p.text_body),
                        );
                        ui.add_space(6.0);
                        ui.label(
                            RichText::new(
                                "Elevated Administrator access is required to modify HKLM hardware registry keys (MSI Mode, PCI Interrupt Affinity, HAGS, MMCSS, AMD Anti-Lag), configure Windows power schemes (powercfg), and write Process Lasso affinity profiles (C:\\ProgramData\\ProcessLasso\\config\\prolasso.ini).",
                            )
                            .size(12.0)
                            .color(p.text_secondary),
                        );

                        ui.add_space(16.0);
                        ui.horizontal(|ui| {
                            if zen_cta_button(
                                ui,
                                self.theme_mode,
                                Some(LucideIcon::Shield),
                                "Relaunch as Administrator (UAC)",
                            )
                            .clicked()
                            {
                                if let Err(e) = scheduler::relaunch_as_admin() {
                                    self.log(format!("[ERROR] {e}"));
                                    self.show_admin_prompt_modal = false;
                                } else {
                                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                                }
                            }

                            ui.add_space(8.0);
                            if zen_pill_button(
                                ui,
                                self.theme_mode,
                                "Continue in Read-Only Mode",
                                false,
                            )
                            .clicked()
                            {
                                self.show_admin_prompt_modal = false;
                            }
                        });
                    });
            });
    }

    fn crc32_bytes(data: &[u8]) -> u32 {
        let mut crc: u32 = 0xFFFF_FFFF;
        for &b in data {
            crc ^= b as u32;
            for _ in 0..8 {
                if (crc & 1) != 0 {
                    crc = (crc >> 1) ^ 0xEDB8_8320;
                } else {
                    crc >>= 1;
                }
            }
        }
        !crc
    }

    fn write_png_chunk(out: &mut Vec<u8>, chunk_type: &[u8; 4], data: &[u8]) {
        out.extend_from_slice(&(data.len() as u32).to_be_bytes());
        let start = out.len();
        out.extend_from_slice(chunk_type);
        out.extend_from_slice(data);
        let crc = Self::crc32_bytes(&out[start..]);
        out.extend_from_slice(&crc.to_be_bytes());
    }

    fn save_color_image_png(image: &egui::ColorImage, path: &std::path::Path) {
        let width = image.size[0] as u32;
        let height = image.size[1] as u32;
        let mut raw_scanlines = Vec::with_capacity((width as usize * 4 + 1) * height as usize);
        for y in 0..(height as usize) {
            raw_scanlines.push(0u8); // filter type 0 (None)
            let row_start = y * (width as usize);
            for x in 0..(width as usize) {
                let c = image.pixels[row_start + x];
                raw_scanlines.extend_from_slice(&[c.r(), c.g(), c.b(), c.a()]);
            }
        }

        // Build uncompressed zlib stream
        let mut zlib = Vec::with_capacity(raw_scanlines.len() + (raw_scanlines.len() / 65535 + 2) * 5 + 6);
        zlib.extend_from_slice(&[0x78, 0x01]);
        let chunks: Vec<&[u8]> = raw_scanlines.chunks(65535).collect();
        for (i, chunk) in chunks.iter().enumerate() {
            let is_last = i + 1 == chunks.len();
            zlib.push(if is_last { 0x01 } else { 0x00 });
            let len = chunk.len() as u16;
            let nlen = !len;
            zlib.extend_from_slice(&len.to_le_bytes());
            zlib.extend_from_slice(&nlen.to_le_bytes());
            zlib.extend_from_slice(chunk);
        }
        // Adler-32
        let mut a: u32 = 1;
        let mut b: u32 = 0;
        for &byte in &raw_scanlines {
            a = (a + byte as u32) % 65521;
            b = (b + a) % 65521;
        }
        let adler = (b << 16) | a;
        zlib.extend_from_slice(&adler.to_be_bytes());

        let mut png = Vec::with_capacity(zlib.len() + 128);
        png.extend_from_slice(&[137, 80, 78, 71, 13, 10, 26, 10]);

        let mut ihdr = Vec::with_capacity(13);
        ihdr.extend_from_slice(&width.to_be_bytes());
        ihdr.extend_from_slice(&height.to_be_bytes());
        ihdr.extend_from_slice(&[8, 6, 0, 0, 0]);
        Self::write_png_chunk(&mut png, b"IHDR", &ihdr);
        Self::write_png_chunk(&mut png, b"IDAT", &zlib);
        Self::write_png_chunk(&mut png, b"IEND", &[]);

        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let _ = std::fs::write(path, png);
    }

    fn handle_screenshot_export(&mut self, ctx: &Context) {
        let Some(step) = self.export_screenshot_frame else {
            return;
        };

        let mut captured_img: Option<std::sync::Arc<egui::ColorImage>> = None;
        ctx.input(|i| {
            for ev in &i.events {
                if let egui::Event::Screenshot { image, .. } = ev {
                    captured_img = Some(image.clone());
                }
            }
        });

        let assets_dir = std::path::Path::new("readme-assets");
        if let Some(img) = captured_img {
            match step {
                10 => {
                    Self::save_color_image_png(&img, &assets_dir.join("light.png"));
                    self.theme_mode = ZenThemeMode::Dark;
                    apply_zen_visuals(ctx, ZenThemeMode::Dark);
                    self.show_one_click_modal = true;
                    self.export_screenshot_frame = Some(20);
                }
                30 => {
                    Self::save_color_image_png(&img, &assets_dir.join("dark.png"));
                    self.theme_mode = ZenThemeMode::Dark;
                    apply_zen_visuals(ctx, ZenThemeMode::Dark);
                    self.show_one_click_modal = false;
                    self.active_tab = ActiveTab::SchedulerVanguard;
                    self.export_screenshot_frame = Some(40);
                }
                50 => {
                    Self::save_color_image_png(&img, &assets_dir.join("topology.png"));
                    self.theme_mode = ZenThemeMode::Light;
                    apply_zen_visuals(ctx, ZenThemeMode::Light);
                    self.show_one_click_modal = false;
                    self.active_tab = ActiveTab::Overview;
                    self.export_screenshot_frame = Some(60);
                }
                70 => {
                    Self::save_color_image_png(&img, &assets_dir.join("hardware.png"));
                    self.export_screenshot_frame = None;
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    return;
                }
                _ => {}
            }
        } else {
            match step {
                4 => {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot);
                    self.export_screenshot_frame = Some(10);
                }
                24 => {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot);
                    self.export_screenshot_frame = Some(30);
                }
                44 => {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot);
                    self.export_screenshot_frame = Some(50);
                }
                64 => {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot);
                    self.export_screenshot_frame = Some(70);
                }
                s if s < 4 || (s >= 20 && s < 24) || (s >= 40 && s < 44) || (s >= 60 && s < 64) => {
                    self.export_screenshot_frame = Some(s + 1);
                }
                _ => {}
            }
        }
        ctx.request_repaint();
    }
}

impl eframe::App for LeagueUnlaggedApp {
    fn update(&mut self, ctx: &Context, _frame: &mut eframe::Frame) {
        self.poll_background_job(ctx);
        let p = self.theme_mode.palette();

        // Render custom frameless titlebar at the top
        self.render_custom_titlebar(ctx);

        // Render single-line Footer Status Bar (+ optional sliding drawer)
        self.render_bottom_status_bar(ctx);

        // Main Central Canvas with generous 26px horizontal padding
        egui::CentralPanel::default()
            .frame(
                Frame::none()
                    .fill(p.app_bg)
                    .inner_margin(Margin::symmetric(26.0, 18.0)),
            )
            .show(ctx, |ui| {
                self.render_header(ui, ctx);

                ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        if self.show_onboarding_screen {
                            self.render_onboarding_screen(ui, ctx);
                        } else {
                            match self.active_tab {
                                ActiveTab::Overview => self.render_overview_tab(ui),
                                ActiveTab::WindowsReg => self.render_windows_reg_tab(ui),
                                ActiveTab::SchedulerVanguard => {
                                    self.render_scheduler_vanguard_tab(ui)
                                }
                                ActiveTab::Interrupts => self.render_interrupts_tab(ui),
                                ActiveTab::NetworkGpu => self.render_network_gpu_tab(ui),
                                ActiveTab::TimerDisplayBackup => {
                                    self.render_timer_display_backup_tab(ui)
                                }
                            }
                        }
                        ui.add_space(24.0);
                    });
            });

        // Render startup Administrator privilege check modal if launched without elevation
        self.render_admin_required_modal(ctx);

        // Render animated background job overlay & completion toast
        self.render_active_job_overlay(ctx);

        // Crisp 1px outer frame border around the frameless window
        let screen_rect = ctx.screen_rect();
        ctx.layer_painter(egui::LayerId::new(
            egui::Order::Foreground,
            egui::Id::new("zen_window_outer_border"),
        ))
        .rect_stroke(
            screen_rect.shrink(0.5),
            Rounding::ZERO,
            Stroke::new(1.0_f32, p.border_selected),
        );

        self.handle_screenshot_export(ctx);
    }
}
