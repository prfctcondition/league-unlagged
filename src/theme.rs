//! Luxury Esports & Raycast/Linear-inspired Design System built on OLED Obsidian depth.
//! Eliminates "box-in-a-box" visual noise: uses flat layered cards, generous 22px breathing room,
//! subtle 1px `rgba(255,255,255,0.07)` borders, borderless underline tabs, and semantic status accents.

use crate::icons::{draw_lucide_icon, paint_lucide_icon, LucideIcon};
use egui::{
    pos2, Color32, Context, FontId, Frame, Margin, Pos2, Rect, Response, RichText, Rounding, Sense,
    Stroke, Ui, Vec2, Visuals,
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ZenThemeMode {
    Dark,
    Light,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BadgeTone {
    Optimal,  // Emerald / Mint glow for Optimal states
    Priority, // Gold / Amber for Dedicated Game Core (LoL Priority)
    Neutral,  // Muted subtle surface for Default / Inactive
    Info,     // Crisp White / Obsidian for standard active badges
}

#[derive(Debug, Clone, Copy)]
pub struct ZenPalette {
    pub app_bg: Color32,
    pub main_glass_panel: Color32,
    pub subtle_glass_panel: Color32,
    pub elevated_modal: Color32,
    pub border_subtle: Color32,
    pub border_selected: Color32,
    pub card_selected_bg: Color32,
    pub hover_bg: Color32,
    pub accent_primary: Color32,
    pub accent_contrast_text: Color32,
    // Luxury Semantic Accents (for CTA button, LoL Priority Core, Optimal status dots)
    pub cta_fill: Color32,
    pub cta_hover: Color32,
    pub cta_text: Color32,
    pub emerald_optimal: Color32,
    pub emerald_bg: Color32,
    pub gold_priority: Color32,
    pub gold_bg: Color32,
    pub cyan_client: Color32,
    pub cyan_bg: Color32,
    // Typography
    pub text_heading: Color32,
    pub text_body: Color32,
    pub text_secondary: Color32,
    pub text_muted: Color32,
}

impl ZenThemeMode {
    pub fn palette(self) -> ZenPalette {
        match self {
            ZenThemeMode::Dark => ZenPalette {
                // OLED Pure Black (#000000)
                app_bg: Color32::from_rgb(0x00, 0x00, 0x00),
                // Main Glass Panel: #08080C (Flat luxury dark surface)
                main_glass_panel: Color32::from_rgb(0x08, 0x08, 0x0C),
                // Subtle Glass Panel: #0D0D12
                subtle_glass_panel: Color32::from_rgb(0x0D, 0x0D, 0x12),
                // Elevated / Drawer: #111118
                elevated_modal: Color32::from_rgb(0x11, 0x11, 0x18),
                // Ultra-soft border: rgba(255, 255, 255, 0.07) -> 18 alpha
                border_subtle: Color32::from_rgba_unmultiplied(0xFF, 0xFF, 0xFF, 18),
                // Highlighted border: rgba(255, 255, 255, 0.22) -> 56 alpha
                border_selected: Color32::from_rgba_unmultiplied(0xFF, 0xFF, 0xFF, 56),
                // Active card background: #0C0C12
                card_selected_bg: Color32::from_rgb(0x0C, 0x0C, 0x12),
                // Hover surface
                hover_bg: Color32::from_rgb(0x16, 0x16, 0x1F),
                // Primary Monochrome Accent: Pure White (#FFFFFF)
                accent_primary: Color32::from_rgb(0xFF, 0xFF, 0xFF),
                accent_contrast_text: Color32::from_rgb(0x00, 0x00, 0x00),
                // Hero CTA Accent (Electric Cyan-Mint)
                cta_fill: Color32::from_rgb(0x10, 0xB9, 0x81),
                cta_hover: Color32::from_rgb(0x34, 0xD3, 0x99),
                cta_text: Color32::from_rgb(0x02, 0x10, 0x0B),
                // Semantic Accents for Infographic & Statuses
                emerald_optimal: Color32::from_rgb(0x34, 0xD3, 0x99),
                emerald_bg: Color32::from_rgba_unmultiplied(0x10, 0xB9, 0x81, 28),
                gold_priority: Color32::from_rgb(0xFB, 0xBF, 0x24),
                gold_bg: Color32::from_rgba_unmultiplied(0xF5, 0x9E, 0x0B, 32),
                cyan_client: Color32::from_rgb(0x38, 0xBD, 0xF8),
                cyan_bg: Color32::from_rgba_unmultiplied(0x0E, 0xA5, 0xE9, 24),
                // Typography
                text_heading: Color32::from_rgb(0xFF, 0xFF, 0xFF),
                text_body: Color32::from_rgb(0xE4, 0xE4, 0xE7),
                text_secondary: Color32::from_rgba_unmultiplied(0xFF, 0xFF, 0xFF, 155),
                text_muted: Color32::from_rgba_unmultiplied(0xFF, 0xFF, 0xFF, 95),
            },
            ZenThemeMode::Light => ZenPalette {
                app_bg: Color32::from_rgb(0xF8, 0xFA, 0xFC),
                main_glass_panel: Color32::from_rgb(0xFF, 0xFF, 0xFF),
                subtle_glass_panel: Color32::from_rgb(0xF1, 0xF5, 0xF9),
                elevated_modal: Color32::from_rgb(0xFF, 0xFF, 0xFF),
                border_subtle: Color32::from_rgba_unmultiplied(0x1B, 0x2C, 0x54, 24),
                border_selected: Color32::from_rgba_unmultiplied(0x1B, 0x2C, 0x54, 125),
                card_selected_bg: Color32::from_rgb(0xFF, 0xFF, 0xFF),
                hover_bg: Color32::from_rgb(0xE2, 0xE8, 0xF0),
                accent_primary: Color32::from_rgb(0x1B, 0x2C, 0x54),
                accent_contrast_text: Color32::from_rgb(0xFF, 0xFF, 0xFF),
                cta_fill: Color32::from_rgb(0x1B, 0x2C, 0x54),
                cta_hover: Color32::from_rgb(0x25, 0x3B, 0x6E),
                cta_text: Color32::from_rgb(0xFF, 0xFF, 0xFF),
                emerald_optimal: Color32::from_rgb(0x05, 0x96, 0x69),
                emerald_bg: Color32::from_rgba_unmultiplied(0x05, 0x96, 0x69, 24),
                gold_priority: Color32::from_rgb(0x1B, 0x2C, 0x54),
                gold_bg: Color32::from_rgba_unmultiplied(0x1B, 0x2C, 0x54, 22),
                cyan_client: Color32::from_rgb(0x02, 0x84, 0xC7),
                cyan_bg: Color32::from_rgba_unmultiplied(0x02, 0x84, 0xC7, 22),
                text_heading: Color32::from_rgb(0x1B, 0x2C, 0x54),
                text_body: Color32::from_rgb(0x1E, 0x29, 0x3B),
                text_secondary: Color32::from_rgb(0x47, 0x55, 0x69),
                text_muted: Color32::from_rgb(0x64, 0x74, 0x8B),
            },
        }
    }
}

pub fn configure_zen_fonts(ctx: &Context) {
    let mut fonts = egui::FontDefinitions::default();

    if let Ok(segoe_bytes) = std::fs::read(r"C:\Windows\Fonts\segoeui.ttf") {
        fonts.font_data.insert(
            "segoe_ui".to_owned(),
            egui::FontData::from_owned(segoe_bytes).into(),
        );
        if let Some(prop) = fonts.families.get_mut(&egui::FontFamily::Proportional) {
            prop.insert(0, "segoe_ui".to_owned());
        }
    }

    if let Ok(consola_bytes) = std::fs::read(r"C:\Windows\Fonts\consola.ttf") {
        fonts.font_data.insert(
            "consolas".to_owned(),
            egui::FontData::from_owned(consola_bytes).into(),
        );
        if let Some(mono) = fonts.families.get_mut(&egui::FontFamily::Monospace) {
            mono.insert(0, "consolas".to_owned());
        }
    }

    ctx.set_fonts(fonts);
}

/// Vector glowing LED status indicator dot — never renders as a missing glyph box.
pub fn zen_status_dot(ui: &mut Ui, color: Color32, radius: f32) {
    let size = Vec2::splat(radius * 2.0 + 6.0);
    let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
    if ui.is_rect_visible(rect) {
        ui.painter()
            .circle_filled(rect.center(), radius + 2.5, color.gamma_multiply(0.22));
        ui.painter().circle_filled(rect.center(), radius, color);
    }
}

/// Vector rounded swatch for infographic legends.
pub fn zen_legend_swatch(ui: &mut Ui, color: Color32) {
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(10.0), Sense::hover());
    if ui.is_rect_visible(rect) {
        ui.painter()
            .rect_filled(rect, Rounding::same(3.0), color);
    }
}

pub fn apply_zen_visuals(ctx: &Context, mode: ZenThemeMode) {
    let p = mode.palette();
    let mut visuals = match mode {
        ZenThemeMode::Dark => Visuals::dark(),
        ZenThemeMode::Light => Visuals::light(),
    };

    visuals.panel_fill = p.app_bg;
    visuals.window_fill = p.elevated_modal;
    visuals.faint_bg_color = p.subtle_glass_panel;
    visuals.extreme_bg_color = p.app_bg;
    visuals.code_bg_color = p.subtle_glass_panel;
    visuals.override_text_color = None;

    visuals.selection.bg_fill = p.accent_primary;
    visuals.selection.stroke = Stroke::new(1.0_f32, p.accent_contrast_text);

    visuals.widgets.noninteractive.bg_fill = p.main_glass_panel;
    visuals.widgets.noninteractive.weak_bg_fill = p.subtle_glass_panel;
    visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0_f32, p.border_subtle);
    visuals.widgets.noninteractive.fg_stroke = Stroke::new(1.0_f32, p.text_body);
    visuals.widgets.noninteractive.rounding = Rounding::same(12.0);

    visuals.widgets.inactive.bg_fill = p.subtle_glass_panel;
    visuals.widgets.inactive.weak_bg_fill = p.subtle_glass_panel;
    visuals.widgets.inactive.bg_stroke = Stroke::new(0.0_f32, Color32::TRANSPARENT);
    visuals.widgets.inactive.fg_stroke = Stroke::new(1.0_f32, p.text_body);
    visuals.widgets.inactive.rounding = Rounding::same(8.0);

    visuals.widgets.hovered.bg_fill = p.hover_bg;
    visuals.widgets.hovered.weak_bg_fill = p.hover_bg;
    visuals.widgets.hovered.bg_stroke = Stroke::new(1.0_f32, p.border_selected);
    visuals.widgets.hovered.fg_stroke = Stroke::new(1.0_f32, p.text_heading);
    visuals.widgets.hovered.rounding = Rounding::same(8.0);

    visuals.widgets.active.bg_fill = p.accent_primary;
    visuals.widgets.active.weak_bg_fill = p.accent_primary;
    visuals.widgets.active.bg_stroke = Stroke::new(1.0_f32, p.accent_primary);
    visuals.widgets.active.fg_stroke = Stroke::new(1.0_f32, p.accent_contrast_text);
    visuals.widgets.active.rounding = Rounding::same(8.0);

    ctx.set_visuals(visuals);
}

