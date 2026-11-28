//! Project → Publish: the thin editor entry point for the official
//! `kit.publish` Circuit. The signed-in REX token is reused by the account
//! circuits page;
//! the server remains responsible for ownership, revision immutability, and
//! approval.

use crate::{export, rex_panel::RexPanel, state::Editor};
use egui::{Context, RichText, Window};

#[derive(Default)]
pub struct PublishPanel {
    pub open: bool,
    title: String,
    description: String,
    tags: String,
    cover_art: String,
    screenshots: String,
    version: String,
    note: String,
}

fn slug_from_title(title: &str) -> String {
    let slug: String = title
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() {
                character.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect();
    let mut result = String::new();
    for part in slug.split('-').filter(|part| !part.is_empty()) {
        if !result.is_empty() {
            result.push('-');
        }
        result.push_str(part);
    }
    result.chars().take(64).collect()
}

pub fn window(ctx: &Context, ed: &mut Editor, panel: &mut PublishPanel, rex: &RexPanel) {
    if !panel.open {
        return;
    }
    Window::new(RichText::new("Publish to Kit Store").strong())
        .title_bar(false)
        .default_size([620.0, 620.0])
        .show(ctx, |ui| {
            ui.label("Game listing details and release submission");
            ui.add_space(8.0);
            if matches!(rex.auth, crate::rex_panel::Auth::SignedIn) {
                ui.colored_label(egui::Color32::from_rgb(90, 190, 120), "REX developer account signed in");
            } else {
                ui.colored_label(egui::Color32::from_rgb(220, 150, 80), "Sign in through the REX panel before publishing.");
            }
            ui.separator();
            ui.horizontal(|ui| {
                ui.label("Title");
                ui.text_edit_singleline(&mut panel.title);
            });
            let slug = slug_from_title(&panel.title);
            if !slug.is_empty() {
                ui.small(format!("Store slug: {slug}"));
            }
            ui.label("Description");
            ui.add(egui::TextEdit::multiline(&mut panel.description).desired_rows(4).desired_width(f32::INFINITY));
            ui.horizontal(|ui| {
                ui.label("Tags");
                ui.text_edit_singleline(&mut panel.tags);
            });
            ui.horizontal(|ui| {
                ui.label("Cover art");
                ui.add(egui::TextEdit::singleline(&mut panel.cover_art).desired_width(360.0));
                if ui.button("Choose…").clicked() {
                    if let Some(path) = rfd::FileDialog::new()
                        .add_filter("Images", &["png", "jpg", "jpeg", "webp"])
                        .pick_file()
                    {
                        panel.cover_art = path.display().to_string();
                    }
                }
            });
            ui.label("Screenshots (one image path per line)");
            ui.add(egui::TextEdit::multiline(&mut panel.screenshots).desired_rows(3).desired_width(f32::INFINITY));
            if ui.button("Add screenshot…").clicked() {
                if let Some(path) = rfd::FileDialog::new()
                    .add_filter("Images", &["png", "jpg", "jpeg", "webp"])
                    .pick_file()
                {
                    if !panel.screenshots.is_empty() {
                        panel.screenshots.push('\n');
                    }
                    panel.screenshots.push_str(&path.display().to_string());
                }
            }
            ui.separator();
            ui.horizontal(|ui| {
                ui.label("Version");
                ui.text_edit_singleline(&mut panel.version);
            });
            ui.label("Release notes");
            ui.text_edit_multiline(&mut panel.note);
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                if ui.button("Open account Circuits").clicked() {
                    export::open_url("https://login.kitgameengine.win/account/circuits");
                }
                if ui.button("Export build").clicked() {
                    ed.notify("Choose the target and export folder in Project → Export; these listing details are ready for submission.");
                }
            });
            ui.small("Every submitted release becomes an immutable revision and stays private until an administrator approves it.");
        });
}
