#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

#[path = "../icons.rs"]
pub mod icons;
#[path = "../theme.rs"]
pub mod theme;

use egui::{
    pos2, Color32, Context, FontFamily, FontId, Frame, Margin, Rect, RichText, Rounding, Sense,
    Stroke, TextureHandle, Ui, Vec2,
};
use icons::{
    build_unlagged_ico_bytes, draw_lucide_icon, load_unlagged_brand_texture,
    load_unlagged_window_icon, paint_lucide_icon, LucideIcon,
};
use std::path::{Path, PathBuf};
use theme::{apply_zen_visuals, configure_zen_fonts, zen_card_frame, ZenThemeMode};
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

    let exe_path =
        std::env::current_exe().map_err(|e| format!("Failed to locate installer path: {e}"))?;
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
        return Err(format!(
            "UAC elevation declined or failed (code: {}).",
            res as isize
        ));
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
        return Err(format!(
            "Failed to launch League Unlagged (code: {}).",
            res as isize
        ));
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
        .args([
            "-NoProfile",
            "-WindowStyle",
            "Hidden",
            "-Command",
            &ps_script,
        ])
        .output()
        .map_err(|e| format!("Failed to invoke PowerShell for shortcut creation: {e}"))?;

    if !out.status.success() {
        return Err(format!(
            "Shortcut creation failed: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }

    let mut bytes = std::fs::read(lnk_path)
        .map_err(|e| format!("Failed to read shortcut '{}': {e}", lnk_path.display()))?;
    if bytes.len() > 0x15 {
        bytes[0x15] |= 0x20;
        std::fs::write(lnk_path, &bytes).map_err(|e| {
            format!(
                "Failed to set RunAsAdmin flag on '{}': {e}",
                lnk_path.display()
            )
        })?;
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
        let _ = key.set_value(
            "UninstallString",
            &format!("cmd.exe /c rmdir /s /q \"{}\"", install_dir.display()),
        );
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

    register_run_as_admin_compat_layer(&target_exe);
    logs.push("Configured Windows elevation policy (~ RUNASADMIN).".to_string());

    if create_desktop_lnk {
        if let Ok(userprofile) = std::env::var("USERPROFILE") {
            let desktop_lnk = Path::new(&userprofile)
                .join("Desktop")
                .join("League Unlagged.lnk");
            create_admin_shortcut(&desktop_lnk, &target_exe, install_dir, &icon_path)?;
            logs.push(format!(
                "Desktop shortcut created (Run as Admin): {}",
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
            logs.push("Start Menu shortcut created (Run as Admin).".to_string());
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
    install_completed: bool,
    installed_exe_path: Option<PathBuf>,
    status_message: Option<(bool, String)>,
    install_logs: Vec<String>,
    export_screenshot_step: Option<u32>,
}

impl InstallerApp {
    fn new(cc: &eframe::CreationContext<'_>, export_screenshot: bool) -> Self {
        let theme_mode = ZenThemeMode::Light;
        configure_zen_fonts(&cc.egui_ctx);
        apply_zen_visuals(&cc.egui_ctx, theme_mode);
        let brand_logo = load_unlagged_brand_texture(&cc.egui_ctx);
        let is_admin = if export_screenshot {
            true
        } else {
            is_running_as_admin()
        };

        Self {
            theme_mode,
            brand_logo,
            is_admin,
            install_dir: default_install_dir(true),
            create_desktop_shortcut: true,
            create_start_menu_shortcut: true,
            install_completed: false,
            installed_exe_path: None,
            status_message: None,
            install_logs: Vec::new(),
            export_screenshot_step: if export_screenshot { Some(0) } else { None },
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

    /// Draws a window control button (`Minimize` or `Close`) at an exact `Rect`.
    fn draw_window_control_at(&self, ui: &mut Ui, rect: Rect, icon: LucideIcon, is_close: bool) -> bool {
        let p = self.theme_mode.palette();
        let resp = ui.allocate_rect(rect, Sense::click());
        if resp.hovered() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }
        let (bg, fg) = if resp.hovered() {
            if is_close {
                (Color32::from_rgb(0xE1, 0x1D, 0x48), Color32::WHITE)
            } else {
                (p.hover_bg, p.text_heading)
            }
        } else {
            (Color32::TRANSPARENT, p.text_secondary)
        };
        if bg != Color32::TRANSPARENT {
            ui.painter().rect_filled(rect, Rounding::same(6.0), bg);
        }
        let icon_rect = Rect::from_center_size(rect.center(), Vec2::splat(13.5));
        paint_lucide_icon(ui.painter(), icon_rect, icon, fg);
        resp.clicked()
    }

    /// Draws a secondary pill button at an exact `Rect`, optically centering the text on `rect.center()`.
    fn draw_pill_button_at(&self, ui: &mut Ui, rect: Rect, label: &str) -> bool {
        let p = self.theme_mode.palette();
        let resp = ui.allocate_rect(rect, Sense::click());
        if resp.hovered() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }
        let (bg, fg, stroke) = if resp.hovered() {
            (p.hover_bg, p.text_heading, Stroke::new(1.0_f32, p.border_selected))
        } else {
            (p.subtle_glass_panel, p.text_secondary, Stroke::new(1.0_f32, p.border_subtle))
        };
        ui.painter().rect(rect, Rounding::same(8.0), bg, stroke);
        let galley = ui
            .painter()
            .layout_no_wrap(label.to_string(), FontId::new(12.5, FontFamily::Proportional), fg);
        ui.painter().galley(
            pos2(
                rect.center().x - galley.size().x * 0.5,
                rect.center().y - galley.size().y * 0.5 - 1.5,
            ),
            galley,
            fg,
        );
        resp.clicked()
    }

    /// Draws a primary CTA button at an exact `Rect`, optically centering both the Lucide icon and label on `rect.center().y`.
    fn draw_cta_button_at(&self, ui: &mut Ui, right_x: f32, cy: f32, icon: LucideIcon, label: &str) -> (Rect, bool) {
        let p = self.theme_mode.palette();
        let font_id = FontId::new(13.0, FontFamily::Proportional);
        let galley = ui
            .painter()
            .layout_no_wrap(label.to_string(), font_id.clone(), p.cta_text);
        let width = galley.size().x + 22.0 + 28.0;
        let height = 36.0;
        let rect = Rect::from_center_size(pos2(right_x - width * 0.5, cy), Vec2::new(width, height));
        let resp = ui.allocate_rect(rect, Sense::click());
        if resp.hovered() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }
        let fill = if resp.hovered() { p.cta_hover } else { p.cta_fill };
        ui.painter().rect_filled(rect, Rounding::same(9.0), fill);

        let icon_x = rect.left() + 14.0;
        let ic_rect = Rect::from_center_size(pos2(icon_x + 8.0, cy), Vec2::splat(15.5));
        paint_lucide_icon(ui.painter(), ic_rect, icon, p.cta_text);

        let text_x = icon_x + 22.0;
        ui.painter().galley(
            pos2(text_x, cy - galley.size().y * 0.5 - 1.5),
            galley,
            p.cta_text,
        );
        (rect, resp.clicked())
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

                let drag_rect = Rect::from_min_max(
                    full_rect.min,
                    pos2(
                        (full_rect.right() - 80.0).max(full_rect.left()),
                        full_rect.bottom(),
                    ),
                );
                let drag_resp = ui.interact(
                    drag_rect,
                    ui.id().with("installer_title_drag"),
                    Sense::click_and_drag(),
                );
                if drag_resp.drag_started_by(egui::PointerButton::Primary) {
                    ctx.send_viewport_cmd(egui::ViewportCommand::StartDrag);
                }

                // Centered title group with ADMIN badge
                {
                    let painter = ui.painter();
                    let title_galley = painter.layout_no_wrap(
                        "LEAGUE UNLAGGED SETUP".to_string(),
                        FontId::new(11.5, FontFamily::Proportional),
                        p.text_heading,
                    );
                    let sub_galley = painter.layout_no_wrap(
                        "|  Windows Installer v1.0.0".to_string(),
                        FontId::new(11.0, FontFamily::Proportional),
                        p.text_muted,
                    );
                    let badge_label = if self.is_admin { "ADMIN" } else { "USER" };
                    let (badge_fg, badge_bg) = if self.is_admin {
                        (p.emerald_optimal, p.emerald_bg)
                    } else {
                        (p.text_muted, p.subtle_glass_panel)
                    };
                    let badge_galley = painter.layout_no_wrap(
                        badge_label.to_string(),
                        FontId::new(10.0, FontFamily::Proportional),
                        badge_fg,
                    );

                    let badge_w = badge_galley.size().x + 12.0;
                    let total_w =
                        24.0 + title_galley.size().x + 8.0 + sub_galley.size().x + 8.0 + badge_w;
                    let mut cx =
                        (full_rect.center().x - total_w * 0.5).max(full_rect.left() + 8.0);

                    if let Some(tex) = &self.brand_logo {
                        let r = Rect::from_center_size(pos2(cx + 9.0, cy), Vec2::splat(18.0));
                        painter.image(
                            tex.id(),
                            r,
                            Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
                            Color32::WHITE,
                        );
                        cx += 24.0;
                    }
                    painter.galley(
                        pos2(cx, cy - title_galley.size().y * 0.5 - 1.0),
                        title_galley.clone(),
                        p.text_heading,
                    );
                    cx += title_galley.size().x + 8.0;
                    painter.galley(
                        pos2(cx, cy - sub_galley.size().y * 0.5 - 1.0),
                        sub_galley.clone(),
                        p.text_muted,
                    );
                    cx += sub_galley.size().x + 8.0;

                    let badge_rect = Rect::from_center_size(
                        pos2(cx + badge_w * 0.5, cy),
                        Vec2::new(badge_w, 18.0),
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
                            cy - badge_galley.size().y * 0.5 - 1.0,
                        ),
                        badge_galley,
                        badge_fg,
                    );
                }

                // Right-aligned window controls placed directly at cy with 4px gap
                let close_rect = Rect::from_center_size(
                    pos2(full_rect.right() - 16.0, cy),
                    Vec2::new(32.0, 24.0),
                );
                let min_rect = Rect::from_center_size(
                    pos2(full_rect.right() - 52.0, cy),
                    Vec2::new(32.0, 24.0),
                );
                if self.draw_window_control_at(ui, close_rect, LucideIcon::Close, true) {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
                if self.draw_window_control_at(ui, min_rect, LucideIcon::Minus, false) {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(true));
                }
            });
    }

    fn render_footer_bar(&mut self, ctx: &Context) {
        let p = self.theme_mode.palette();
        egui::TopBottomPanel::bottom("installer_footer_bar")
            .exact_height(54.0)
            .frame(
                Frame::none()
                    .fill(p.main_glass_panel)
                    .stroke(Stroke::new(1.0_f32, p.border_subtle))
                    .inner_margin(Margin::symmetric(24.0, 0.0)),
            )
            .show(ctx, |ui| {
                let full_rect = ui.max_rect();
                let cy = full_rect.center().y;

                // Left status dot & status text centered at cy
                {
                    let painter = ui.painter();
                    let (dot_color, status_str) = if let Some((ok, msg)) = &self.status_message {
                        (
                            if *ok { p.emerald_optimal } else { p.gold_priority },
                            msg.as_str(),
                        )
                    } else if self.is_admin {
                        (
                            p.emerald_optimal,
                            "Status: Ready  |  Administrator Elevation Active",
                        )
                    } else {
                        (p.gold_priority, "Status: Standard User Mode")
                    };

                    let dot_center = pos2(full_rect.left() + 6.0, cy);
                    painter.circle_filled(dot_center, 6.8, dot_color.gamma_multiply(0.18));
                    painter.circle_filled(dot_center, 4.0, dot_color);

                    let galley = painter.layout_no_wrap(
                        status_str.to_string(),
                        FontId::new(12.0, FontFamily::Proportional),
                        p.text_secondary,
                    );
                    painter.galley(
                        pos2(full_rect.left() + 18.0, cy - galley.size().y * 0.5 - 1.5),
                        galley,
                        p.text_secondary,
                    );
                }

                // Right-aligned primary CTA action buttons placed directly at cy
                if !self.install_completed {
                    let (cta_rect, cta_clicked) = self.draw_cta_button_at(
                        ui,
                        full_rect.right(),
                        cy,
                        LucideIcon::Shield,
                        "Install League Unlagged v1.0.0",
                    );
                    if cta_clicked {
                        let target_dir = PathBuf::from(self.install_dir.trim());
                        match perform_installation(
                            &target_dir,
                            self.create_desktop_shortcut,
                            self.create_start_menu_shortcut,
                        ) {
                            Ok((exe_path, logs)) => {
                                self.install_logs = logs;
                                self.installed_exe_path = Some(exe_path);
                                self.install_completed = true;
                                self.status_message = Some((
                                    true,
                                    "Status: Installed v1.0.0 — Ready to Launch"
                                        .to_string(),
                                ));
                            }
                            Err(err) => {
                                self.status_message = Some((false, err));
                            }
                        }
                    }

                    if !self.is_admin {
                        let admin_rect = Rect::from_center_size(
                            pos2(cta_rect.left() - 58.0, cy),
                            Vec2::new(100.0, 30.0),
                        );
                        if self.draw_pill_button_at(ui, admin_rect, "Run as Admin") {
                            if relaunch_self_as_admin().is_ok() {
                                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                            }
                        }
                    }
                } else {
                    let (cta_rect, launch_clicked) = self.draw_cta_button_at(
                        ui,
                        full_rect.right(),
                        cy,
                        LucideIcon::Zap,
                        "Launch League Unlagged",
                    );
                    if launch_clicked {
                        if let Some(exe) = &self.installed_exe_path {
                            let _ = launch_installed_app_as_admin(exe);
                        }
                        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    }

                    let close_rect = Rect::from_center_size(
                        pos2(cta_rect.left() - 46.0, cy),
                        Vec2::new(76.0, 30.0),
                    );
                    if self.draw_pill_button_at(ui, close_rect, "Close") {
                        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                }
            });
    }

    /// Renders a single toggle row where both the 40x22 switch and the text label sit on the exact vertical centerline `cy`.
    fn render_centered_toggle_row(
        &self,
        ui: &mut Ui,
        value: &mut bool,
        label_text: &str,
        bold: bool,
    ) {
        let p = self.theme_mode.palette();
        let avail_w = ui.available_width();
        let (row_rect, row_resp) =
            ui.allocate_exact_size(Vec2::new(avail_w, 28.0), Sense::click());
        if row_resp.clicked() {
            *value = !*value;
        }
        if row_resp.hovered() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }

        let cy = row_rect.center().y;
        let painter = ui.painter();

        // Draw 40x22 toggle switch centered on cy
        let switch_rect = Rect::from_center_size(
            pos2(row_rect.left() + 20.0, cy),
            Vec2::new(40.0, 22.0),
        );
        let radius = 11.0;
        let bg_fill = if *value {
            p.accent_primary
        } else if row_resp.hovered() {
            p.hover_bg
        } else {
            p.subtle_glass_panel
        };
        let border_stroke = if *value {
            Stroke::new(1.0_f32, p.accent_primary)
        } else {
            Stroke::new(1.0_f32, p.border_selected)
        };
        painter.rect(switch_rect, Rounding::same(radius), bg_fill, border_stroke);

        let how_on = ui.ctx().animate_bool(row_resp.id.with(label_text), *value);
        let circle_x = egui::lerp(
            (switch_rect.left() + radius)..=(switch_rect.right() - radius),
            how_on,
        );
        let thumb_color = if *value {
            p.accent_contrast_text
        } else {
            p.text_secondary
        };
        painter.circle_filled(pos2(circle_x, cy), radius - 3.5, thumb_color);

        // Draw label text optically centered on cy (-2.2px compensates for Ubuntu-Light ascender padding)
        let font_id = FontId::new(if bold { 13.0 } else { 12.5 }, FontFamily::Proportional);
        let text_col = if bold { p.text_heading } else { p.text_body };
        let galley = painter.layout_no_wrap(label_text.to_string(), font_id, text_col);
        painter.galley(
            pos2(row_rect.left() + 52.0, cy - galley.size().y * 0.5 - 2.2),
            galley,
            text_col,
        );
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

    fn save_color_image_png(image: &egui::ColorImage, path: &Path) {
        let width = image.size[0] as u32;
        let height = image.size[1] as u32;
        let mut raw_scanlines = Vec::with_capacity((width as usize * 4 + 1) * height as usize);
        for y in 0..(height as usize) {
            raw_scanlines.push(0u8);
            let row_start = y * (width as usize);
            for x in 0..(width as usize) {
                let c = image.pixels[row_start + x];
                raw_scanlines.extend_from_slice(&[c.r(), c.g(), c.b(), c.a()]);
            }
        }
        let mut zlib =
            Vec::with_capacity(raw_scanlines.len() + (raw_scanlines.len() / 65535 + 2) * 5 + 6);
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
        let _ = std::fs::write(path, png);
    }

    fn handle_screenshot_export(&mut self, ctx: &Context) {
        let Some(step) = self.export_screenshot_step else {
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
        if let Some(img) = captured_img {
            Self::save_color_image_png(&img, Path::new("readme-assets/installer.png"));
            self.export_screenshot_step = None;
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            return;
        }
        if step == 4 {
            ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot);
            self.export_screenshot_step = Some(10);
        } else if step < 4 {
            self.export_screenshot_step = Some(step + 1);
        }
        ctx.request_repaint();
    }
}

impl eframe::App for InstallerApp {
    fn update(&mut self, ctx: &Context, _frame: &mut eframe::Frame) {
        let p = self.theme_mode.palette();
        self.render_titlebar(ctx);
        self.render_footer_bar(ctx);

        egui::CentralPanel::default()
            .frame(
                Frame::none()
                    .fill(p.app_bg)
                    .inner_margin(Margin::symmetric(24.0, 18.0)),
            )
            .show(ctx, |ui| {
                // Brand Header Row (44px exact height, vertically centered logo + title stack)
                {
                    let avail_w = ui.available_width();
                    let (header_rect, _) =
                        ui.allocate_exact_size(Vec2::new(avail_w, 44.0), Sense::hover());
                    let cy = header_rect.center().y;
                    let painter = ui.painter();
                    let mut left_x = header_rect.left();

                    if let Some(tex) = &self.brand_logo {
                        let logo_rect =
                            Rect::from_center_size(pos2(left_x + 20.0, cy), Vec2::splat(40.0));
                        painter.image(
                            tex.id(),
                            logo_rect,
                            Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
                            Color32::WHITE,
                        );
                        left_x += 52.0;
                    }

                    let title_galley = painter.layout_no_wrap(
                        "League Unlagged v1.0.0 — Official Setup".to_string(),
                        FontId::new(17.5, FontFamily::Proportional),
                        p.text_heading,
                    );
                    let sub_galley = painter.layout_no_wrap(
                        "Hardware-Adaptive Input Latency, Core Isolation & Interrupt Utility"
                            .to_string(),
                        FontId::new(12.0, FontFamily::Proportional),
                        p.text_secondary,
                    );
                    let stack_h = title_galley.size().y + 3.0 + sub_galley.size().y;
                    let top_y = cy - stack_h * 0.5 - 1.0;
                    painter.galley(pos2(left_x, top_y), title_galley.clone(), p.text_heading);
                    painter.galley(
                        pos2(left_x, top_y + title_galley.size().y + 3.0),
                        sub_galley,
                        p.text_secondary,
                    );
                }

                ui.add_space(14.0);

                if !self.install_completed {
                    // Section 1: Target Installation Directory
                    zen_card_frame(self.theme_mode, false).show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.label(
                            RichText::new("1. Target Installation Directory")
                                .size(13.0)
                                .strong()
                                .color(p.text_heading),
                        );
                        ui.add_space(10.0);

                        // Fixed 32px height row where input field and Browse... button share exact cy
                        let row_w = ui.available_width();
                        let (row_rect, _) =
                            ui.allocate_exact_size(Vec2::new(row_w, 32.0), Sense::hover());
                        let cy = row_rect.center().y;

                        let btn_w = 88.0;
                        let btn_rect = Rect::from_center_size(
                            pos2(row_rect.right() - btn_w * 0.5, cy),
                            Vec2::new(btn_w, 30.0),
                        );
                        if self.draw_pill_button_at(ui, btn_rect, "Browse...") {
                            if let Some(picked) = Self::pick_folder_dialog(&self.install_dir) {
                                self.install_dir = picked;
                            }
                        }

                        let input_right = (btn_rect.left() - 10.0).max(row_rect.left() + 120.0);
                        let input_rect = Rect::from_min_max(
                            pos2(row_rect.left(), cy - 15.0),
                            pos2(input_right, cy + 15.0),
                        );
                        ui.painter().rect(
                            input_rect,
                            Rounding::same(8.0),
                            p.subtle_glass_panel,
                            Stroke::new(1.0_f32, p.border_subtle),
                        );

                        let path_galley = ui.painter().layout_no_wrap(
                            self.install_dir.clone(),
                            FontId::new(12.5, FontFamily::Monospace),
                            p.text_heading,
                        );
                        ui.painter().galley(
                            pos2(
                                input_rect.left() + 12.0,
                                cy - path_galley.size().y * 0.5 - 1.0,
                            ),
                            path_galley,
                            p.text_heading,
                        );
                    });

                    ui.add_space(12.0);

                    // Section 2: Shortcuts & Administrator Elevation
                    zen_card_frame(self.theme_mode, false).show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.label(
                            RichText::new("2. Shortcuts & Administrator Elevation")
                                .size(13.0)
                                .strong()
                                .color(p.text_heading),
                        );
                        ui.add_space(8.0);

                        let mut desktop = self.create_desktop_shortcut;
                        self.render_centered_toggle_row(
                            ui,
                            &mut desktop,
                            "Create Desktop Shortcut (Run as Administrator enabled)",
                            true,
                        );
                        self.create_desktop_shortcut = desktop;

                        ui.add_space(6.0);
                        let mut start_menu = self.create_start_menu_shortcut;
                        self.render_centered_toggle_row(
                            ui,
                            &mut start_menu,
                            "Create Start Menu Shortcut (Run as Administrator enabled)",
                            false,
                        );
                        self.create_start_menu_shortcut = start_menu;
                    });
                } else {
                    zen_card_frame(self.theme_mode, true).show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.horizontal(|ui| {
                            draw_lucide_icon(ui, LucideIcon::Check, 18.0, p.emerald_optimal);
                            ui.label(
                                RichText::new("Installation Completed Successfully")
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
                        ui.add_space(12.0);
                        ui.label(
                            RichText::new(
                                "Click 'Launch League Unlagged' in the bottom bar to open the utility as Administrator.",
                            )
                            .size(12.0)
                            .color(p.text_body),
                        );
                    });
                }
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

        self.handle_screenshot_export(ctx);
    }
}

fn main() -> eframe::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let is_admin = is_running_as_admin();
    let export_screenshot = args.iter().any(|a| a == "--export-screenshot");

    if args.iter().any(|a| a == "--silent") {
        let dir = PathBuf::from(default_install_dir(is_admin));
        if let Err(e) = perform_installation(&dir, true, true) {
            eprintln!("Silent installation failed: {e}");
            std::process::exit(1);
        }
        return Ok(());
    }

    if !is_admin && !export_screenshot && !args.iter().any(|a| a == "--elevated") {
        if relaunch_self_as_admin().is_ok() {
            return Ok(());
        }
    }

    if !export_screenshot {
        let mutex_name: Vec<u16> = "Local\\LeagueUnlagged_Installer_SingleInstance_v1\0"
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
            return Ok(());
        }
    }

    let mut viewport = egui::ViewportBuilder::default()
        .with_title("League Unlagged v1.0.0 — Windows Setup")
        .with_decorations(false)
        .with_inner_size([620.0, 434.0])
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
        Box::new(move |cc| Ok(Box::new(InstallerApp::new(cc, export_screenshot)))),
    )
}