/// Primary flat glass card with generous 22px breathing room and ultra-subtle 1px border (`rgba(255,255,255,0.07)`).
pub fn zen_card_frame(mode: ZenThemeMode, is_active: bool) -> Frame {
    let p = mode.palette();
    Frame::none()
        .fill(if is_active {
            p.card_selected_bg
        } else {
            p.main_glass_panel
        })
        .stroke(Stroke::new(
            1.0_f32,
            if is_active {
                p.border_selected
            } else {
                p.border_subtle
            },
        ))
        .rounding(Rounding::same(14.0))
        .inner_margin(Margin::symmetric(22.0, 18.0))
}

/// Borderless inner surface — NO border stroke so we never get "box inside a box"!
pub fn zen_subtle_frame(mode: ZenThemeMode) -> Frame {
    let p = mode.palette();
    Frame::none()
        .fill(p.subtle_glass_panel)
        .rounding(Rounding::same(10.0))
        .inner_margin(Margin::symmetric(16.0, 12.0))
}

/// Borderless Raycast / Linear style Tab Bar item with an active underline indicator.
pub fn zen_underline_tab(ui: &mut Ui, mode: ZenThemeMode, label: &str, active: bool) -> Response {
    let p = mode.palette();
    let font_id = FontId::proportional(13.5);
    let text_color = if active { p.text_heading } else { p.text_muted };

    let galley = ui
        .painter()
        .layout_no_wrap(label.to_string(), font_id, text_color);
    let desired_size = Vec2::new(galley.size().x + 24.0, 36.0);
    let (rect, response) = ui.allocate_exact_size(desired_size, Sense::click());

    if ui.is_rect_visible(rect) {
        if response.hovered() && !active {
            ui.painter()
                .rect_filled(rect.shrink2(Vec2::new(2.0, 4.0)), Rounding::same(6.0), p.hover_bg);
        }
        let fg = if active {
            p.text_heading
        } else if response.hovered() {
            p.text_body
        } else {
            p.text_muted
        };
        let galley = ui
            .painter()
            .layout_no_wrap(label.to_string(), FontId::proportional(13.5), fg);
        let text_pos = Pos2::new(
            rect.center().x - galley.size().x * 0.5,
            rect.center().y - galley.size().y * 0.5 - 2.0,
        );
        ui.painter().galley(text_pos, galley, fg);

        if active {
            // Glowing bottom indicator line
            let line_rect = Rect::from_min_max(
                pos2(rect.left() + 8.0, rect.bottom() - 2.5),
                pos2(rect.right() - 8.0, rect.bottom()),
            );
            ui.painter()
                .rect_filled(line_rect, Rounding::same(2.0), p.accent_primary);
        }
    }
    response
}

