//! New Project wizard and its editable draft.

use crate::templates::{NewProject, Template};
use egui::{Id, RichText};
use std::path::{Path, PathBuf};

pub struct Wizard {
    pub options: NewProject,
    pub location: String,
    pub folder: String,
    track_title: bool,
    suggestion: usize,
    pub error: Option<String>,
}

impl Wizard {
    pub fn new(location: &Path) -> Self {
        let options = NewProject::default();
        let folder = suggested_folder(location, &options.title);
        Self {
            options,
            location: location.to_string_lossy().into_owned(),
            folder,
            track_title: true,
            suggestion: 0,
            error: None,
        }
    }

    pub fn select_location(&mut self, path: &Path) {
        self.location = path.to_string_lossy().into_owned();
        self.update_suggestion();
    }

    fn update_suggestion(&mut self) {
        if self.track_title {
            self.folder = suggested_folder(Path::new(self.location.trim()), &self.options.title);
        }
    }

    fn destination(&self) -> anyhow::Result<PathBuf> {
        use anyhow::ensure;
        let folder = self.folder.trim();
        ensure!(!self.options.title.trim().is_empty(), "Enter a project name.");
        ensure!(!self.location.trim().is_empty(), "Choose a parent folder.");
        ensure!(
            !folder.is_empty() && folder != "." && folder != ".." && folder == folder_name(folder),
            "Use a single folder name without slashes, reserved names, or special characters."
        );
        let parent = std::path::absolute(self.location.trim())?;
        ensure!(!parent.exists() || parent.is_dir(), "The parent location is a file.");
        let path = parent.join(folder);
        if path.exists() {
            ensure!(path.is_dir() && std::fs::read_dir(&path)?.next().is_none(), "The project folder must be empty.");
        }
        Ok(path)
    }
}

fn folder_name(title: &str) -> String {
    let name: String =
        title.trim().chars().map(|c| if c.is_control() || "<>:\"/\\|?*".contains(c) { '-' } else { c }).collect();
    let name = name.trim_end_matches(['.', ' ']);
    let stem = name.split('.').next().unwrap_or_default().to_ascii_uppercase();
    let reserved = matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || ["COM", "LPT"].iter().any(|prefix| {
            stem.strip_prefix(prefix)
                .is_some_and(|suffix| suffix.len() == 1 && ('1'..='9').contains(&suffix.chars().next().unwrap()))
        });
    if name.is_empty() || reserved { "Kit Game".into() } else { name.into() }
}

fn suggested_folder(parent: &Path, title: &str) -> String {
    let stem = folder_name(title);
    (1..)
        .map(|n| if n == 1 { stem.clone() } else { format!("{stem} {n}") })
        .find(|candidate| !parent.join(candidate).exists())
        .unwrap()
}

pub enum Action {
    Browse,
    Create(PathBuf, NewProject),
    Close,
}

pub fn window(ctx: &egui::Context, wizard: &mut Wizard) -> Option<Action> {
    let mut action = None;
    let modal = egui::Modal::new(Id::new("new_project_wizard")).show(ctx, |ui| {
        ui.set_width(ui.ctx().content_rect().width().min(590.0) - 28.0);
        ui.heading(RichText::new("New Project").strong());
        ui.label("Choose a template and where Kit should create the project.");
        ui.add_space(10.0);
        egui::ScrollArea::vertical().max_height((ctx.content_rect().height() - 230.0).max(100.0)).show(ui, |ui| {
            ui.group(|ui| {
                ui.label(RichText::new("Template").strong());
                ui.horizontal_wrapped(|ui| {
                    for template in Template::ALL {
                        if ui
                            .selectable_label(
                                wizard.options.template == template,
                                format!("{} {}", template.icon(), template.name()),
                            )
                            .clicked()
                        {
                            wizard.options.template = template;
                            wizard.suggestion = 0;
                        }
                    }
                });
                ui.label(wizard.options.template.blurb());
            });
            ui.add_space(8.0);
            egui::Grid::new("new_project_fields").num_columns(2).spacing([12.0, 8.0]).show(ui, |ui| {
                ui.label("Project name");
                ui.horizontal(|ui| {
                    if ui.text_edit_singleline(&mut wizard.options.title).changed() {
                        wizard.update_suggestion();
                    }
                    if ui.button("Suggest name").clicked() {
                        let names = wizard.options.template.suggestions();
                        wizard.options.title = names[wizard.suggestion % names.len()].into();
                        wizard.suggestion = wizard.suggestion.wrapping_add(1);
                        wizard.update_suggestion();
                    }
                });
                ui.end_row();
                ui.label("Parent folder");
                ui.horizontal(|ui| {
                    if ui.text_edit_singleline(&mut wizard.location).changed() {
                        wizard.update_suggestion();
                    }
                    if ui.button("Browse…").clicked() {
                        action = Some(Action::Browse);
                    }
                });
                ui.end_row();
                ui.label("Project folder name");
                ui.horizontal(|ui| {
                    if ui.text_edit_singleline(&mut wizard.folder).changed() {
                        wizard.track_title = false;
                    }
                    if ui.button("Suggest folder").clicked() {
                        wizard.track_title = true;
                        wizard.update_suggestion();
                    }
                });
                ui.end_row();
                ui.label("Game window");
                ui.horizontal(|ui| {
                    ui.add(egui::DragValue::new(&mut wizard.options.width).range(320..=7680).suffix(" px"));
                    ui.label("×");
                    ui.add(egui::DragValue::new(&mut wizard.options.height).range(240..=4320).suffix(" px"));
                });
                ui.end_row();
                ui.label("Physics");
                ui.checkbox(&mut wizard.options.physics, "Simulate rigid bodies");
                ui.end_row();
            });
        });
        let destination = wizard.destination();
        match &destination {
            Ok(path) => {
                ui.label(format!("Creates {}", path.display()));
            }
            Err(error) => {
                ui.colored_label(crate::theme::ERROR(), error.to_string());
            }
        }
        if let Some(error) = &wizard.error {
            ui.colored_label(crate::theme::ERROR(), error);
        }
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            let button = ui.add_enabled(destination.is_ok(), egui::Button::new("Create Project"));
            crate::ui::mark_rect(ui, "create_project", button.rect);
            if button.clicked()
                && let Ok(path) = destination
            {
                action = Some(Action::Create(path, wizard.options.clone()));
            }
            if ui.button("Cancel").clicked() {
                action = Some(Action::Close);
            }
        });
    });
    if modal.should_close() {
        return Some(Action::Close);
    }
    action
}
