#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

#[path = "../icons.rs"]
pub mod icons;
#[path = "../theme.rs"]
pub mod theme;

use egui::{
    pos2, Align, Context, Frame, Layout, Margin, RichText, Rounding, Sense, Stroke, TextureHandle,
    Vec2,
};
use icons::{
    build_unlagged_ico_bytes, draw_lucide_icon, load_unlagged_brand_texture,
    load_unlagged_window_icon, LucideIcon,
};
use std::path::{Path, PathBuf};
use theme::{
    apply_zen_visuals, configure_zen_fonts, zen_card_frame, zen_cta_button, zen_pill_button,
    zen_subtle_frame, zen_toggle_switch, zen_window_control_button, ZenThemeMode,
};
use winreg::enums::*;
use winreg::RegKey;

const PAYLOAD_APP_EXE: &[u8] = include_bytes!("../../target/release/league-unlagged.exe");
const PAYLOAD_README: &[u8] = include_bytes!("../../README.md");
const PAYLOAD_LICENSE: &[u8] = include_bytes!("../../LICENSE");
const PAYLOAD_CRU_EXE: &[u8] = include_bytes!("../../tools/cru/CRU.exe");
const PAYLOAD_RESTART_EXE: &[u8] = include_bytes!("../../tools/cru/restart.exe");
const PAYLOAD_RESTART64_EXE: &[u8] = include_bytes!("../../tools/cru/restart64.exe");
const PAYLOAD_RESET_ALL_EXE: &[u8] = include_bytes!("../../tools/cru/reset-all.exe");
const PAYLOAD_CRU_INFO: &[u8] = include_bytes!("../../tools/cru/Info.txt");

fn is_running_as_admin() -> bool {
    RegKey::predef(HKEY_LOCAL_MACHINE)
        .open_subkey_with_flags("SYSTEM\\CurrentControlSet\\Control", KEY_WRITE)
        .is_ok()
}

fn relaunch_self_as_admin() -> Result<(), String> {
    use windows_sys::Win32::UI::Shell::ShellExecuteW;
    use windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

    let exe_path = std::env::current_exe().map_err(|e| format!("Failed to locate installer path: {e}"))?;
    let exe_str = exe_path.display().to_string();

    let verb: Vec<u16> = "runas\0".encode_utf16().collect();
    let file: Vec<u16> = format!("{exe_str}\0").encode_utf16().collect();
    let params: Vec<u16> = "--elevated\0".encode_utf16().collect();

    let res = unsafe {
        ShellExecuteW(
            std::ptr::null_mut(),
            verb.as_ptr(),
            file.as_ptr(),
            params.as_ptr(),
            std::ptr::null(),
            SW_SHOWNORMAL,
        )
    };
    if (res as isize) <= 32 {
        return Err(format!("UAC elevation declined or failed (code: {}).", res as isize));
    }
    Ok(())
}

fn launch_installed_app_as_admin(installed_exe: &Path) -> Result<(), String> {
    use windows_sys::Win32::UI::Shell::ShellExecuteW;
    use windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

    let exe_str = installed_exe.display().to_string();
    let dir_str = installed_exe
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .display()
        .to_string();

    let verb: Vec<u16> = "runas\0".encode_utf16().collect();
    let file: Vec<u16> = format!("{exe_str}\0").encode_utf16().collect();
    let dir: Vec<u16> = format!("{dir_str}\0").encode_utf16().collect();

    let res = unsafe {
        ShellExecuteW(
            std::ptr::null_mut(),
            verb.as_ptr(),
            file.as_ptr(),
            std::ptr::null(),
            dir.as_ptr(),
            SW_SHOWNORMAL,
        )
    };
    if (res as isize) <= 32 {
        return Err(format!("Failed to launch League Unlagged (code: {}).", res as isize));
    }
    Ok(())
}