/// Prominent Hero CTA Button with a monochrome linear Lucide icon (16px, `currentColor`) and vibrant emerald/cyan fill.
pub fn zen_cta_button(
    ui: &mut Ui,
    mode: ZenThemeMode,
    icon: Option<LucideIcon>,
    label: &str,
) -> Response {
    let p = mode.palette();
    let font_id = FontId::proportional(13.5);
    let galley = ui
        .painter()
        .layout_no_wrap(label.to_string(), font_id.clone(), p.cta_text);
    let icon_space = if icon.is_some() { 22.0 } else { 0.0 };
    let desired_size = Vec2::new(galley.size().x + icon_space + 28.0, 36.0);
    let (rect, response) = ui.allocate_exact_size(desired_size, Sense::click());

    if ui.is_rect_visible(rect) {
        let fill = if response.hovered() {
            p.cta_hover
        } else {
            p.cta_fill
        };
        ui.painter()
            .rect_filled(rect, Rounding::same(10.0), fill);

        let mut x = rect.left() + 14.0;
        if let Some(ic) = icon {
            let ic_rect = Rect::from_center_size(pos2(x + 8.0, rect.center().y), Vec2::splat(16.0));
            paint_lucide_icon(ui.painter(), ic_rect, ic, p.cta_text);
            x += 22.0;
        }
        let text_pos = pos2(x, rect.center().y - galley.size().y * 0.5);
        ui.painter().galley(text_pos, galley, p.cta_text);
    }
    response
}

