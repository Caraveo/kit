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

/// Paint the Kit app icon at the requested size.
pub fn logo(ui: &mut Ui, size: f32) -> Response {
    let id = egui::Id::new("kit-logo-texture");
    let texture = match ui.ctx().data(|data| data.get_temp::<egui::TextureHandle>(id)) {
        Some(texture) => texture,
        None => {
            let image = image::load_from_memory(include_bytes!("../../packaging/icons/kit-128.png"))
                .expect("valid bundled Kit logo")
                .into_rgba8();
            let pixels = egui::ColorImage::from_rgba_unmultiplied(
                [image.width() as usize, image.height() as usize],
                image.as_raw(),
            );
            let texture = ui.ctx().load_texture("Kit logo", pixels, egui::TextureOptions::LINEAR);
            ui.ctx().data_mut(|data| data.insert_temp(id, texture.clone()));
            texture
        }
    };
    ui.image((texture.id(), Vec2::splat(size)))
}