fn default_install_dir(is_admin: bool) -> String {
    if is_admin {
        if let Ok(pf) = std::env::var("ProgramFiles") {
            return Path::new(&pf)
                .join("League Unlagged")
                .display()
                .to_string();
        }
        r"C:\Program Files\League Unlagged".to_string()
    } else if let Ok(local) = std::env::var("LOCALAPPDATA") {
        Path::new(&local)
            .join("Programs")
            .join("League Unlagged")
            .display()
            .to_string()
    } else {
        r"C:\League Unlagged".to_string()
    }
}

/// Creates a Windows `.lnk` shortcut via WScript.Shell and sets bit `0x20` (`SLDF_RUNAS_USER`)
/// at byte offset `0x15` of the `.lnk` binary header so Windows marks "Run as administrator" enabled.
fn create_admin_shortcut(
    lnk_path: &Path,
    target_exe: &Path,
    working_dir: &Path,
    icon_path: &Path,
) -> Result<(), String> {
    let lnk_str = lnk_path.display().to_string().replace('\'', "''");
    let target_str = target_exe.display().to_string().replace('\'', "''");
    let work_str = working_dir.display().to_string().replace('\'', "''");
    let icon_str = icon_path.display().to_string().replace('\'', "''");

    let ps_script = format!(
        "$ws = New-Object -ComObject WScript.Shell; \
         $s = $ws.CreateShortcut('{lnk_str}'); \
         $s.TargetPath = '{target_str}'; \
         $s.WorkingDirectory = '{work_str}'; \
         $s.Description = 'League Unlagged - Hardware-Adaptive Input Latency & Topology Optimizer'; \
         $s.IconLocation = '{icon_str},0'; \
         $s.Save()"
    );

    let out = std::process::Command::new("powershell")
        .args(["-NoProfile", "-WindowStyle", "Hidden", "-Command", &ps_script])
        .output()
        .map_err(|e| format!("Failed to invoke PowerShell for shortcut creation: {e}"))?;

    if !out.status.success() {
        return Err(format!(
            "Shortcut creation failed: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }

    // Patch byte 0x15 (21) with 0x20 (SLDF_RUNAS_USER) in the [MS-SHLLINK] binary header
    let mut bytes = std::fs::read(lnk_path)
        .map_err(|e| format!("Failed to read shortcut '{}': {e}", lnk_path.display()))?;
    if bytes.len() > 0x15 {
        bytes[0x15] |= 0x20;
        std::fs::write(lnk_path, &bytes)
            .map_err(|e| format!("Failed to set RunAsAdmin flag on '{}': {e}", lnk_path.display()))?;
    }

    Ok(())
}

fn register_run_as_admin_compat_layer(target_exe: &Path) {
    let exe_str = target_exe.display().to_string();
    let subkey = r"Software\Microsoft\Windows NT\CurrentVersion\AppCompatFlags\Layers";

    if let Ok((key, _)) = RegKey::predef(HKEY_CURRENT_USER).create_subkey(subkey) {
        let _ = key.set_value(&exe_str, &"~ RUNASADMIN");
    }
    if let Ok((key, _)) = RegKey::predef(HKEY_LOCAL_MACHINE).create_subkey(subkey) {
        let _ = key.set_value(&exe_str, &"~ RUNASADMIN");
    }
}

fn register_uninstall_entry(install_dir: &Path, target_exe: &Path, icon_path: &Path) {
    let subkey = r"Software\Microsoft\Windows\CurrentVersion\Uninstall\LeagueUnlagged";
    let root = if is_running_as_admin() {
        RegKey::predef(HKEY_LOCAL_MACHINE)
    } else {
        RegKey::predef(HKEY_CURRENT_USER)
    };
    if let Ok((key, _)) = root.create_subkey(subkey) {
        let _ = key.set_value("DisplayName", &"League Unlagged");
        let _ = key.set_value("DisplayVersion", &"1.0.0");
        let _ = key.set_value("Publisher", &"prfctCondition");
        let _ = key.set_value("InstallLocation", &install_dir.display().to_string());
        let _ = key.set_value("DisplayIcon", &icon_path.display().to_string());
        let _ = key.set_value("UninstallString", &format!("cmd.exe /c rmdir /s /q \"{}\"", install_dir.display()));
        let _ = key.set_value("NoModify", &1u32);
        let _ = key.set_value("NoRepair", &1u32);
        let _ = key.set_value("ExecutablePath", &target_exe.display().to_string());
    }
}

fn perform_installation(
    install_dir: &Path,
    create_desktop_lnk: bool,
    create_start_menu_lnk: bool,
) -> Result<(PathBuf, Vec<String>), String> {
    let mut logs = Vec::new();
    let _ = std::process::Command::new("taskkill")
        .args(["/F", "/IM", "league-unlagged.exe"])
        .output();

    std::fs::create_dir_all(install_dir)
        .map_err(|e| format!("Failed to create directory '{}': {e}", install_dir.display()))?;

    let cru_dir = install_dir.join("tools").join("cru");
    std::fs::create_dir_all(&cru_dir)
        .map_err(|e| format!("Failed to create CRU tools directory: {e}"))?;

    let target_exe = install_dir.join("league-unlagged.exe");
    std::fs::write(&target_exe, PAYLOAD_APP_EXE)
        .map_err(|e| format!("Failed to write '{}': {e}", target_exe.display()))?;
    logs.push(format!("Installed binary: {}", target_exe.display()));

    let icon_path = install_dir.join("unlagged.ico");
    if let Some(ico_bytes) = build_unlagged_ico_bytes() {
        let _ = std::fs::write(&icon_path, ico_bytes);
    }

    let _ = std::fs::write(install_dir.join("README.md"), PAYLOAD_README);
    let _ = std::fs::write(install_dir.join("LICENSE"), PAYLOAD_LICENSE);
    let _ = std::fs::write(cru_dir.join("CRU.exe"), PAYLOAD_CRU_EXE);
    let _ = std::fs::write(cru_dir.join("restart.exe"), PAYLOAD_RESTART_EXE);
    let _ = std::fs::write(cru_dir.join("restart64.exe"), PAYLOAD_RESTART64_EXE);
    let _ = std::fs::write(cru_dir.join("reset-all.exe"), PAYLOAD_RESET_ALL_EXE);
    let _ = std::fs::write(cru_dir.join("Info.txt"), PAYLOAD_CRU_INFO);
    logs.push("Extracted DisplayID 2.0 & CRU driver utilities.".to_string());

    // Register AppCompatFlags\Layers = "~ RUNASADMIN" so Windows always elevates league-unlagged.exe
    register_run_as_admin_compat_layer(&target_exe);
    logs.push("Configured Windows elevation policy (~ RUNASADMIN) for league-unlagged.exe.".to_string());

    if create_desktop_lnk {
        if let Ok(userprofile) = std::env::var("USERPROFILE") {
            let desktop_lnk = Path::new(&userprofile)
                .join("Desktop")
                .join("League Unlagged.lnk");
            create_admin_shortcut(&desktop_lnk, &target_exe, install_dir, &icon_path)?;
            logs.push(format!(
                "Created Desktop shortcut with Administrator privilege flag: {}",
                desktop_lnk.display()
            ));
        }
    }

    if create_start_menu_lnk {
        if let Ok(appdata) = std::env::var("APPDATA") {
            let start_menu_lnk = Path::new(&appdata)
                .join("Microsoft")
                .join("Windows")
                .join("Start Menu")
                .join("Programs")
                .join("League Unlagged.lnk");
            let _ = create_admin_shortcut(&start_menu_lnk, &target_exe, install_dir, &icon_path);
            logs.push(format!(
                "Created Start Menu shortcut with Administrator privilege flag: {}",
                start_menu_lnk.display()
            ));
        }
    }

    register_uninstall_entry(install_dir, &target_exe, &icon_path);
    Ok((target_exe, logs))
}

struct InstallerApp {
    theme_mode: ZenThemeMode,
    brand_logo: Option<TextureHandle>,
    is_admin: bool,
    install_dir: String,
    create_desktop_shortcut: bool,
    create_start_menu_shortcut: bool,
    launch_after_install: bool,
    install_completed: bool,
    installed_exe_path: Option<PathBuf>,
    status_message: Option<(bool, String)>,
    install_logs: Vec<String>,
}

impl InstallerApp {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let theme_mode = ZenThemeMode::Light;
        configure_zen_fonts(&cc.egui_ctx);
        apply_zen_visuals(&cc.egui_ctx, theme_mode);
        let brand_logo = load_unlagged_brand_texture(&cc.egui_ctx);
        let is_admin = is_running_as_admin();

        Self {
            theme_mode,
            brand_logo,
            is_admin,
            install_dir: default_install_dir(is_admin),
            create_desktop_shortcut: true,
            create_start_menu_shortcut: true,
            launch_after_install: true,
            install_completed: false,
            installed_exe_path: None,
            status_message: None,
            install_logs: Vec::new(),
        }
    }

    fn pick_folder_dialog(initial_dir: &str) -> Option<String> {
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

    fn render_titlebar(&mut self, ctx: &Context) {
        let p = self.theme_mode.palette();
        egui::TopBottomPanel::top("installer_custom_titlebar")
            .exact_height(36.0)
            .frame(
                Frame::none()
                    .fill(p.main_glass_panel)
                    .stroke(Stroke::new(1.0_f32, p.border_subtle))
                    .inner_margin(Margin::symmetric(14.0, 0.0)),
            )
            .show(ctx, |ui| {
                let full_rect = ui.max_rect();
                let cy = full_rect.center().y;

                let drag_rect = egui::Rect::from_min_max(
                    full_rect.min,
                    pos2((full_rect.right() - 88.0).max(full_rect.left()), full_rect.bottom()),
                );
                let drag_resp = ui.interact(
                    drag_rect,
                    ui.id().with("installer_title_drag"),
                    Sense::click_and_drag(),
                );
                if drag_resp.drag_started_by(egui::PointerButton::Primary) {
                    ctx.send_viewport_cmd(egui::ViewportCommand::StartDrag);
                }

                let painter = ui.painter();
                let title_galley = painter.layout_no_wrap(
                    "LEAGUE UNLAGGED SETUP".to_string(),
                    egui::FontId::new(11.5, egui::FontFamily::Proportional),
                    p.text_heading,
                );
                let sub_galley = painter.layout_no_wrap(
                    "|  Windows Installer v1.0.0".to_string(),
                    egui::FontId::new(11.0, egui::FontFamily::Proportional),
                    p.text_muted,
                );

                let total_w = 24.0 + title_galley.size().x + 8.0 + sub_galley.size().x;
                let mut cx = (full_rect.center().x - total_w * 0.5).max(full_rect.left() + 8.0);

                if let Some(tex) = &self.brand_logo {
                    let r = egui::Rect::from_center_size(pos2(cx + 9.0, cy), Vec2::splat(18.0));
                    painter.image(
                        tex.id(),
                        r,
                        egui::Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
                        egui::Color32::WHITE,
                    );
                    cx += 24.0;
                }
                painter.galley(
                    pos2(cx, cy - title_galley.size().y * 0.5),
                    title_galley.clone(),
                    p.text_heading,
                );
                cx += title_galley.size().x + 8.0;
                painter.galley(
                    pos2(cx, cy - sub_galley.size().y * 0.5),
                    sub_galley,
                    p.text_muted,
                );

                ui.allocate_new_ui(egui::UiBuilder::new().max_rect(full_rect), |ui| {
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if zen_window_control_button(
                            ui,
                            self.theme_mode,
                            LucideIcon::Close,
                            true,
                        )
                        .clicked()
                        {
                            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                        }
                        if zen_window_control_button(
                            ui,
                            self.theme_mode,
                            LucideIcon::Minus,
                            false,
                        )
                        .clicked()
                        {
                            ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(true));
                        }
                    });
                });
            });
    }
}