/// Clean secondary action button without icons.
pub fn zen_pill_button(ui: &mut Ui, mode: ZenThemeMode, label: &str, active: bool) -> Response {
    zen_pill_button_with_icons(ui, mode, None, label, None, active)
}

/// Clean secondary action button with optional leading and trailing Lucide vector icons (16px, `currentColor`).
pub fn zen_pill_button_with_icons(
    ui: &mut Ui,
    mode: ZenThemeMode,
    leading_icon: Option<LucideIcon>,
    label: &str,
    trailing_icon: Option<LucideIcon>,
    active: bool,
) -> Response {
    let p = mode.palette();
    let font_id = FontId::proportional(12.5);
    let (_, fg_base) = if active {
        (p.accent_primary, p.accent_contrast_text)
    } else {
        (p.subtle_glass_panel, p.text_secondary)
    };
    let galley = ui
        .painter()
        .layout_no_wrap(label.to_string(), font_id.clone(), fg_base);
    let lead_w = if leading_icon.is_some() { 21.0 } else { 0.0 };
    let trail_w = if trailing_icon.is_some() { 20.0 } else { 0.0 };
    let desired_size = Vec2::new(galley.size().x + lead_w + trail_w + 24.0, 30.0);
    let (rect, response) = ui.allocate_exact_size(desired_size, Sense::click());

    if ui.is_rect_visible(rect) {
        let (bg, fg) = if active {
            (p.accent_primary, p.accent_contrast_text)
        } else if response.hovered() {
            (p.hover_bg, p.text_heading)
        } else {
            (p.subtle_glass_panel, p.text_secondary)
        };
        let stroke = if response.hovered() && !active {
            Stroke::new(1.0_f32, p.border_selected)
        } else {
            Stroke::NONE
        };
        ui.painter()
            .rect(rect, Rounding::same(8.0), bg, stroke);

        let mut x = rect.left() + 12.0;
        if let Some(ic) = leading_icon {
            let ic_rect = Rect::from_center_size(pos2(x + 7.5, rect.center().y), Vec2::splat(15.0));
            paint_lucide_icon(ui.painter(), ic_rect, ic, fg);
            x += 21.0;
        }
        let galley = ui.painter().layout_no_wrap(label.to_string(), font_id, fg);
        let text_pos = pos2(x, rect.center().y - galley.size().y * 0.5);
        let text_w = galley.size().x;
        ui.painter().galley(text_pos, galley, fg);
        x += text_w + 6.0;

        if let Some(ic) = trailing_icon {
            let ic_rect = Rect::from_center_size(pos2(x + 7.0, rect.center().y), Vec2::splat(14.0));
            paint_lucide_icon(ui.painter(), ic_rect, ic, fg);
        }
    }
    response
}

