//! Colors, fonts and widget styling (mirrors the extension's look).

use egui::{Color32, Context, CornerRadius, FontData, FontDefinitions, FontFamily, Stroke, Theme, Visuals};
use std::sync::Arc;

pub const ACCENT: Color32 = Color32::from_rgb(0x33, 0x5c, 0x4d);
pub const ACCENT_DARK: Color32 = Color32::from_rgb(0x25, 0x4c, 0x3e);
pub const INK: Color32 = Color32::from_rgb(0x25, 0x33, 0x4b);
pub const MUTED: Color32 = Color32::from_rgb(0x75, 0x80, 0x8e);
pub const BG: Color32 = Color32::from_rgb(0xed, 0xf0, 0xf3);
pub const PANEL: Color32 = Color32::WHITE;
pub const BORDER: Color32 = Color32::from_rgb(0xe0, 0xe4, 0xe8);
pub const ACTIVE_BG: Color32 = Color32::from_rgb(0xe7, 0xf0, 0xea);
pub const ACTIVE_FG: Color32 = Color32::from_rgb(0x2d, 0x60, 0x49);
pub const HOVER_BG: Color32 = Color32::from_rgb(0xe8, 0xee, 0xeb);
pub const ERROR: Color32 = Color32::from_rgb(0xb4, 0x3c, 0x32);
pub const SELECT_BLUE: Color32 = Color32::from_rgb(0x4c, 0x82, 0xc3);
pub const FORM_BG: Color32 = Color32::from_rgb(0xe9, 0xef, 0xfd);

pub fn install(ctx: &Context) {
    let mut fonts = FontDefinitions::default();
    fonts.font_data.insert("dejavu".into(), Arc::new(FontData::from_static(crate::font::FONT_BYTES)));
    fonts.families.insert(FontFamily::Name(crate::font::FAMILY.into()), vec!["dejavu".into()]);
    // Prefer the native Windows UI font when available; DejaVu covers everything else.
    let mut primary = None;
    if let Some(windir) = std::env::var_os("WINDIR") {
        let path = std::path::PathBuf::from(windir).join("Fonts").join("segoeui.ttf");
        if let Ok(bytes) = std::fs::read(path) {
            fonts.font_data.insert("segoe".into(), Arc::new(FontData::from_owned(bytes)));
            primary = Some("segoe");
        }
    }
    let prop = fonts.families.entry(FontFamily::Proportional).or_default();
    if let Some(p) = primary {
        prop.insert(0, p.into());
    }
    prop.insert(if primary.is_some() { 1 } else { 0 }, "dejavu".into());
    ctx.set_fonts(fonts);

    ctx.set_theme(Theme::Light);
    let mut v = Visuals::light();
    v.panel_fill = PANEL;
    v.window_fill = PANEL;
    v.extreme_bg_color = Color32::WHITE;
    v.faint_bg_color = Color32::from_rgb(0xf5, 0xf7, 0xf9);
    v.override_text_color = Some(INK);
    v.hyperlink_color = ACCENT;
    v.selection.bg_fill = Color32::from_rgb(0xc8, 0xdc, 0xd0);
    v.selection.stroke = Stroke::new(1.0, ACCENT);
    v.window_corner_radius = CornerRadius::same(12);
    v.menu_corner_radius = CornerRadius::same(10);
    v.window_stroke = Stroke::new(1.0, BORDER);
    let r = CornerRadius::same(7);
    for w in [&mut v.widgets.noninteractive, &mut v.widgets.inactive, &mut v.widgets.hovered, &mut v.widgets.active, &mut v.widgets.open] {
        w.corner_radius = r;
    }
    v.widgets.noninteractive.bg_stroke = Stroke::new(1.0, BORDER);
    v.widgets.inactive.weak_bg_fill = Color32::WHITE;
    v.widgets.inactive.bg_fill = Color32::WHITE;
    v.widgets.inactive.bg_stroke = Stroke::new(1.0, Color32::from_rgb(0xd7, 0xde, 0xdb));
    v.widgets.hovered.weak_bg_fill = HOVER_BG;
    v.widgets.hovered.bg_fill = HOVER_BG;
    v.widgets.hovered.bg_stroke = Stroke::new(1.0, Color32::from_rgb(0xc9, 0xd3, 0xce));
    v.widgets.active.weak_bg_fill = ACTIVE_BG;
    v.widgets.active.bg_fill = ACTIVE_BG;
    v.widgets.active.bg_stroke = Stroke::new(1.0, ACCENT);
    v.widgets.open.weak_bg_fill = ACTIVE_BG;
    ctx.set_visuals_of(Theme::Light, v);
    ctx.style_mut_of(Theme::Light, |s| {
        s.spacing.item_spacing = egui::vec2(8.0, 6.0);
        s.spacing.button_padding = egui::vec2(10.0, 5.0);
        s.spacing.interact_size.y = 28.0;
        s.text_styles.insert(egui::TextStyle::Body, egui::FontId::proportional(14.0));
        s.text_styles.insert(egui::TextStyle::Button, egui::FontId::proportional(14.0));
        s.text_styles.insert(egui::TextStyle::Small, egui::FontId::proportional(12.0));
        s.text_styles.insert(egui::TextStyle::Heading, egui::FontId::proportional(22.0));
    });
    ctx.options_mut(|o| o.zoom_with_keyboard = false);
}
