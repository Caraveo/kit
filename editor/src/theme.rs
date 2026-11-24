//! Dimension's look, translated to egui (github.com/Caraveo/dimension, ui/style.css).
#![allow(non_snake_case)]

use std::sync::atomic::{AtomicBool, Ordering};

use egui::{Color32, CornerRadius, FontFamily, FontId, Margin, Shadow, Stroke, TextStyle, Visuals};

/// Dimension's original dark palette. The 3D viewport overlays always use it.
pub mod dark {
    use egui::Color32;
    pub const BG: Color32 = Color32::from_rgb(0x10, 0x10, 0x17);
    pub const TEXT: Color32 = Color32::from_rgb(0xee, 0xeb, 0xfb);
    pub const TEXT_STRONG: Color32 = Color32::from_rgb(0xe8, 0xe2, 0xf6);
    pub const MUTED: Color32 = Color32::from_rgb(0xa1, 0x9a, 0xb2);
    pub const LABEL: Color32 = Color32::from_rgb(0xc7, 0xbb, 0xdd);
    pub const ACCENT: Color32 = Color32::from_rgb(0xc1, 0xa9, 0xff);
    pub const ACCENT_HOVER: Color32 = Color32::from_rgb(0xd0, 0xbe, 0xff);
    pub const ACCENT_TEXT: Color32 = Color32::from_rgb(0x20, 0x16, 0x32);
    pub const BADGE_TEXT: Color32 = Color32::from_rgb(0xd4, 0xbf, 0xff);
    pub const BADGE_BORDER: Color32 = Color32::from_rgb(0x47, 0x39, 0x59);
    pub const STAGE_BORDER: Color32 = Color32::from_rgb(0x36, 0x31, 0x47);
    pub const STAGE_BG: Color32 = Color32::from_rgb(0x15, 0x14, 0x20);
    pub const CARD: Color32 = Color32::from_rgb(0x13, 0x12, 0x1b);
    pub const CARD_BORDER: Color32 = Color32::from_rgb(0x2a, 0x25, 0x38);
    pub const DOCK_BG: Color32 = Color32::from_rgba_premultiplied(0x16, 0x13, 0x20, 0xed);
    pub const DOCK_BORDER: Color32 = Color32::from_rgb(0x4d, 0x43, 0x5f);
    pub const DOCK_TEXT: Color32 = Color32::from_rgb(0xc6, 0xbf, 0xd5);
    pub const PRESSED_TEXT: Color32 = Color32::from_rgb(0xe2, 0xd3, 0xff);
    pub const DIALOG_BG: Color32 = Color32::from_rgb(0x1c, 0x18, 0x28);
    pub const DIALOG_BORDER: Color32 = Color32::from_rgb(0x54, 0x45, 0x69);
    pub const NOTICE_BG: Color32 = Color32::from_rgb(0x1b, 0x17, 0x27);
    pub const NOTICE_BORDER: Color32 = Color32::from_rgb(0x57, 0x47, 0x6c);
    pub const FIELD_BG: Color32 = Color32::from_rgb(0x1a, 0x17, 0x25);
    pub const ERROR: Color32 = Color32::from_rgb(0xff, 0x8f, 0xa3);
    pub const WARN: Color32 = Color32::from_rgb(0xff, 0xd0, 0x8a);
    pub const AXIS_X: Color32 = Color32::from_rgb(0xff, 0x9b, 0xb0);
    pub const AXIS_Y: Color32 = Color32::from_rgb(0xa8, 0xf0, 0xc6);
    pub const AXIS_Z: Color32 = Color32::from_rgb(0x9f, 0xc3, 0xff);

    // Syntax colors for the x code editor.
    pub const CODE_KEYWORD: Color32 = Color32::from_rgb(0xc1, 0xa9, 0xff);
    pub const CODE_TYPE: Color32 = Color32::from_rgb(0x8f, 0xc8, 0xff);
    pub const CODE_STRING: Color32 = Color32::from_rgb(0xa8, 0xf0, 0xc6);
    pub const CODE_NUMBER: Color32 = Color32::from_rgb(0xff, 0xc9, 0x8f);
    pub const CODE_COMMENT: Color32 = Color32::from_rgb(0x6e, 0x66, 0x82);
    pub const CODE_MODULE: Color32 = Color32::from_rgb(0xff, 0xa8, 0xd8);
}