/// Semantic Luxury Badge (`OPTIMAL`, `LOL PRIORITY`, `ISOLATED`, `DEFAULT`).
/// Borderless translucent pill with a crisp colored or monochrome label.
pub fn zen_semantic_badge(ui: &mut Ui, mode: ZenThemeMode, tone: BadgeTone, text: &str) {
    zen_semantic_badge_with_icon(ui, mode, tone, None, text);
}

/// Semantic Luxury Badge with an optional monochrome linear Lucide vector icon (`currentColor`).
pub fn zen_semantic_badge_with_icon(
    ui: &mut Ui,
    mode: ZenThemeMode,
    tone: BadgeTone,
    icon: Option<LucideIcon>,
    text: &str,
) {
    let p = mode.palette();
    let (bg, fg) = match tone {
        BadgeTone::Optimal => (p.emerald_bg, p.emerald_optimal),
        BadgeTone::Priority => (p.gold_bg, p.gold_priority),
        BadgeTone::Info => (p.cyan_bg, p.cyan_client),
        BadgeTone::Neutral => (p.subtle_glass_panel, p.text_muted),
    };

    Frame::none()
        .fill(bg)
        .rounding(Rounding::same(6.0))
        .inner_margin(Margin::symmetric(8.0, 4.0))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 5.0;
                if let Some(ic) = icon {
                    draw_lucide_icon(ui, ic, 12.0, fg);
                }
                ui.label(RichText::new(text).size(10.5).strong().color(fg));
            });
        });
}

