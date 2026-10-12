//! Monochrome linear vector icons based on Lucide Icons (24x24 grid, 16-18px, `currentColor`)
//! plus SVG rasterizer for the application's custom `unlagged.svg` brand icon.
//! Strictly 0% Unicode emojis.

use egui::{pos2, Color32, ColorImage, Context, Pos2, Rect, Rounding, Sense, Stroke, TextureHandle, TextureOptions, Ui, Vec2};

const UNLAGGED_SVG_BYTES: &[u8] = include_bytes!("../assets/unlagged.svg");

/// Rasterizes `assets/unlagged.svg` into an `egui::IconData` (64x64 RGBA) for the native OS window and taskbar icon.
pub fn load_unlagged_window_icon() -> Option<egui::IconData> {
    let rgba = rasterize_svg_unmultiplied(UNLAGGED_SVG_BYTES, 64, 64)?;
    Some(egui::IconData {
        rgba,
        width: 64,
        height: 64,
    })
}

/// Rasterizes `assets/unlagged.svg` into a crisp high-DPI `egui::TextureHandle` for the in-app header logo.
pub fn load_unlagged_brand_texture(ctx: &Context) -> Option<TextureHandle> {
    let size = 96u32;
    let rgba = rasterize_svg_unmultiplied(UNLAGGED_SVG_BYTES, size, size)?;
    let image = ColorImage::from_rgba_unmultiplied([size as usize, size as usize], &rgba);
    Some(ctx.load_texture("unlagged_brand_logo", image, TextureOptions::LINEAR))
}

/// Generates a standard Windows `.ico` file (256x256 PNG-encoded frame) from `assets/unlagged.svg`.
pub fn build_unlagged_ico_bytes() -> Option<Vec<u8>> {
    let opt = resvg::usvg::Options::default();
    let tree = resvg::usvg::Tree::from_data(UNLAGGED_SVG_BYTES, &opt).ok()?;
    let mut pixmap = resvg::tiny_skia::Pixmap::new(256, 256)?;
    let svg_size = tree.size();
    let transform = resvg::tiny_skia::Transform::from_scale(
        256.0 / svg_size.width(),
        256.0 / svg_size.height(),
    );
    resvg::render(&tree, transform, &mut pixmap.as_mut());
    let png_bytes = pixmap.encode_png().ok()?;

    let mut ico = Vec::with_capacity(22 + png_bytes.len());
    // ICONDIR (6 bytes): Reserved=0, Type=1 (ICO), Count=1
    ico.extend_from_slice(&[0, 0, 1, 0, 1, 0]);
    // ICONDIRENTRY (16 bytes): Width=0 (256), Height=0 (256), ColorCount=0, Reserved=0, Planes=1, BitCount=32
    ico.extend_from_slice(&[0, 0, 0, 0, 1, 0, 32, 0]);
    ico.extend_from_slice(&(png_bytes.len() as u32).to_le_bytes());
    ico.extend_from_slice(&22u32.to_le_bytes());
    ico.extend_from_slice(&png_bytes);
    Some(ico)
}

fn rasterize_svg_unmultiplied(svg_bytes: &[u8], width: u32, height: u32) -> Option<Vec<u8>> {
    let opt = resvg::usvg::Options::default();
    let tree = resvg::usvg::Tree::from_data(svg_bytes, &opt).ok()?;
    let mut pixmap = resvg::tiny_skia::Pixmap::new(width, height)?;

    let svg_size = tree.size();
    let sx = width as f32 / svg_size.width();
    let sy = height as f32 / svg_size.height();
    let transform = resvg::tiny_skia::Transform::from_scale(sx, sy);

    resvg::render(&tree, transform, &mut pixmap.as_mut());

    // Convert premultiplied RGBA from tiny-skia to unmultiplied RGBA expected by egui::IconData & ColorImage
    let mut out = Vec::with_capacity((width * height * 4) as usize);
    for px in pixmap.pixels() {
        let c = px.demultiply();
        out.push(c.red());
        out.push(c.green());
        out.push(c.blue());
        out.push(c.alpha());
    }
    Some(out)
}

/// Monochrome linear vector icons from the Lucide Icons specification (24x24 viewBox).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LucideIcon {
    Zap,
    Cpu,
    Crosshair,
    Usb,
    Shield,
    Sliders,
    Sun,
    Moon,
    ChevronDown,
    ChevronUp,
    Terminal,
    RotateCcw,
    Activity,
    Monitor,
    Award,
    Minus,
    Maximize,
    Restore,
    Close,
    Check,
}