/// The light palette.
pub mod light {
    use egui::Color32;
    pub const BG: Color32 = Color32::from_rgb(0xf5, 0xf3, 0xfa);
    pub const TEXT: Color32 = Color32::from_rgb(0x2a, 0x24, 0x38);
    pub const TEXT_STRONG: Color32 = Color32::from_rgb(0x1a, 0x15, 0x26);
    pub const MUTED: Color32 = Color32::from_rgb(0x6c, 0x64, 0x80);
    pub const LABEL: Color32 = Color32::from_rgb(0x4c, 0x43, 0x63);
    pub const ACCENT: Color32 = Color32::from_rgb(0x7a, 0x58, 0xdc);
    pub const ACCENT_HOVER: Color32 = Color32::from_rgb(0x69, 0x46, 0xcf);
    pub const ACCENT_TEXT: Color32 = Color32::from_rgb(0xff, 0xff, 0xff);
    pub const BADGE_TEXT: Color32 = Color32::from_rgb(0x5d, 0x3f, 0xb8);
    pub const BADGE_BORDER: Color32 = Color32::from_rgb(0xcf, 0xc2, 0xef);
    pub const STAGE_BORDER: Color32 = Color32::from_rgb(0xd8, 0xd1, 0xe8);
    pub const STAGE_BG: Color32 = Color32::from_rgb(0xec, 0xe8, 0xf5);
    pub const CARD: Color32 = Color32::from_rgb(0xff, 0xff, 0xff);
    pub const CARD_BORDER: Color32 = Color32::from_rgb(0xe1, 0xdb, 0xee);
    pub const DOCK_BG: Color32 = Color32::from_rgba_premultiplied(0xf2, 0xf0, 0xf7, 0xf2);
    pub const DOCK_BORDER: Color32 = Color32::from_rgb(0xd2, 0xc8, 0xe6);
    pub const DOCK_TEXT: Color32 = Color32::from_rgb(0x4a, 0x40, 0x60);
    pub const PRESSED_TEXT: Color32 = Color32::from_rgb(0x4b, 0x2f, 0xa6);
    pub const DIALOG_BG: Color32 = Color32::from_rgb(0xff, 0xff, 0xff);
    pub const DIALOG_BORDER: Color32 = Color32::from_rgb(0xd2, 0xc8, 0xe6);
    pub const NOTICE_BG: Color32 = Color32::from_rgb(0xff, 0xff, 0xff);
    pub const NOTICE_BORDER: Color32 = Color32::from_rgb(0xc9, 0xbb, 0xee);
    pub const FIELD_BG: Color32 = Color32::from_rgb(0xef, 0xec, 0xf6);
    pub const ERROR: Color32 = Color32::from_rgb(0xc4, 0x2b, 0x4b);
    pub const WARN: Color32 = Color32::from_rgb(0xa0, 0x62, 0x00);
    pub const AXIS_X: Color32 = Color32::from_rgb(0xd0, 0x2f, 0x58);
    pub const AXIS_Y: Color32 = Color32::from_rgb(0x1d, 0x93, 0x55);
    pub const AXIS_Z: Color32 = Color32::from_rgb(0x2c, 0x69, 0xd2);
    pub const CODE_KEYWORD: Color32 = Color32::from_rgb(0x6a, 0x3f, 0xd0);
    pub const CODE_TYPE: Color32 = Color32::from_rgb(0x1e, 0x6c, 0xb5);
    pub const CODE_STRING: Color32 = Color32::from_rgb(0x1d, 0x85, 0x48);
    pub const CODE_NUMBER: Color32 = Color32::from_rgb(0xad, 0x5a, 0x00);
    pub const CODE_COMMENT: Color32 = Color32::from_rgb(0x98, 0x90, 0xab);
    pub const CODE_MODULE: Color32 = Color32::from_rgb(0xbb, 0x35, 0x86);
}