/// Legacy status badge wrapper using semantic tones (Optimal when active, Neutral when inactive).
pub fn zen_status_badge(
    ui: &mut Ui,
    mode: ZenThemeMode,
    is_active: bool,
    active_text: &str,
    inactive_text: &str,
) {
    if is_active {
        zen_semantic_badge(ui, mode, BadgeTone::Optimal, active_text);
    } else {
        zen_semantic_badge(ui, mode, BadgeTone::Neutral, inactive_text);
    }
}

/// Smooth Toggle Switch.
pub fn zen_toggle_switch(ui: &mut Ui, mode: ZenThemeMode, on: &mut bool) -> Response {
    let p = mode.palette();
    let desired_size = Vec2::new(42.0, 22.0);
    let (rect, mut response) = ui.allocate_exact_size(desired_size, Sense::click());

    if response.clicked() {
        *on = !*on;
        response.mark_changed();
    }

    if ui.is_rect_visible(rect) {
        let how_on = ui.ctx().animate_bool_responsive(response.id, *on);
        let bg_color = if *on {
            p.accent_primary
        } else if response.hovered() {
            p.hover_bg
        } else {
            p.subtle_glass_panel
        };

        let radius = 0.5 * rect.height();
        ui.painter()
            .rect_filled(rect, Rounding::same(radius), bg_color);

        let circle_x = egui::lerp((rect.left() + radius)..=(rect.right() - radius), how_on);
        let center = egui::pos2(circle_x, rect.center().y);
        let thumb_color = if *on {
            p.accent_contrast_text
        } else {
            p.text_muted
        };
        ui.painter()
            .circle_filled(center, radius - 3.5, thumb_color);
    }

    response
}

/// Custom frameless window control button (Minimize, Maximize/Restore, Close).
pub fn zen_window_control_button(
    ui: &mut Ui,
    mode: ZenThemeMode,
    icon: LucideIcon,
    is_close: bool,
) -> Response {
    let p = mode.palette();
    let desired_size = Vec2::new(36.0, 26.0);
    let (rect, response) = ui.allocate_exact_size(desired_size, Sense::click());

    if ui.is_rect_visible(rect) {
        let (bg, fg) = if response.hovered() {
            if is_close {
                (Color32::from_rgb(0xE1, 0x1D, 0x48), Color32::WHITE)
            } else {
                (p.hover_bg, p.text_heading)
            }
        } else {
            (Color32::TRANSPARENT, p.text_secondary)
        };
        if bg != Color32::TRANSPARENT {
            ui.painter()
                .rect_filled(rect, Rounding::same(6.0), bg);
        }
        let icon_rect = Rect::from_center_size(rect.center(), Vec2::splat(13.5));
        paint_lucide_icon(ui.painter(), icon_rect, icon, fg);
    }
    response
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dark_theme_is_strictly_monochrome_oled() {
        let p = ZenThemeMode::Dark.palette();
        assert_eq!(p.app_bg, Color32::from_rgb(0, 0, 0));
        assert_eq!(p.accent_primary, Color32::from_rgb(255, 255, 255));
        assert_eq!(p.accent_contrast_text, Color32::from_rgb(0, 0, 0));
        assert!(p.main_glass_panel.r() <= 16 && p.main_glass_panel.g() <= 16 && p.main_glass_panel.b() <= 20);
    }

    #[test]
    fn test_light_theme_uses_royal_navy_1b2c54_accent() {
        let p = ZenThemeMode::Light.palette();
        assert_eq!(p.app_bg, Color32::from_rgb(0xF8, 0xFA, 0xFC));
        assert_eq!(p.accent_primary, Color32::from_rgb(0x1B, 0x2C, 0x54));
        assert_eq!(p.gold_priority, Color32::from_rgb(0x1B, 0x2C, 0x54));
        assert_eq!(p.accent_contrast_text, Color32::from_rgb(0xFF, 0xFF, 0xFF));
    }
}
