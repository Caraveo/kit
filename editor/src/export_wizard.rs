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
}

impl ExportWizard {
    pub fn show(&mut self, ed: &mut Editor) {
        if ed.project.is_none() {
            ed.notify("Open or create a project first (Project menu)");
            return;
        }
        self.open = true;
        self.refresh();
        if self.platform.is_none() {
            self.platform = Some(crate::export::host());
        }
        ed.last_export = None;
    }

    pub fn refresh(&mut self) {
        let bin_dir = std::env::current_exe().ok().and_then(|e| e.parent().map(|p| p.to_path_buf())).unwrap_or_default();
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
        self.folder.clone().or_else(|| Some(ed.project.as_ref()?.root().join("export").join(platform.id)))
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
    painter.rect(rect, CornerRadius::same(12), fill, Stroke::new(if selected { 1.6 } else { 1.0 }, border), egui::StrokeKind::Inside);
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
    let pill_rect = egui::Rect::from_center_size(rect.center_top() + Vec2::new(0.0, 69.0), galley.size() + Vec2::new(12.0, 4.0));
    painter.rect_filled(pill_rect, CornerRadius::same(8), color.gamma_multiply(0.15));
    painter.galley(pill_rect.min + Vec2::new(6.0, 2.0), galley, color);
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// Draw the wizard. Returns true when the user asked to pick an output folder.
pub fn window(ctx: &egui::Context, ed: &mut Editor, wiz: &mut ExportWizard) -> bool {
    if !wiz.open {
        return false;
    }
    let mut open = true;
    let mut pick_folder = false;
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
                .inner_margin(Margin::same(20))
                .shadow(egui::Shadow { offset: [0, 18], blur: 60, spread: 0, color: Color32::from_black_alpha(120) }),
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
                        let result = ed.start_export(&platform, &dest);
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
    pick_folder
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
