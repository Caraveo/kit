//! Project Settings window: edits `kit.project` (title, main scene, window, physics).

use crate::controls::{BindingEditor, Controls};
use crate::state::Editor;
use crate::theme::*;
use egui::{CornerRadius, Frame, Id, Margin, RichText, Stroke};
use kit::resource::ProjectConfig;

/// Draft of the settings being edited (None = window closed).
#[derive(Default)]
pub struct SettingsWindow {
    pub draft: Option<ProjectConfig>,
    controls: Controls,
    bindings: BindingEditor,
    controls_tab: bool,
}

impl SettingsWindow {
    pub fn capturing(&self) -> bool {
        self.draft.is_some() && self.controls_tab && self.bindings.capture.is_some()
    }

    pub fn open(&mut self, ed: &mut Editor) {
        match &ed.project {
            Some(p) => {
                self.draft = Some(p.config.clone());
                self.controls = ed.controls.clone();
                self.bindings = BindingEditor::default();
                self.controls_tab = false;
            }
            None => ed.notify("Open or create a project first (Project menu)"),
        }
    }
}

fn row(ui: &mut egui::Ui, label: &str, add: impl FnOnce(&mut egui::Ui)) {
    ui.horizontal(|ui| {
        ui.add_sized([110.0, 22.0], egui::Label::new(RichText::new(label).size(12.0).color(LABEL())));
        add(ui);
    });
}

/// Apply a config: save `kit.project` and update the running physics.
pub fn apply(ed: &mut Editor, config: ProjectConfig) -> anyhow::Result<()> {
    let Some(project) = ed.project.as_mut() else { anyhow::bail!("no project open") };
    config.save(&project.root().join(kit::resource::PROJECT_FILE))?;
    project.config = config.clone();
    if !config.rex.enabled {
        ed.settings.rex = false;
    }
    let p = &config.physics;
    kit::physics::apply_settings(
        &mut ed.app.world,
        kit::physics::PhysicsSettings {
            enabled: p.enabled,
            gravity: kit::math::Vec3::from_array(p.gravity),
            rate: p.rate,
        },
    );
    ed.notify("Project settings saved");
    Ok(())
}

pub fn window(ctx: &egui::Context, ed: &mut Editor, win: &mut SettingsWindow) {
    let Some(draft) = win.draft.as_mut() else { return };
    let mut open = true;
    let mut save = false;
    let mut close = false;
    let scenes = ed.assets_with(&["kscene"]);
    egui::Window::new(RichText::new("Project Settings").strong())
        .id(Id::new("project_settings"))
        .open(&mut open)
        .default_width(440.0)
        .resizable(false)
        .movable(false)
        .collapsible(false)
        .frame(
            Frame::new()
                .fill(DIALOG_BG())
                .stroke(Stroke::new(1.0, DIALOG_BORDER()))
                .corner_radius(CornerRadius::same(15))
                .inner_margin(Margin::same(18)),
        )
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.selectable_value(&mut win.controls_tab, false, "Project");
                ui.selectable_value(&mut win.controls_tab, true, "Editor Controls");
            });
            ui.separator();
            if win.controls_tab {
                win.bindings.show(ui, &mut win.controls);
            } else {
                crate::widgets::section_label(ui, "PROJECT");
                row(ui, "Title", |ui| {
                    ui.text_edit_singleline(&mut draft.project.title);
                });
                row(ui, "Version", |ui| {
                    ui.text_edit_singleline(&mut draft.project.version);
                });
                row(ui, "Main scene", |ui| {
                    egui::ComboBox::from_id_salt("main_scene").selected_text(draft.project.main_scene.clone()).show_ui(
                        ui,
                        |ui| {
                            for s in &scenes {
                                ui.selectable_value(&mut draft.project.main_scene, s.clone(), s.as_str());
                            }
                        },
                    );
                });
                ui.add_space(8.0);
                crate::widgets::section_label(ui, "WINDOW (PLAYER)");
                row(ui, "Size", |ui| {
                    ui.add(egui::DragValue::new(&mut draft.display.width).range(320..=7680).suffix(" px"));
                    ui.label("×");
                    ui.add(egui::DragValue::new(&mut draft.display.height).range(240..=4320).suffix(" px"));
                });
                row(ui, "", |ui| {
                    ui.checkbox(&mut draft.display.vsync, "VSync");
                    ui.checkbox(&mut draft.display.fullscreen, "Fullscreen");
                });
                ui.add_space(8.0);
                crate::widgets::section_label(ui, "PHYSICS");
                row(ui, "Simulation", |ui| {
                    ui.checkbox(&mut draft.physics.enabled, "Simulate rigid bodies")
                        .on_hover_text("Off: colliders still report touches and triggers, nothing falls");
                });
                row(ui, "Gravity", |ui| {
                    for (i, axis) in ["X ", "Y ", "Z "].iter().enumerate() {
                        ui.add(
                            egui::DragValue::new(&mut draft.physics.gravity[i])
                                .speed(0.05)
                                .max_decimals(2)
                                .prefix(*axis),
                        );
                    }
                });
                row(ui, "", |ui| {
                    if ui.small_button("Earth").clicked() {
                        draft.physics.gravity = [0.0, -9.81, 0.0];
                    }
                    if ui.small_button("Moon").clicked() {
                        draft.physics.gravity = [0.0, -1.62, 0.0];
                    }
                    if ui.small_button("Zero-G").clicked() {
                        draft.physics.gravity = [0.0, 0.0, 0.0];
                    }
                });
                row(ui, "Step rate", |ui| {
                    ui.add(egui::Slider::new(&mut draft.physics.rate, 30..=240).suffix(" Hz"));
                });
                ui.label(
                    RichText::new("Physics changes apply immediately, even while playing.").size(11.0).color(MUTED()),
                );
                ui.separator();
                ui.checkbox(&mut draft.rex.enabled, "Rex");
                if draft.rex.enabled {
                    ui.checkbox(&mut draft.rex.friendly, "Friendly Rex")
                        .on_hover_text("Use puppy artwork, emoji, and playful labels");
                }
            }
            ui.add_space(10.0);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if crate::widgets::primary(ui, "Save", None).clicked() {
                    save = true;
                }
                if crate::widgets::ghost(ui, "Cancel").clicked() {
                    close = true;
                }
            });
        });
    if save {
        let controls_changed = win.controls != ed.controls;
        let config_changed = ed.project.as_ref().is_some_and(|p| p.config != *draft);
        let result = (|| -> anyhow::Result<()> {
            if controls_changed {
                win.controls.save()?;
                ed.controls = win.controls.clone();
            }
            if config_changed {
                apply(ed, draft.clone())?;
            } else if controls_changed {
                ed.notify("Editor controls saved");
            }
            Ok(())
        })();
        close = result.is_ok();
        ed.report(result);
    }
    if close || !open {
        win.draft = None;
    }
}
