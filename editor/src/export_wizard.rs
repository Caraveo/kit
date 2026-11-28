//! The Export wizard: pick a platform, choose where to put the build, export,
//! then open the folder or run the game.

use crate::export::{Availability, Group, Platform, platforms};
use crate::state::Editor;
use crate::theme::*;
use egui::{Color32, CornerRadius, Frame, Id, Margin, RichText, Sense, Stroke, Vec2};
use std::path::PathBuf;

#[derive(Default)]
pub struct ExportWizard {
    pub open: bool,
    pub platform: Option<&'static str>,
    /// Output folder chosen by the user (default: `<project>/export/<platform>`).
    pub folder: Option<PathBuf>,
    /// Availability per platform, computed when the wizard opens
    /// (checking a player reads the whole binary, so not every frame).
    status: Vec<(&'static str, Availability)>,
    /// The Protection module: how the export is protected.
    pub protection: crate::export::Protection,
    /// The publisher key's fingerprint, or why it can't be used.
    pub key_status: Option<std::result::Result<String, String>>,
}

impl ExportWizard {
    /// Re-read the publisher key (after generating or importing one).
    pub fn refresh_key(&mut self) {
        self.key_status = Some(crate::export::load_key(None).map(|k| k.fingerprint()).map_err(|e| format!("{e:#}")));
    }
}

impl ExportWizard {
    pub fn show(&mut self, ed: &mut Editor) {
        if ed.project.is_none() {
            ed.notify("Open or create a project first (Project menu)");
            return;
        }
        self.open = true;
        self.refresh();
        self.refresh_key();
        if self.platform.is_none() {
            self.platform = Some(crate::export::host());
        }
        ed.last_export = None;
    }

    pub fn refresh(&mut self) {
        let bin_dir =
            std::env::current_exe().ok().and_then(|e| e.parent().map(|p| p.to_path_buf())).unwrap_or_default();
        self.status = platforms().iter().map(|p| (p.id, crate::export::availability(p, &bin_dir))).collect();
    }

    fn availability(&self, id: &str) -> Option<&Availability> {
        self.status.iter().find(|(p, _)| *p == id).map(|(_, a)| a)
    }

    fn selected(&self) -> Option<Platform> {
        self.platform.and_then(crate::export::platform)
    }

