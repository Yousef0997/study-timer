//! Look and feel: a chalkboard (dark) and a ruled notebook page (light), set in the Red Hat type family.

use eframe::egui::{
    self, Color32, CornerRadius, FontData, FontDefinitions, FontFamily, FontId, Stroke, TextStyle, Theme, Visuals,
};
use std::sync::Arc;

use crate::model::Kind;

#[derive(Clone, Copy)]
pub struct Palette {
    pub dark: bool,
    pub bg: Color32,
    pub raised: Color32,
    pub line: Color32,
    pub text: Color32,
    pub dim: Color32,
    pub faint: Color32,
    pub accent: Color32,
    pub on_accent: Color32,
    pub danger: Color32,
    /// Ruled lines drawn behind the light theme; transparent on the chalkboard.
    pub rule: Color32,
    pub margin: Color32,
}

const fn hex(rgb: u32) -> Color32 {
    Color32::from_rgb((rgb >> 16) as u8, (rgb >> 8) as u8, rgb as u8)
}

pub const CHALKBOARD: Palette = Palette {
    dark: true,
    bg: hex(0x1F3A32),
    raised: hex(0x19302A),
    line: hex(0x34574B),
    text: hex(0xEEF0E6),
    dim: hex(0xA6BBAF),
    faint: hex(0x6F8C80),
    accent: hex(0xF2D374),
    on_accent: hex(0x1F3A32),
    danger: hex(0xF59A8F),
    rule: Color32::TRANSPARENT,
    margin: Color32::TRANSPARENT,
};

pub const NOTEBOOK: Palette = Palette {
    dark: false,
    bg: hex(0xF5F7FA),
    raised: hex(0xFFFFFF),
    line: hex(0xD3DCEA),
    text: hex(0x1E2A44),
    dim: hex(0x56627E),
    faint: hex(0x8A95AD),
    accent: hex(0x2B4C9A),
    on_accent: hex(0xFFFFFF),
    danger: hex(0xB3263B),
    rule: hex(0xE4EAF3),
    margin: hex(0xEDB8C0),
};

pub fn palette(dark: bool) -> Palette {
    if dark { CHALKBOARD } else { NOTEBOOK }
}

/// Each kind of activity has its own chalk colour; it is always shown next to its name.
pub fn kind_color(kind: Kind, dark: bool) -> Color32 {
    match (kind, dark) {
        (Kind::Course, true) => hex(0xF2D374),
        (Kind::Rest, true) => hex(0x8EC5F0),
        (Kind::Gym, true) => hex(0xF2937E),
        (Kind::Meal, true) => hex(0xC9AEF2),
        (Kind::Other, true) => hex(0x9FE3C8),
        (Kind::Course, false) => hex(0xA87400),
        (Kind::Rest, false) => hex(0x2B73B5),
        (Kind::Gym, false) => hex(0xC2482F),
        (Kind::Meal, false) => hex(0x7A4FC0),
        (Kind::Other, false) => hex(0x1E8A6A),
    }
}

pub fn display_font(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name("display".into()))
}

pub fn clock_font(size: f32) -> FontId {
    FontId::new(size, FontFamily::Monospace)
}

pub fn install_fonts(ctx: &egui::Context) {
    let mut fonts = FontDefinitions::default();
    let load = |fonts: &mut FontDefinitions, key: &str, file: &str| -> bool {
        match std::fs::read(format!("/usr/share/fonts/redhat-vf/{file}")) {
            Ok(bytes) => {
                fonts.font_data.insert(key.into(), Arc::new(FontData::from_owned(bytes)));
                true
            }
            Err(_) => false,
        }
    };
    let text = load(&mut fonts, "rh-text", "RedHatText[wght].ttf");
    let display = load(&mut fonts, "rh-display", "RedHatDisplay[wght].ttf");
    let mono = load(&mut fonts, "rh-mono", "RedHatMono[wght].ttf");

    let proportional = fonts.families.get(&FontFamily::Proportional).cloned().unwrap_or_default();
    if text {
        fonts.families.entry(FontFamily::Proportional).or_default().insert(0, "rh-text".into());
    }
    if mono {
        fonts.families.entry(FontFamily::Monospace).or_default().insert(0, "rh-mono".into());
    }
    // Headings use Red Hat Display, falling back to the body fonts (and their symbols).
    let mut display_family = if display { vec!["rh-display".to_string()] } else { vec![] };
    if text {
        display_family.push("rh-text".into());
    }
    display_family.extend(proportional);
    fonts.families.insert(FontFamily::Name("display".into()), display_family);
    ctx.set_fonts(fonts);
}

pub fn apply(ctx: &egui::Context, dark: bool) {
    let p = palette(dark);
    let mut v = if dark { Visuals::dark() } else { Visuals::light() };
    v.override_text_color = Some(p.text);
    v.panel_fill = p.bg;
    v.window_fill = p.raised;
    v.window_stroke = Stroke::new(1.0, p.line);
    v.extreme_bg_color = if dark { hex(0x16291F) } else { hex(0xFFFFFF) };
    v.faint_bg_color = p.raised;
    v.code_bg_color = p.raised;
    v.hyperlink_color = p.accent;
    v.selection.bg_fill = p.accent.gamma_multiply(if dark { 0.35 } else { 0.25 });
    v.selection.stroke = Stroke::new(1.0, p.accent);

    let radius = CornerRadius::same(6);
    let w = &mut v.widgets;
    w.noninteractive.bg_fill = p.bg;
    w.noninteractive.weak_bg_fill = p.bg;
    w.noninteractive.bg_stroke = Stroke::new(1.0, p.line);
    w.noninteractive.fg_stroke = Stroke::new(1.0, p.text);
    w.inactive.bg_fill = p.line;
    w.inactive.weak_bg_fill = p.raised;
    w.inactive.bg_stroke = Stroke::new(1.0, p.line);
    w.inactive.fg_stroke = Stroke::new(1.0, p.text);
    w.hovered.weak_bg_fill = if dark { hex(0x28473D) } else { hex(0xEAF0F8) };
    w.hovered.bg_fill = p.faint;
    w.hovered.bg_stroke = Stroke::new(1.0, p.dim);
    w.hovered.fg_stroke = Stroke::new(1.5, p.text);
    w.active.weak_bg_fill = if dark { hex(0x2F5246) } else { hex(0xDDE6F3) };
    w.active.bg_fill = p.accent;
    w.active.bg_stroke = Stroke::new(1.0, p.accent);
    w.active.fg_stroke = Stroke::new(1.5, p.text);
    w.open = w.active;
    for s in [&mut w.noninteractive, &mut w.inactive, &mut w.hovered, &mut w.active, &mut w.open] {
        s.corner_radius = radius;
    }
    let theme = if dark { Theme::Dark } else { Theme::Light };
    ctx.set_visuals_of(theme, v);
    ctx.style_mut_of(theme, |style| {
        style.spacing.item_spacing = egui::vec2(8.0, 8.0);
        style.spacing.button_padding = egui::vec2(12.0, 6.0);
        style.spacing.interact_size.y = 30.0;
        style.text_styles = [
            (TextStyle::Heading, display_font(24.0)),
            (TextStyle::Body, FontId::proportional(15.0)),
            (TextStyle::Button, FontId::proportional(15.0)),
            (TextStyle::Small, FontId::proportional(12.5)),
            (TextStyle::Monospace, FontId::monospace(14.0)),
        ]
        .into();
    });
    ctx.set_theme(theme);
}
