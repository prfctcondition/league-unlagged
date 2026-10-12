#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

pub mod app;
pub mod backup;
pub mod hardware;
pub mod icons;
pub mod theme;
pub mod tweaks;

use app::LeagueUnlaggedApp;

fn main() -> eframe::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    if args.iter().any(|a| a == "--cli-report") {
        let hw = hardware::SystemHardwareReport::scan_system();
        let win = tweaks::windows_reg::WindowsRegState::refresh(&hw.league_exe_paths);
        let sched = tweaks::scheduler::SchedulerState::refresh(&hw.cpu_plan);
        let irq = tweaks::interrupts::InterruptsState::refresh(&hw);
        let net = tweaks::network::NetworkState::refresh(&hw.nic.name);
        let gpu = tweaks::gpu::GpuTweaksState::refresh(&hw.gpu);
        println!("=== LEAGUE UNLAGGED HARDWARE & STATE REPORT ===");
        println!("OS: {} (Build {})", hw.os_caption, hw.os_build);
        println!("CPU: {} ({}C/{}T) -> {:?}", hw.cpu_name, hw.physical_cores, hw.logical_threads, hw.cpu_plan.arch_class);
        println!("Dedicated LoL Core: Core #{}", hw.cpu_plan.dedicated_lol_core_idx);
        println!("Game Mask: 0x{:04X} (CPUs: {})", hw.cpu_plan.game_mask, hw.cpu_plan.game_cpus_lasso);
        println!("Client/Vanguard Mask: 0x{:04X} (CPUs: {})", hw.cpu_plan.vanguard_mask, hw.cpu_plan.vanguard_cpus_lasso);
        println!("GPU: {} ({:?}) [Subkey: {}]", hw.gpu.name, hw.gpu.vendor, hw.gpu.driver_class_subkey);
        println!("USB Controllers: {}", hw.usb_controllers.len());
        for (i, u) in hw.usb_controllers.iter().enumerate() {
            println!("  USB #{}: {} (CPU SoC Direct: {}) -> Peripherals: {:?}", i + 1, u.name, u.is_cpu_soc_direct, u.connected_peripherals);
        }
        println!("Windows Reg State: {:?}", win);
        println!("Scheduler State: {:?}", sched);
        println!("Interrupts State: {:?}", irq);
        println!("Network State: {:?}", net);
        println!("GPU Tweaks State: {:?}", gpu);
        return Ok(());
    }

    let mut viewport = egui::ViewportBuilder::default()
        .with_title("League Unlagged — Hardware-Adaptive Latency & FPS Suite")
        .with_decorations(false)
        .with_inner_size([1260.0, 840.0])
        .with_min_inner_size([1120.0, 720.0]);

    if let Some(icon_data) = icons::load_unlagged_window_icon() {
        viewport = viewport.with_icon(std::sync::Arc::new(icon_data));
    }

    let options = eframe::NativeOptions {
        viewport,
        ..Default::default()
    };

    eframe::run_native(
        "League Unlagged",
        options,
        Box::new(|cc| Ok(Box::new(LeagueUnlaggedApp::new(cc)))),
    )
}