    /// Where the selected platform's build will be written.
    pub fn destination(&self, ed: &Editor) -> Option<PathBuf> {
        let platform = self.selected()?;
        let title = &ed.project.as_ref()?.config.project.title;
        self.folder.clone().or_else(|| Some(crate::export::default_builds_dir(title).join(platform.id)))
    }
}

/// Short status pill for a card: (text, color).
fn pill(a: Option<&Availability>) -> (&'static str, Color32) {
    match a {
        Some(Availability::Ready { .. }) => ("Ready", AXIS_Y()),
        Some(Availability::Rebuild { .. }) => ("Builds player", ACCENT()),
        Some(Availability::NeedsPlayer(_)) => ("Needs player", WARN()),
        Some(Availability::RequestAccess) => ("Request access", ACCENT()),
        Some(Availability::Hook(_)) => ("Ready (hook)", AXIS_Y()),
        Some(Availability::Unavailable(why)) if why.starts_with("Coming later") => ("Coming later", MUTED()),
        _ => ("Guide", MUTED()),
    }
}

fn card(ui: &mut egui::Ui, platform: &Platform, a: Option<&Availability>, selected: bool) -> egui::Response {
    let size = Vec2::new(104.0, 84.0);
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
    crate::ui::mark_rect(ui, platform.id, rect);
    let enabled = a.is_some_and(Availability::can_export);
    let painter = ui.painter_at(rect.expand(2.0));
    let fill = if selected {
        ACCENT().gamma_multiply(0.16)
    } else if resp.hovered() {
        FIELD_BG()
    } else {
        CARD()
    };
    let border = if selected { ACCENT() } else { CARD_BORDER() };
    painter.rect(
        rect,
        CornerRadius::same(12),
        fill,
        Stroke::new(if selected { 1.6 } else { 1.0 }, border),
        egui::StrokeKind::Inside,
    );
    let fade = if enabled { 1.0 } else { 0.55 };
    painter.text(
        rect.center_top() + Vec2::new(0.0, 23.0),
        egui::Align2::CENTER_CENTER,
        platform.icon,
        egui::FontId::proportional(24.0),
        TEXT_STRONG().gamma_multiply(fade),
    );
    painter.text(
        rect.center_top() + Vec2::new(0.0, 49.0),
        egui::Align2::CENTER_CENTER,
        platform.name,
        egui::FontId::proportional(12.0),
        TEXT().gamma_multiply(fade),
    );
    let (text, color) = pill(a);
    let galley = painter.layout_no_wrap(text.to_owned(), egui::FontId::proportional(10.0), color);
    let pill_rect =
        egui::Rect::from_center_size(rect.center_top() + Vec2::new(0.0, 69.0), galley.size() + Vec2::new(12.0, 4.0));
    painter.rect_filled(pill_rect, CornerRadius::same(8), color.gamma_multiply(0.15));
    painter.galley(pill_rect.min + Vec2::new(6.0, 2.0), galley, color);
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// What the wizard asks the host to do (OS dialogs).
#[derive(Default)]
pub struct WizardRequests {
    pub pick_folder: bool,
    pub import_key: bool,
}

/// Draw the wizard.
pub fn window(ctx: &egui::Context, ed: &mut Editor, wiz: &mut ExportWizard) -> WizardRequests {
    if !wiz.open {
        return WizardRequests::default();
    }
    let mut open = true;
    let mut pick_folder = false;
    let mut import_key = false;
    let title = ed.project.as_ref().map(|p| p.config.project.title.clone()).unwrap_or_default();
    egui::Window::new(RichText::new("Export").strong())
        .id(Id::new("export_wizard"))
        .open(&mut open)
        .default_width(560.0)
        .resizable(false)
        .collapsible(false)
        .anchor(egui::Align2::CENTER_CENTER, Vec2::ZERO)
        .frame(
            Frame::new()
                .fill(DIALOG_BG())
                .stroke(Stroke::new(1.0, DIALOG_BORDER()))
                .corner_radius(CornerRadius::same(15))
                .inner_margin(Margin::same(20)),
        )
        .show(ctx, |ui| {
            ui.set_width(560.0);
            ui.label(RichText::new(format!("Ship “{title}”")).size(18.0).strong().color(TEXT_STRONG()));
            ui.label(
                RichText::new("Pick a platform. Kit writes a ready-to-run copy of your game, with every scene, model and script.")
                    .size(12.0)
                    .color(MUTED()),
            );
            ui.add_space(10.0);

            // 1. Platform grid, grouped.
            {
                let rows: [(&str, &[Group]); 3] = [
                    ("DESKTOP", &[Group::Desktop]),
                    ("WEB & MOBILE", &[Group::Web, Group::Mobile]),
                    ("CONSOLES", &[Group::Console]),
                ];
                for (label, groups) in rows {
                    crate::widgets::section_label(ui, label);
                    ui.horizontal_wrapped(|ui| {
                        ui.spacing_mut().item_spacing = Vec2::splat(10.0);
                        for p in platforms().iter().filter(|p| groups.contains(&p.group)) {
                            let selected = wiz.platform == Some(p.id);
                            if card(ui, p, wiz.availability(p.id), selected).clicked() && !ed.exporting() {
                                if wiz.platform != Some(p.id) {
                                    wiz.folder = None;
                                    ed.last_export = None;
                                }
                                wiz.platform = Some(p.id);
                            }
                        }
                    });
                    ui.add_space(4.0);
                }
            }

            ui.add_space(6.0);
            ui.separator();
            ui.add_space(6.0);

            // 2. Details for the selected platform.
            let Some(platform) = wiz.selected() else { return };
            let availability = wiz.availability(platform.id).cloned();
            ui.horizontal(|ui| {
                ui.label(RichText::new(platform.icon).size(20.0));
                ui.label(RichText::new(platform.name).size(15.0).strong().color(TEXT_STRONG()));
                let (text, color) = pill(availability.as_ref());
                ui.label(RichText::new(text).size(11.0).color(color));
            });
            let can_export = availability.as_ref().is_some_and(Availability::can_export);
            match &availability {
                Some(Availability::Ready { player }) => {
                    let host = platform.id == crate::export::host() || platform.id == "web";
                    let what = match platform.id {
                        "mac" => "a .app bundle",
                        "web" => "a web page (index.html + game files) for any web host or itch.io. Players need a browser with WebGPU",
                        _ => "a folder with the game and its executable",
                    };
                    ui.label(
                        RichText::new(format!(
                            "Writes {what}.{}",
                            if host { "" } else { " Copy it to a computer running this system to play." }
                        ))
                        .size(12.0)
                        .color(LABEL()),
                    );
                    ui.label(RichText::new(format!("Player: {}", player.display())).size(10.5).color(MUTED()));
                }
                Some(Availability::Rebuild { .. }) => {
                    ui.label(
                        RichText::new(
                            "Kit is running from source and this platform's player is out of date, so the export \
                             builds it first (this can take a minute).",
                        )
                        .size(12.0)
                        .color(LABEL()),
                    );
                }
                Some(Availability::NeedsPlayer(why) | Availability::Unavailable(why)) => {
                    ui.label(RichText::new(why).size(12.0).color(LABEL()));
                    requirements(ui, &platform);
                }
                Some(Availability::RequestAccess) => {
                    ui.label(RichText::new(platform.summary).size(12.0).color(LABEL()));
                    requirements(ui, &platform);
                    ui.add_space(6.0);
                    ui.horizontal(|ui| {
                        if let Some(url) = platform.request_url {
                            let label = format!("{} ↗", platform.request_label.unwrap_or("Request access"));
                            let r = crate::widgets::primary(ui, &label, None);
                            crate::ui::mark_rect(ui, "request_access", r.rect);
                            if r.clicked() {
                                crate::export::open_url(url);
                            }
                        }
                    });
                    ui.label(
                        RichText::new(format!(
                            "Already approved? Install the {} hook from the platform's developer portal into hooks/{}/ next to Kit, and this card turns Ready.",
                            platform.name, platform.id
                        ))
                        .size(11.0)
                        .color(MUTED()),
                    );
                }
                Some(Availability::Hook(external)) => {
                    ui.label(
                        RichText::new(format!("Exported by the installed {} hook ({}).", platform.name, external.dir.display()))
                            .size(12.0)
                            .color(LABEL()),
                    );
                }
                None => {}
            }

            if can_export {
                ui.add_space(8.0);
                let dest = wiz.destination(ed).unwrap_or_default();
                ui.horizontal(|ui| {
                    ui.add_sized([86.0, 22.0], egui::Label::new(RichText::new("Output folder").size(12.0).color(LABEL())));
                    let shown = dest.display().to_string();
                    ui.add(
                        egui::Label::new(RichText::new(shown.clone()).size(12.0).monospace().color(TEXT()))
                            .truncate(),
                    )
                    .on_hover_text(shown);
                });
                ui.horizontal(|ui| {
                    ui.add_space(90.0);
                    if ui.add_enabled(!ed.exporting(), egui::Button::new("Choose…")).clicked() {
                        pick_folder = true;
                    }
                    if wiz.folder.is_some() && ui.add_enabled(!ed.exporting(), egui::Button::new("Use default")).clicked() {
                        wiz.folder = None;
                    }
                });
                protection_module(ui, ed, wiz, &platform, &mut import_key);
            }

            ui.add_space(12.0);
            // 3. Export / progress / result.
            ui.horizontal(|ui| {
                if ed.exporting() {
                    ui.spinner();
                    let text = if matches!(availability, Some(Availability::Rebuild { .. })) {
                        "Building the game player, then copying your game…"
                    } else {
                        "Copying your game…"
                    };
                    ui.label(RichText::new(text).size(12.0).color(LABEL()));
                    return;
                }
                let export = ui.add_enabled_ui(can_export, |ui| crate::widgets::primary(ui, &format!("Export for {}", platform.name), None)).inner;
                crate::ui::mark_rect(ui, "export_go", export.rect);
                if export.clicked() {
                    if let Some(dest) = wiz.destination(ed) {
                        let result = ed.start_export(&platform, &dest, wiz.protection);
                        ed.report(result);
                    }
                }
            });
            match ed.last_export.clone() {
                Some(Ok((out, done))) if done == platform => {
                    ui.add_space(10.0);
                    Frame::new()
                        .fill(AXIS_Y().gamma_multiply(0.12))
                        .stroke(Stroke::new(1.0, AXIS_Y()))
                        .corner_radius(CornerRadius::same(10))
                        .inner_margin(Margin::same(10))
                        .show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            ui.label(RichText::new("✔ Exported").strong().color(TEXT_STRONG()));
                            ui.label(RichText::new(out.display().to_string()).size(11.5).monospace().color(LABEL()));
                            ui.horizontal(|ui| {
                                if ui.button("📂 Open folder").clicked() {
                                    crate::export::reveal(out.parent().unwrap_or(&out));
                                }
                                let label = if platform.id == "web" { "▶ Play in browser" } else { "▶ Run game" };
                                let runnable = platform.id == crate::export::host() || platform.id == "web";
                                if runnable && ui.button(label).clicked() {
                                    let result = crate::export::launch(&out, &platform);
                                    ed.report(result);
                                }
                            });
                        });
                }
                Some(Err(e)) => {
                    ui.add_space(10.0);
                    ui.label(RichText::new(format!("Export failed: {e}")).size(12.0).color(ERROR()));
                }
                _ => {}
            }
        });
    if !open && !ed.exporting() {
        wiz.open = false;
    }
    WizardRequests { pick_folder, import_key }
}