/// Allocates space and draws a monochrome linear Lucide vector icon (`size` = 14..18px) inheriting `color` (`currentColor`).
pub fn draw_lucide_icon(ui: &mut Ui, icon: LucideIcon, size: f32, color: Color32) {
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(size), Sense::hover());
    if ui.is_rect_visible(rect) {
        paint_lucide_icon(ui.painter(), rect, icon, color);
    }
}

/// Paints a Lucide vector icon inside `rect` mapped from the standard `0..24` Lucide coordinate system.
pub fn paint_lucide_icon(painter: &egui::Painter, rect: Rect, icon: LucideIcon, color: Color32) {
    let stroke = Stroke::new((rect.width() * 0.082).clamp(1.35, 1.85), color);
    let map = |x: f32, y: f32| -> Pos2 {
        pos2(
            rect.left() + (x / 24.0) * rect.width(),
            rect.top() + (y / 24.0) * rect.height(),
        )
    };
    let seg = |x1: f32, y1: f32, x2: f32, y2: f32| {
        painter.line_segment([map(x1, y1), map(x2, y2)], stroke);
    };

    match icon {
        LucideIcon::Zap => {
            // Lucide `zap`: polygon 13 2 -> 3 14 -> 12 14 -> 11 22 -> 21 10 -> 12 10 -> 13 2
            let pts = [
                map(13.0, 2.0),
                map(3.0, 14.0),
                map(12.0, 14.0),
                map(11.0, 22.0),
                map(21.0, 10.0),
                map(12.0, 10.0),
                map(13.0, 2.0),
            ];
            for w in pts.windows(2) {
                painter.line_segment([w[0], w[1]], stroke);
            }
        }
        LucideIcon::Cpu => {
            // Lucide `cpu`: outer rect (4,4)..(20,20), inner core (9,9)..(15,15), pins on 4 sides
            let outer = Rect::from_min_max(map(4.0, 4.0), map(20.0, 20.0));
            let inner = Rect::from_min_max(map(9.0, 9.0), map(15.0, 15.0));
            painter.rect_stroke(outer, Rounding::same(2.0), stroke);
            painter.rect_stroke(inner, Rounding::same(1.0), stroke);
            seg(9.0, 1.0, 9.0, 4.0);
            seg(15.0, 1.0, 15.0, 4.0);
            seg(9.0, 20.0, 9.0, 23.0);
            seg(15.0, 20.0, 15.0, 23.0);
            seg(20.0, 9.0, 23.0, 9.0);
            seg(20.0, 14.0, 23.0, 14.0);
            seg(1.0, 9.0, 4.0, 9.0);
            seg(1.0, 14.0, 4.0, 14.0);
        }
        LucideIcon::Crosshair => {
            // Lucide `crosshair`: circle (12,12) r=10 + 4 inward target lines
            let r = (10.0 / 24.0) * rect.width();
            painter.circle_stroke(map(12.0, 12.0), r, stroke);
            seg(22.0, 12.0, 18.0, 12.0);
            seg(6.0, 12.0, 2.0, 12.0);
            seg(12.0, 6.0, 12.0, 2.0);
            seg(12.0, 22.0, 12.0, 18.0);
        }
        LucideIcon::Usb => {
            // Lucide `usb`: central stem + branches + node circles
            seg(10.0, 19.0, 14.0, 5.0);
            seg(7.0, 12.0, 11.5, 13.5);
            seg(12.5, 9.5, 17.0, 11.0);
            painter.circle_stroke(map(10.0, 19.0), (2.2 / 24.0) * rect.width(), stroke);
            painter.circle_stroke(map(7.0, 11.0), (1.7 / 24.0) * rect.width(), stroke);
            painter.circle_stroke(map(17.0, 10.0), (1.7 / 24.0) * rect.width(), stroke);
            painter.circle_filled(map(14.0, 5.0), (1.6 / 24.0) * rect.width(), color);
        }
        LucideIcon::Shield => {
            // Lucide `shield`: crest outline
            let pts = [
                map(12.0, 2.0),
                map(20.0, 6.0),
                map(20.0, 12.5),
                map(16.5, 18.0),
                map(12.0, 22.0),
                map(7.5, 18.0),
                map(4.0, 12.5),
                map(4.0, 6.0),
                map(12.0, 2.0),
            ];
            for w in pts.windows(2) {
                painter.line_segment([w[0], w[1]], stroke);
            }
        }
        LucideIcon::Sliders => {
            // Lucide `sliders-horizontal`: 3 horizontal slider tracks with vertical knobs
            seg(21.0, 4.0, 14.0, 4.0);
            seg(10.0, 4.0, 3.0, 4.0);
            seg(14.0, 2.0, 14.0, 6.0);

            seg(21.0, 12.0, 12.0, 12.0);
            seg(8.0, 12.0, 3.0, 12.0);
            seg(8.0, 10.0, 8.0, 14.0);

            seg(21.0, 20.0, 16.0, 20.0);
            seg(12.0, 20.0, 3.0, 20.0);
            seg(16.0, 18.0, 16.0, 22.0);
        }
        LucideIcon::Sun => {
            // Lucide `sun`: center circle r=4 + 8 rays
            let r = (4.0 / 24.0) * rect.width();
            painter.circle_stroke(map(12.0, 12.0), r, stroke);
            seg(12.0, 2.0, 12.0, 4.0);
            seg(12.0, 20.0, 12.0, 22.0);
            seg(4.93, 4.93, 6.34, 6.34);
            seg(17.66, 17.66, 19.07, 19.07);
            seg(2.0, 12.0, 4.0, 12.0);
            seg(20.0, 12.0, 22.0, 12.0);
            seg(6.34, 17.66, 4.93, 19.07);
            seg(19.07, 4.93, 17.66, 6.34);
        }
        LucideIcon::Moon => {
            // Lucide `moon`: clean geometric crescent
            let pts = [
                map(12.0, 3.0),
                map(8.0, 4.5),
                map(5.0, 8.0),
                map(4.5, 12.5),
                map(6.5, 17.0),
                map(10.5, 20.0),
                map(15.5, 20.0),
                map(19.5, 17.5),
                map(21.0, 13.5),
                map(16.5, 14.0),
                map(12.5, 11.5),
                map(11.0, 7.5),
                map(12.0, 3.0),
            ];
            for w in pts.windows(2) {
                painter.line_segment([w[0], w[1]], stroke);
            }
        }
        LucideIcon::ChevronDown => {
            // Lucide `chevron-down`: (6,9) -> (12,15) -> (18,9)
            seg(6.0, 9.0, 12.0, 15.0);
            seg(12.0, 15.0, 18.0, 9.0);
        }
        LucideIcon::ChevronUp => {
            // Lucide `chevron-up`: (18,15) -> (12,9) -> (6,15)
            seg(18.0, 15.0, 12.0, 9.0);
            seg(12.0, 9.0, 6.0, 15.0);
        }
        LucideIcon::Terminal => {
            // Lucide `terminal`: (4,17) -> (10,11) -> (4,5) and (12,19) -> (20,19)
            seg(4.0, 17.0, 10.0, 11.0);
            seg(10.0, 11.0, 4.0, 5.0);
            seg(12.0, 19.0, 20.0, 19.0);
        }
        LucideIcon::RotateCcw => {
            // Lucide `rotate-ccw`: counter-clockwise arrow
            let pts = [
                map(3.0, 12.0),
                map(4.5, 16.8),
                map(8.5, 20.2),
                map(13.8, 20.8),
                map(18.5, 18.2),
                map(20.8, 13.2),
                map(19.5, 7.8),
                map(15.2, 4.2),
                map(9.8, 4.0),
                map(4.0, 8.0),
            ];
            for w in pts.windows(2) {
                painter.line_segment([w[0], w[1]], stroke);
            }
            seg(3.0, 3.5, 3.5, 8.5);
            seg(3.5, 8.5, 8.5, 8.0);
        }
        LucideIcon::Activity => {
            // Lucide `activity`: (22,12) -> (18,12) -> (15,21) -> (9,3) -> (6,12) -> (2,12)
            let pts = [
                map(22.0, 12.0),
                map(18.0, 12.0),
                map(15.0, 20.5),
                map(9.0, 3.5),
                map(6.0, 12.0),
                map(2.0, 12.0),
            ];
            for w in pts.windows(2) {
                painter.line_segment([w[0], w[1]], stroke);
            }
        }
        LucideIcon::Monitor => {
            // Lucide `monitor`: screen + stand
            let screen = Rect::from_min_max(map(2.0, 3.0), map(22.0, 17.0));
            painter.rect_stroke(screen, Rounding::same(2.0), stroke);
            seg(8.0, 21.0, 16.0, 21.0);
            seg(12.0, 17.0, 12.0, 21.0);
        }
        LucideIcon::Award => {
            // Lucide `award`: circle (12,8) r=6 + ribbon legs
            let r = (6.0 / 24.0) * rect.width();
            painter.circle_stroke(map(12.0, 8.5), r, stroke);
            seg(8.5, 13.8, 7.0, 22.0);
            seg(7.0, 22.0, 12.0, 19.0);
            seg(12.0, 19.0, 17.0, 22.0);
            seg(17.0, 22.0, 15.5, 13.8);
        }
        LucideIcon::Minus => {
            // Clean horizontal minimize bar
            seg(5.0, 12.0, 19.0, 12.0);
        }
        LucideIcon::Maximize => {
            // Clean square window frame
            let box_rect = Rect::from_min_max(map(5.0, 5.0), map(19.0, 19.0));
            painter.rect_stroke(box_rect, Rounding::same(1.5), stroke);
        }
        LucideIcon::Restore => {
            // Overlapping double window icon for unmaximize/restore
            let front = Rect::from_min_max(map(4.5, 7.5), map(16.5, 19.5));
            painter.rect_stroke(front, Rounding::same(1.5), stroke);
            seg(8.0, 7.5, 8.0, 4.5);
            seg(8.0, 4.5, 19.5, 4.5);
            seg(19.5, 4.5, 19.5, 16.0);
            seg(19.5, 16.0, 16.5, 16.0);
        }
        LucideIcon::Close => {
            // Crisp diagonal X
            seg(6.0, 6.0, 18.0, 18.0);
            seg(18.0, 6.0, 6.0, 18.0);
        }
        LucideIcon::Check => {
            // Lucide `check`: (20,6) -> (9,17) -> (4,12)
            seg(20.0, 6.0, 9.0, 17.0);
            seg(9.0, 17.0, 4.0, 12.0);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_unlagged_svg_rasterizes_cleanly() {
        let icon = load_unlagged_window_icon().expect("unlagged.svg must rasterize into 64x64 RGBA");
        assert_eq!(icon.width, 64);
        assert_eq!(icon.height, 64);
        assert_eq!(icon.rgba.len(), 64 * 64 * 4);
    }

    #[test]
    fn test_zero_unicode_emojis_in_codebase() {
        let files = [
            ("main.rs", include_str!("main.rs")),
            ("app.rs", include_str!("app.rs")),
            ("theme.rs", include_str!("theme.rs")),
            ("hardware.rs", include_str!("hardware.rs")),
            ("backup.rs", include_str!("backup.rs")),
            ("icons.rs", include_str!("icons.rs")),
            ("tweaks/display_cru.rs", include_str!("tweaks/display_cru.rs")),
            ("tweaks/gpu.rs", include_str!("tweaks/gpu.rs")),
            ("tweaks/interrupts.rs", include_str!("tweaks/interrupts.rs")),
            ("tweaks/network.rs", include_str!("tweaks/network.rs")),
            ("tweaks/scheduler.rs", include_str!("tweaks/scheduler.rs")),
            ("tweaks/windows_reg.rs", include_str!("tweaks/windows_reg.rs")),
        ];
        for (name, content) in files {
            for ch in content.chars() {
                let code = ch as u32;
                let is_forbidden_emoji_or_dingbat = (0x2600..=0x27BF).contains(&code)
                    || (0x1F300..=0x1FAFF).contains(&code)
                    || (0x25A0..=0x25FF).contains(&code);
                assert!(
                    !is_forbidden_emoji_or_dingbat,
                    "Forbidden Unicode emoji/symbol U+{:04X} ('{}') found in {}",
                    code, ch, name
                );
            }
        }
    }

    #[test]
    fn test_english_only_no_cyrillic_in_codebase() {
        let files = [
            ("main.rs", include_str!("main.rs")),
            ("app.rs", include_str!("app.rs")),
            ("theme.rs", include_str!("theme.rs")),
            ("hardware.rs", include_str!("hardware.rs")),
            ("backup.rs", include_str!("backup.rs")),
            ("icons.rs", include_str!("icons.rs")),
            ("tweaks/display_cru.rs", include_str!("tweaks/display_cru.rs")),
            ("tweaks/gpu.rs", include_str!("tweaks/gpu.rs")),
            ("tweaks/interrupts.rs", include_str!("tweaks/interrupts.rs")),
            ("tweaks/network.rs", include_str!("tweaks/network.rs")),
            ("tweaks/scheduler.rs", include_str!("tweaks/scheduler.rs")),
            ("tweaks/windows_reg.rs", include_str!("tweaks/windows_reg.rs")),
        ];
        for (name, content) in files {
            for ch in content.chars() {
                let code = ch as u32;
                let is_cyrillic = (0x0400..=0x04FF).contains(&code);
                assert!(
                    !is_cyrillic,
                    "Non-English Cyrillic character U+{:04X} ('{}') found in {}",
                    code, ch, name
                );
            }
        }
    }
}