impl eframe::App for InstallerApp {
    fn update(&mut self, ctx: &Context, _frame: &mut eframe::Frame) {
        let p = self.theme_mode.palette();
        self.render_titlebar(ctx);

        egui::CentralPanel::default()
            .frame(
                Frame::none()
                    .fill(p.app_bg)
                    .inner_margin(Margin::symmetric(24.0, 20.0)),
            )
            .show(ctx, |ui| {
                zen_card_frame(self.theme_mode, true).show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.horizontal(|ui| {
                        if let Some(tex) = &self.brand_logo {
                            ui.image((tex.id(), Vec2::splat(42.0)));
                            ui.add_space(10.0);
                        }
                        ui.vertical(|ui| {
                            ui.label(
                                RichText::new("League Unlagged v1.0.0 — Official Setup")
                                    .size(18.0)
                                    .strong()
                                    .color(p.text_heading),
                            );
                            ui.label(
                                RichText::new(
                                    "Hardware-Adaptive Input Latency, Core Isolation & Interrupt Utility",
                                )
                                .size(12.0)
                                .color(p.text_secondary),
                            );
                        });
                    });

                    ui.add_space(16.0);

                    if !self.install_completed {
                        zen_subtle_frame(self.theme_mode).show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            ui.label(
                                RichText::new("1. Target Installation Directory")
                                    .size(13.0)
                                    .strong()
                                    .color(p.text_heading),
                            );
                            ui.add_space(6.0);
                            ui.horizontal(|ui| {
                                let edit = egui::TextEdit::singleline(&mut self.install_dir)
                                    .desired_width(410.0)
                                    .font(egui::TextStyle::Monospace);
                                ui.add(edit);
                                if zen_pill_button(ui, self.theme_mode, "Browse...", false).clicked()
                                {
                                    if let Some(picked) =
                                        Self::pick_folder_dialog(&self.install_dir)
                                    {
                                        self.install_dir = picked;
                                    }
                                }
                            });
                        });

                        ui.add_space(12.0);

                        zen_subtle_frame(self.theme_mode).show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            ui.label(
                                RichText::new("2. Shortcuts & Administrator Elevation")
                                    .size(13.0)
                                    .strong()
                                    .color(p.text_heading),
                            );
                            ui.add_space(8.0);

                            ui.horizontal(|ui| {
                                zen_toggle_switch(
                                    ui,
                                    self.theme_mode,
                                    &mut self.create_desktop_shortcut,
                                );
                                ui.add_space(6.0);
                                ui.label(
                                    RichText::new(
                                        "Create Desktop Shortcut (Run as Administrator enabled)",
                                    )
                                    .size(12.5)
                                    .strong()
                                    .color(p.text_body),
                                );
                            });

                            ui.add_space(6.0);
                            ui.horizontal(|ui| {
                                zen_toggle_switch(
                                    ui,
                                    self.theme_mode,
                                    &mut self.create_start_menu_shortcut,
                                );
                                ui.add_space(6.0);
                                ui.label(
                                    RichText::new(
                                        "Create Start Menu Shortcut (Run as Administrator enabled)",
                                    )
                                    .size(12.5)
                                    .color(p.text_body),
                                );
                            });

                            ui.add_space(6.0);
                            ui.horizontal(|ui| {
                                zen_toggle_switch(
                                    ui,
                                    self.theme_mode,
                                    &mut self.launch_after_install,
                                );
                                ui.add_space(6.0);
                                ui.label(
                                    RichText::new(
                                        "Launch League Unlagged as Administrator when setup finishes",
                                    )
                                    .size(12.5)
                                    .color(p.text_body),
                                );
                            });
                        });