static LIGHT: AtomicBool = AtomicBool::new(false);

/// Is the light theme active?
pub fn is_light() -> bool {
    LIGHT.load(Ordering::Relaxed)
}


static VIEWPORT_LIGHT: AtomicBool = AtomicBool::new(false);

/// Colors for overlays drawn on the 3D viewport. They follow the viewport's
/// background (white in light mode unless the scene sets its own), not the panels.
#[allow(dead_code)]
pub mod viewport {
    use super::*;

    /// Is the viewport background light this frame?
    pub fn is_light() -> bool {
        VIEWPORT_LIGHT.load(Ordering::Relaxed)
    }

    pub fn set_light(light: bool) {
        VIEWPORT_LIGHT.store(light, Ordering::Relaxed);
    }

    /// Pick between a color for a light and a dark background.
    pub fn pick(light: Color32, dark: Color32) -> Color32 {
        if is_light() { light } else { dark }
    }

    pub fn BG() -> Color32 {
        pick(light::BG, dark::BG)
    }
    pub fn TEXT() -> Color32 {
        pick(light::TEXT, dark::TEXT)
    }
    pub fn TEXT_STRONG() -> Color32 {
        pick(light::TEXT_STRONG, dark::TEXT_STRONG)
    }
    pub fn MUTED() -> Color32 {
        pick(light::MUTED, dark::MUTED)
    }
    pub fn LABEL() -> Color32 {
        pick(light::LABEL, dark::LABEL)
    }
    pub fn ACCENT() -> Color32 {
        pick(light::ACCENT, dark::ACCENT)
    }
    pub fn ACCENT_HOVER() -> Color32 {
        pick(light::ACCENT_HOVER, dark::ACCENT_HOVER)
    }
    pub fn ACCENT_TEXT() -> Color32 {
        pick(light::ACCENT_TEXT, dark::ACCENT_TEXT)
    }
    pub fn BADGE_TEXT() -> Color32 {
        pick(light::BADGE_TEXT, dark::BADGE_TEXT)
    }
    pub fn BADGE_BORDER() -> Color32 {
        pick(light::BADGE_BORDER, dark::BADGE_BORDER)
    }
    pub fn STAGE_BORDER() -> Color32 {
        pick(light::STAGE_BORDER, dark::STAGE_BORDER)
    }
    pub fn STAGE_BG() -> Color32 {
        pick(light::STAGE_BG, dark::STAGE_BG)
    }
    pub fn CARD() -> Color32 {
        pick(light::CARD, dark::CARD)
    }
    pub fn CARD_BORDER() -> Color32 {
        pick(light::CARD_BORDER, dark::CARD_BORDER)
    }
    pub fn DOCK_BG() -> Color32 {
        pick(light::DOCK_BG, dark::DOCK_BG)
    }
    pub fn DOCK_BORDER() -> Color32 {
        pick(light::DOCK_BORDER, dark::DOCK_BORDER)
    }
    pub fn DOCK_TEXT() -> Color32 {
        pick(light::DOCK_TEXT, dark::DOCK_TEXT)
    }
    pub fn PRESSED_TEXT() -> Color32 {
        pick(light::PRESSED_TEXT, dark::PRESSED_TEXT)
    }
    pub fn DIALOG_BG() -> Color32 {
        pick(light::DIALOG_BG, dark::DIALOG_BG)
    }
    pub fn DIALOG_BORDER() -> Color32 {
        pick(light::DIALOG_BORDER, dark::DIALOG_BORDER)
    }
    pub fn NOTICE_BG() -> Color32 {
        pick(light::NOTICE_BG, dark::NOTICE_BG)
    }
    pub fn NOTICE_BORDER() -> Color32 {
        pick(light::NOTICE_BORDER, dark::NOTICE_BORDER)
    }
    pub fn FIELD_BG() -> Color32 {
        pick(light::FIELD_BG, dark::FIELD_BG)
    }
    pub fn ERROR() -> Color32 {
        pick(light::ERROR, dark::ERROR)
    }
    pub fn WARN() -> Color32 {
        pick(light::WARN, dark::WARN)
    }
    pub fn AXIS_X() -> Color32 {
        pick(light::AXIS_X, dark::AXIS_X)
    }
    pub fn AXIS_Y() -> Color32 {
        pick(light::AXIS_Y, dark::AXIS_Y)
    }
    pub fn AXIS_Z() -> Color32 {
        pick(light::AXIS_Z, dark::AXIS_Z)
    }
    pub fn CODE_KEYWORD() -> Color32 {
        pick(light::CODE_KEYWORD, dark::CODE_KEYWORD)
    }
    pub fn CODE_TYPE() -> Color32 {
        pick(light::CODE_TYPE, dark::CODE_TYPE)
    }
    pub fn CODE_STRING() -> Color32 {
        pick(light::CODE_STRING, dark::CODE_STRING)
    }
    pub fn CODE_NUMBER() -> Color32 {
        pick(light::CODE_NUMBER, dark::CODE_NUMBER)
    }
    pub fn CODE_COMMENT() -> Color32 {
        pick(light::CODE_COMMENT, dark::CODE_COMMENT)
    }
    pub fn CODE_MODULE() -> Color32 {
        pick(light::CODE_MODULE, dark::CODE_MODULE)
    }
}

