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
    let mut open = panel.open;
    Window::new(RichText::new("Publish to Kit Store").strong())
        .open(&mut open)
        .default_size([540.0, 420.0])
        .show(ctx, |ui| {
            ui.label("Manage your games in the Kit account's Circuits page.");
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
                if ui.button("Export Linux build").clicked() {
                    ed.notify("Use Project → Export to create the Linux build, then upload it from the Store dashboard.");
                }
            });
            ui.small("Every submitted release becomes an immutable revision and stays private until an administrator approves it.");
        });
    panel.open = open;
}