/// A platform's requirements, as a short list.
fn requirements(ui: &mut egui::Ui, platform: &Platform) {
    if platform.requirements.is_empty() {
        return;
    }
    ui.add_space(4.0);
    for r in platform.requirements {
        ui.label(RichText::new(format!("•  {r}")).size(11.5).color(MUTED()));
    }
}

/// The Protection module: Standard, Signed, or Signed + Encrypted, and the
/// publisher key.
fn protection_module(
    ui: &mut egui::Ui,
    ed: &mut Editor,
    wiz: &mut ExportWizard,
    platform: &Platform,
    import_key: &mut bool,
) {
    use crate::export::{Protection, protection_supported};
    ui.add_space(10.0);
    crate::widgets::section_label(ui, "PROTECTION");
    if protection_supported(platform, wiz.protection).is_err() {
        wiz.protection = Protection::Standard;
    }
    ui.horizontal(|ui| {
        for mode in Protection::ALL {
            let supported = protection_supported(platform, mode);
            let r = ui.add_enabled(
                supported.is_ok() && !ed.exporting(),
                egui::Button::selectable(wiz.protection == mode, mode.label()),
            );
            crate::ui::mark_rect(
                ui,
                match mode {
                    Protection::Standard => "protect_standard",
                    Protection::Signed => "protect_signed",
                    Protection::Encrypted => "protect_encrypted",
                },
                r.rect,
            );
            let r = match &supported {
                Err(why) => r.on_disabled_hover_text(why.as_str()),
                Ok(()) => r,
            };
            if r.clicked() {
                wiz.protection = mode;
            }
        }
    });
    let explain = match wiz.protection {
        Protection::Standard => "Plain game files, as before: anyone can open, read or change them.",
        Protection::Signed => {
            "Your game checks a signature before it loads anything. If the game data is changed, swapped for someone \
             else's, or missing, it refuses to run. The files can still be read."
        }
        Protection::Encrypted => {
            "Signed, and the game data is encrypted so its scripts and assets can't be browsed or copied casually. \
             The game must decrypt it to play, so a determined person inspecting the running game can still recover \
             it. Desktop only."
        }
    };
    ui.label(RichText::new(explain).size(11.5).color(MUTED()));
    if wiz.protection == Protection::Standard {
        return;
    }
    ui.add_space(4.0);
    ui.horizontal(|ui| {
        ui.add_sized([86.0, 22.0], egui::Label::new(RichText::new("Publisher key").size(12.0).color(LABEL())));
        match &wiz.key_status {
            Some(Ok(fingerprint)) => {
                ui.label(RichText::new(fingerprint).monospace().size(12.0).color(TEXT()))
                    .on_hover_text("Your public fingerprint. Builds you publish only trust this key.");
            }
            Some(Err(_)) | None => {
                ui.label(RichText::new("None yet").size(12.0).color(WARN()));
            }
        }
    });
    ui.horizontal(|ui| {
        ui.add_space(90.0);
        let has_key = matches!(wiz.key_status, Some(Ok(_)));
        if !has_key {
            let r = ui.add_enabled(!ed.exporting(), egui::Button::new("Generate key"));
            crate::ui::mark_rect(ui, "generate_key", r.rect);
            if r.clicked() {
                let result = crate::export::default_key_path()
                    .ok_or_else(|| anyhow::anyhow!("no settings folder for the publisher key"))
                    .and_then(|path| {
                        kit_export_security::PublisherKey::generate().and_then(|k| k.save(&path).map(|()| (k, path)))
                    });
                match result {
                    Ok((k, path)) => ed.notify(format!(
                        "Created your publisher key ({}). Back up {}: builds you publish only trust this key.",
                        k.fingerprint(),
                        path.display()
                    )),
                    Err(e) => ed.report(Err(e)),
                }
                wiz.refresh_key();
            }
            if ui.add_enabled(!ed.exporting(), egui::Button::new("Import key file…")).clicked() {
                *import_key = true;
            }
        } else if let Some(path) = crate::export::default_key_path() {
            if ui.button("Show key file").on_hover_text(path.display().to_string()).clicked() {
                crate::export::reveal(path.parent().unwrap_or(&path));
            }
        }
    });
    if let Some(Err(why)) = &wiz.key_status {
        if !why.starts_with("No publisher key yet") {
            ui.label(RichText::new(why).size(11.0).color(ERROR()));
        }
    }
    ui.label(
        RichText::new("The private key stays in your Kit settings folder: never in the project or the export.")
            .size(11.0)
            .color(MUTED()),
    );
}