// Theme-aware colors (functions so they follow the light/dark switch).
pub fn BG() -> Color32 {
    if is_light() { light::BG } else { dark::BG }
}
pub fn TEXT() -> Color32 {
    if is_light() { light::TEXT } else { dark::TEXT }
}
pub fn TEXT_STRONG() -> Color32 {
    if is_light() { light::TEXT_STRONG } else { dark::TEXT_STRONG }
}
pub fn MUTED() -> Color32 {
    if is_light() { light::MUTED } else { dark::MUTED }
}
pub fn LABEL() -> Color32 {
    if is_light() { light::LABEL } else { dark::LABEL }
}
pub fn ACCENT() -> Color32 {
    if is_light() { light::ACCENT } else { dark::ACCENT }
}
pub fn ACCENT_HOVER() -> Color32 {
    if is_light() { light::ACCENT_HOVER } else { dark::ACCENT_HOVER }
}
pub fn ACCENT_TEXT() -> Color32 {
    if is_light() { light::ACCENT_TEXT } else { dark::ACCENT_TEXT }
}
pub fn BADGE_TEXT() -> Color32 {
    if is_light() { light::BADGE_TEXT } else { dark::BADGE_TEXT }
}
pub fn BADGE_BORDER() -> Color32 {
    if is_light() { light::BADGE_BORDER } else { dark::BADGE_BORDER }
}
pub fn STAGE_BORDER() -> Color32 {
    if is_light() { light::STAGE_BORDER } else { dark::STAGE_BORDER }
}
pub fn STAGE_BG() -> Color32 {
    if is_light() { light::STAGE_BG } else { dark::STAGE_BG }
}
pub fn CARD() -> Color32 {
    if is_light() { light::CARD } else { dark::CARD }
}
pub fn CARD_BORDER() -> Color32 {
    if is_light() { light::CARD_BORDER } else { dark::CARD_BORDER }
}
pub fn DOCK_BG() -> Color32 {
    if is_light() { light::DOCK_BG } else { dark::DOCK_BG }
}
pub fn DOCK_BORDER() -> Color32 {
    if is_light() { light::DOCK_BORDER } else { dark::DOCK_BORDER }
}
pub fn DOCK_TEXT() -> Color32 {
    if is_light() { light::DOCK_TEXT } else { dark::DOCK_TEXT }
}
pub fn PRESSED_TEXT() -> Color32 {
    if is_light() { light::PRESSED_TEXT } else { dark::PRESSED_TEXT }
}
pub fn DIALOG_BG() -> Color32 {
    if is_light() { light::DIALOG_BG } else { dark::DIALOG_BG }
}
pub fn DIALOG_BORDER() -> Color32 {
    if is_light() { light::DIALOG_BORDER } else { dark::DIALOG_BORDER }
}
pub fn NOTICE_BG() -> Color32 {
    if is_light() { light::NOTICE_BG } else { dark::NOTICE_BG }
}
pub fn NOTICE_BORDER() -> Color32 {
    if is_light() { light::NOTICE_BORDER } else { dark::NOTICE_BORDER }
}
pub fn FIELD_BG() -> Color32 {
    if is_light() { light::FIELD_BG } else { dark::FIELD_BG }
}
pub fn ERROR() -> Color32 {
    if is_light() { light::ERROR } else { dark::ERROR }
}
pub fn WARN() -> Color32 {
    if is_light() { light::WARN } else { dark::WARN }
}
pub fn AXIS_X() -> Color32 {
    if is_light() { light::AXIS_X } else { dark::AXIS_X }
}
pub fn AXIS_Y() -> Color32 {
    if is_light() { light::AXIS_Y } else { dark::AXIS_Y }
}
pub fn AXIS_Z() -> Color32 {
    if is_light() { light::AXIS_Z } else { dark::AXIS_Z }
}
pub fn CODE_KEYWORD() -> Color32 {
    if is_light() { light::CODE_KEYWORD } else { dark::CODE_KEYWORD }
}
pub fn CODE_TYPE() -> Color32 {
    if is_light() { light::CODE_TYPE } else { dark::CODE_TYPE }
}
pub fn CODE_STRING() -> Color32 {
    if is_light() { light::CODE_STRING } else { dark::CODE_STRING }
}
pub fn CODE_NUMBER() -> Color32 {
    if is_light() { light::CODE_NUMBER } else { dark::CODE_NUMBER }
}
pub fn CODE_COMMENT() -> Color32 {
    if is_light() { light::CODE_COMMENT } else { dark::CODE_COMMENT }
}
pub fn CODE_MODULE() -> Color32 {
    if is_light() { light::CODE_MODULE } else { dark::CODE_MODULE }
}