                        if let Some((ok, msg)) = &self.status_message {
                            ui.add_space(10.0);
                            let col = if *ok { p.emerald_optimal } else { p.gold_priority };
                            ui.label(RichText::new(msg).size(12.0).color(col));
                        }

                        ui.add_space(18.0);
                        ui.horizontal(|ui| {
                            if zen_cta_button(
                                ui,
                                self.theme_mode,
                                Some(LucideIcon::Shield),
                                "Install League Unlagged v1.0.0",
                            )
                            .clicked()
                            {
                                let target_dir = PathBuf::from(self.install_dir.trim());
                                match perform_installation(
                                    &target_dir,
                                    self.create_desktop_shortcut,
                                    self.create_start_menu_shortcut,
                                ) {
                                    Ok((exe_path, logs)) => {
                                        self.install_logs = logs;
                                        self.installed_exe_path = Some(exe_path.clone());
                                        self.install_completed = true;
                                        self.status_message = Some((
                                            true,
                                            "League Unlagged v1.0.0 installed successfully."
                                                .to_string(),
                                        ));
                                        if self.launch_after_install {
                                            let _ = launch_installed_app_as_admin(&exe_path);
                                        }
                                    }
                                    Err(err) => {
                                        self.status_message = Some((false, err));
                                    }
                                }
                            }

                            if !self.is_admin {
                                ui.add_space(8.0);
                                if zen_pill_button(
                                    ui,
                                    self.theme_mode,
                                    "Elevate Setup as Admin",
                                    false,
                                )
                                .clicked()
                                {
                                    if relaunch_self_as_admin().is_ok() {
                                        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                                    }
                                }
                            }
                        });
                    } else {
                        zen_subtle_frame(self.theme_mode).show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            ui.horizontal(|ui| {
                                draw_lucide_icon(ui, LucideIcon::Check, 18.0, p.emerald_optimal);
                                ui.label(
                                    RichText::new(
                                        "Installation Completed Successfully",
                                    )
                                    .size(14.5)
                                    .strong()
                                    .color(p.emerald_optimal),
                                );
                            });
                            ui.add_space(8.0);
                            for line in &self.install_logs {
                                ui.label(
                                    RichText::new(format!("- {line}"))
                                        .size(11.5)
                                        .monospace()
                                        .color(p.text_secondary),
                                );
                            }
                        });

                        ui.add_space(18.0);
                        ui.horizontal(|ui| {
                            if zen_cta_button(
                                ui,
                                self.theme_mode,
                                Some(LucideIcon::Zap),
                                "Launch League Unlagged & Exit Setup",
                            )
                            .clicked()
                            {
                                if let Some(exe) = &self.installed_exe_path {
                                    let _ = launch_installed_app_as_admin(exe);
                                }
                                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                            }

                            ui.add_space(8.0);
                            if zen_pill_button(ui, self.theme_mode, "Close Setup", false).clicked()
                            {
                                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                            }
                        });
                    }
                });
            });

        let screen_rect = ctx.screen_rect();
        ctx.layer_painter(egui::LayerId::new(
            egui::Order::Foreground,
            egui::Id::new("installer_outer_border"),
        ))
        .rect_stroke(
            screen_rect.shrink(0.5),
            Rounding::ZERO,
            Stroke::new(1.0_f32, p.border_selected),
        );
    }
}

fn main() -> eframe::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let is_admin = is_running_as_admin();

    if args.iter().any(|a| a == "--silent") {
        let dir = PathBuf::from(default_install_dir(is_admin));
        if let Err(e) = perform_installation(&dir, true, true) {
            eprintln!("Silent installation failed: {e}");
            std::process::exit(1);
        }
        return Ok(());
    }

    // Automatically request Administrator elevation on first launch unless already elevated or --elevated passed
    if !is_admin && !args.iter().any(|a| a == "--elevated") {
        if relaunch_self_as_admin().is_ok() {
            return Ok(());
        }
    }

    let mut viewport = egui::ViewportBuilder::default()
        .with_title("League Unlagged v1.0.0 — Windows Setup")
        .with_decorations(false)
        .with_inner_size([640.0, 440.0])
        .with_resizable(false);

    if let Some(icon_data) = load_unlagged_window_icon() {
        viewport = viewport.with_icon(std::sync::Arc::new(icon_data));
    }

    let options = eframe::NativeOptions {
        viewport,
        ..Default::default()
    };

    eframe::run_native(
        "League Unlagged Setup",
        options,
        Box::new(|cc| Ok(Box::new(InstallerApp::new(cc)))),
    )
}
