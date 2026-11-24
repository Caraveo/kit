//! Small styled widgets used across the editor.

use crate::theme::*;
use egui::{Button, CornerRadius, Response, RichText, Stroke, Ui, Vec2};

/// Transparent button with a hover tint (Dimension's header and icon buttons).
pub fn ghost(ui: &mut Ui, text: impl Into<String>) -> Response {
    ui.add(
        Button::new(RichText::new(text.into()).color(TEXT()))
            .fill(egui::Color32::TRANSPARENT)
            .stroke(Stroke::NONE)
            .corner_radius(CornerRadius::same(9)),
    )
}

/// Ghost button with a keyboard hint on the right.
pub fn ghost_kbd(ui: &mut Ui, text: &str, kbd: &str) -> Response {
    let mut job = egui::text::LayoutJob::default();
    job.append(
        text,
        0.0,
        egui::TextFormat { color: TEXT(), font_id: egui::FontId::proportional(13.0), ..Default::default() },
    );
    job.append(
        kbd,
        8.0,
        egui::TextFormat {
            color: MUTED(),
            font_id: egui::FontId::proportional(10.0),
            valign: egui::Align::Center,
            ..Default::default()
        },
    );
    ui.add(Button::new(job).fill(egui::Color32::TRANSPARENT).stroke(Stroke::NONE).corner_radius(CornerRadius::same(9)))
}

/// The lavender primary button ("Open model" in Dimension; Play in Kit).
pub fn primary(ui: &mut Ui, text: &str, kbd: Option<&str>) -> Response {
    let mut job = egui::text::LayoutJob::default();
    job.append(
        text,
        0.0,
        egui::TextFormat { color: ACCENT_TEXT(), font_id: egui::FontId::proportional(14.0), ..Default::default() },
    );
    if let Some(k) = kbd {
        job.append(
            k,
            10.0,
            egui::TextFormat {
                color: ACCENT_TEXT().gamma_multiply(0.75),
                background: egui::Color32::from_rgba_unmultiplied(0x29, 0x1c, 0x43, 0x22),
                font_id: egui::FontId::proportional(10.0),
                valign: egui::Align::Center,
                ..Default::default()
            },
        );
    }
    let hovered_id = ui.next_auto_id();
    let hovered = ui.ctx().read_response(hovered_id).is_some_and(|r| r.hovered());
    ui.add(
        Button::new(job)
            .fill(if hovered { ACCENT_HOVER() } else { ACCENT() })
            .stroke(Stroke::NONE)
            .corner_radius(CornerRadius::same(9))
            .min_size(Vec2::new(0.0, 34.0)),
    )
}

/// A dock toggle (Wireframe / Lighting / Grid ...).
pub fn dock_toggle(ui: &mut Ui, icon: &str, label: &str, on: bool) -> Response {
    let text = RichText::new(format!("{icon}  {label}")).size(13.0).color(if on { PRESSED_TEXT() } else { DOCK_TEXT() });
    let button = Button::new(text)
        .fill(if on {
            egui::Color32::from_rgba_unmultiplied(0xb6, 0x9a, 0xf3, 0x2b)
        } else {
            egui::Color32::TRANSPARENT
        })
        .stroke(if on {
            Stroke::new(1.0, egui::Color32::from_rgba_unmultiplied(0xbe, 0x9b, 0xff, 0x30))
        } else {
            Stroke::NONE
        })
        .corner_radius(CornerRadius::same(9))
        .min_size(Vec2::new(0.0, 34.0));
    ui.add(button)
}

/// A small bordered tag ("DEMO" / "MODEL" badges).
pub fn badge(ui: &mut Ui, text: &str) {
    egui::Frame::new()
        .stroke(Stroke::new(1.0, BADGE_BORDER()))
        .corner_radius(CornerRadius::same(4))
        .inner_margin(egui::Margin::symmetric(6, 2))
        .show(ui, |ui| {
            ui.label(RichText::new(text).size(9.5).color(BADGE_TEXT()).strong());
        });
}

/// Uppercase, letter-spaced section label like Dimension's "● VIEWPORT".
pub fn section_label(ui: &mut Ui, text: &str) {
    ui.label(spaced(text, 10.0, LABEL()));
}

pub fn spaced(text: &str, size: f32, color: egui::Color32) -> egui::text::LayoutJob {
    let mut job = egui::text::LayoutJob::default();
    job.append(
        text,
        0.0,
        egui::TextFormat {
            font_id: egui::FontId::proportional(size),
            color,
            extra_letter_spacing: size * 0.17,
            ..Default::default()
        },
    );
    job
}

/// Painted cube logo (the Dimension mark, in Kit's colors).
pub fn logo(ui: &mut Ui, size: f32) -> Response {
    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(size), egui::Sense::hover());
    let p = ui.painter();
    let c = rect.center();
    let r = size * 0.46;
    let hex: Vec<egui::Pos2> = (0..6)
        .map(|i| {
            let a = std::f32::consts::FRAC_PI_3 * i as f32 - std::f32::consts::FRAC_PI_2;
            c + egui::vec2(a.cos(), a.sin()) * r
        })
        .collect();
    let stroke = Stroke::new(size * 0.06, ACCENT());
    p.add(egui::Shape::convex_polygon(
        vec![hex[0], hex[1], c, hex[5]],
        egui::Color32::from_rgba_unmultiplied(0xc1, 0xa9, 0xff, 0x40),
        Stroke::NONE,
    ));
    p.add(egui::Shape::closed_line(hex.clone(), stroke));
    p.line_segment([c, hex[1]], stroke);
    p.line_segment([c, hex[5]], stroke);
    p.line_segment([c, hex[3]], stroke);
    resp
}