pub fn apply(ctx: &egui::Context) {
    // Symbols (◆ ▸ ↶ ...) live in the monospace font; let proportional text fall back to it.
    let mut fonts = egui::FontDefinitions::default();
    let mono = fonts.families.get(&FontFamily::Monospace).and_then(|f| f.first().cloned());
    if let (Some(mono), Some(prop)) = (mono, fonts.families.get_mut(&FontFamily::Proportional)) {
        prop.push(mono);
    }
    ctx.set_fonts(fonts);
    set_light(ctx, is_light());
}

/// Switch between the light and dark theme.
pub fn set_light(ctx: &egui::Context, light: bool) {
    LIGHT.store(light, Ordering::Relaxed);
    ctx.set_theme(if light { egui::Theme::Light } else { egui::Theme::Dark });
    let mut style = (*ctx.global_style()).clone();
    style.text_styles = [
        (TextStyle::Heading, FontId::new(20.0, FontFamily::Proportional)),
        (TextStyle::Body, FontId::new(13.0, FontFamily::Proportional)),
        (TextStyle::Button, FontId::new(13.0, FontFamily::Proportional)),
        (TextStyle::Small, FontId::new(10.5, FontFamily::Proportional)),
        (TextStyle::Monospace, FontId::new(13.0, FontFamily::Monospace)),
    ]
    .into();
    style.spacing.item_spacing = egui::vec2(8.0, 6.0);
    style.spacing.button_padding = egui::vec2(10.0, 5.0);
    style.spacing.interact_size.y = 24.0;
    style.spacing.slider_width = 150.0;
    style.spacing.slider_rail_height = 6.0;
    style.spacing.window_margin = Margin::same(18);
    style.spacing.menu_margin = Margin::same(6);

    let mut v = if light { Visuals::light() } else { Visuals::dark() };
    v.override_text_color = Some(TEXT());
    v.panel_fill = BG();
    v.window_fill = DIALOG_BG();
    v.window_stroke = Stroke::new(1.0, DIALOG_BORDER());
    v.window_corner_radius = CornerRadius::same(15);
    v.window_shadow = Shadow { offset: [0, 18], blur: 60, spread: 0, color: Color32::from_black_alpha(if light { 50 } else { 170 }) };
    v.popup_shadow = Shadow { offset: [0, 12], blur: 35, spread: 0, color: Color32::from_black_alpha(if light { 35 } else { 110 }) };
    v.menu_corner_radius = CornerRadius::same(10);
    v.extreme_bg_color = FIELD_BG();
    v.text_edit_bg_color = Some(FIELD_BG());
    v.code_bg_color = FIELD_BG();
    v.faint_bg_color = CARD();
    v.hyperlink_color = ACCENT();
    v.error_fg_color = ERROR();
    v.warn_fg_color = WARN();
    v.selection.bg_fill = if light { Color32::from_rgba_unmultiplied(0x7a, 0x58, 0xdc, 0x40) } else { Color32::from_rgba_unmultiplied(0xb6, 0x9a, 0xf3, 0x55) };
    v.selection.stroke = Stroke::new(1.0, ACCENT());
    v.collapsing_header_frame = false;
    // Filled slider track in the accent color, visible on dark dialogs.
    v.slider_trailing_fill = true;
    v.indent_has_left_vline = false;

    let r = CornerRadius::same(8);
    v.widgets.noninteractive.bg_fill = CARD();
    v.widgets.noninteractive.weak_bg_fill = CARD();
    v.widgets.noninteractive.bg_stroke = Stroke::new(1.0, CARD_BORDER());
    v.widgets.noninteractive.fg_stroke = Stroke::new(1.0, LABEL());
    v.widgets.noninteractive.corner_radius = r;

    v.widgets.inactive.bg_fill = pick(light, Color32::from_rgb(0xdc, 0xd5, 0xec), Color32::from_rgb(0x2e, 0x28, 0x40)); // slider rails, checkbox boxes
    v.widgets.inactive.weak_bg_fill = FIELD_BG();
    v.widgets.inactive.bg_stroke = Stroke::new(1.0, pick(light, Color32::from_rgb(0xdb, 0xd4, 0xea), Color32::from_rgb(0x2c, 0x27, 0x3b)));
    v.widgets.inactive.fg_stroke = Stroke::new(1.0, TEXT());
    v.widgets.inactive.corner_radius = r;

    v.widgets.hovered.bg_fill = pick(light, Color32::from_rgb(0xe9, 0xe4, 0xf5), Color32::from_rgb(0x26, 0x21, 0x35));
    v.widgets.hovered.weak_bg_fill = pick(light, Color32::from_rgb(0xe9, 0xe4, 0xf5), Color32::from_rgb(0x26, 0x21, 0x35));
    v.widgets.hovered.bg_stroke = Stroke::new(1.0, pick(light, Color32::from_rgb(0xc4, 0xb8, 0xe0), Color32::from_rgb(0x4d, 0x43, 0x5f)));
    v.widgets.hovered.fg_stroke = Stroke::new(1.0, TEXT_STRONG());
    v.widgets.hovered.corner_radius = r;
    v.widgets.hovered.expansion = 0.0;

    v.widgets.active.bg_fill = pick(light, Color32::from_rgb(0xe2, 0xd9, 0xf7), Color32::from_rgb(0x33, 0x2a, 0x4a));
    v.widgets.active.weak_bg_fill = pick(light, Color32::from_rgb(0xe2, 0xd9, 0xf7), Color32::from_rgb(0x33, 0x2a, 0x4a));
    v.widgets.active.bg_stroke = Stroke::new(1.0, ACCENT());
    v.widgets.active.fg_stroke = Stroke::new(1.0, PRESSED_TEXT());
    v.widgets.active.corner_radius = r;
    v.widgets.active.expansion = 0.0;

    v.widgets.open = v.widgets.active;
    style.visuals = v;
    ctx.set_global_style(style);
}

fn pick(light: bool, l: Color32, d: Color32) -> Color32 {
    if light { l } else { d }
}

/// A rounded card like Dimension's panels.
pub fn card() -> egui::Frame {
    egui::Frame::new()
        .fill(CARD())
        .stroke(Stroke::new(1.0, CARD_BORDER()))
        .corner_radius(CornerRadius::same(16))
        .inner_margin(Margin::same(12))
}
