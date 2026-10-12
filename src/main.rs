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
    if let Some(pos) = args.iter().position(|a| a == "--timer-daemon") {
        let desired_100ns = args
            .get(pos + 1)
            .and_then(|s| s.parse::<u32>().ok())
            .unwrap_or(5050);
        tweaks::scheduler::run_timer_daemon_loop(desired_100ns);
        return Ok(());
    }
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

    if !args.iter().any(|a| a == "--export-screenshots") {
        let mutex_name: Vec<u16> = "Local\\LeagueUnlagged_SingleInstance_Mutex_v1\0"
            .encode_utf16()
            .collect();
        let _handle = unsafe {
            windows_sys::Win32::System::Threading::CreateMutexW(
                std::ptr::null(),
                1,
                mutex_name.as_ptr(),
            )
        };
        let last_err = unsafe { windows_sys::Win32::Foundation::GetLastError() };
        if last_err == windows_sys::Win32::Foundation::ERROR_ALREADY_EXISTS
            || last_err == windows_sys::Win32::Foundation::ERROR_ACCESS_DENIED
        {
            let win_title: Vec<u16> =
                "League Unlagged — Hardware-Adaptive Latency & FPS Suite\0"
                    .encode_utf16()
                    .collect();
            let hwnd = unsafe {
                windows_sys::Win32::UI::WindowsAndMessaging::FindWindowW(
                    std::ptr::null(),
                    win_title.as_ptr(),
                )
            };
            if !hwnd.is_null() {
                unsafe {
                    windows_sys::Win32::UI::WindowsAndMessaging::ShowWindow(
                        hwnd,
                        windows_sys::Win32::UI::WindowsAndMessaging::SW_RESTORE,
                    );
                    windows_sys::Win32::UI::WindowsAndMessaging::SetForegroundWindow(hwnd);
                }
            }
            return Ok(());
        }
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
