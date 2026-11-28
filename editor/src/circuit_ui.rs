//! What installed Circuits add to the editor: their components (under
//! Circuits in "+ Add component", with an inspector section each) and their
//! settings (a Project menu item per Circuit). Generic: driven entirely by
//! each Circuit's circuit.toml, nothing special-cased.
//!
//! Component values live on the entity in the scene file under the
//! component's id (Kit keeps unknown scene fields), so removing a Circuit
//! never damages a scene and reinstalling brings its components back.

use std::cell::RefCell;
use std::path::PathBuf;
use std::time::SystemTime;

use egui::{Color32, CornerRadius, Frame, Id, Margin, RichText, Stroke, Ui};
use kit::ecs::Entity;
use kit::gamesys::ExtraData;
use kit_circuit::manifest::{ConfigField, Manifest};
use serde_json::{Map, Value};

use crate::state::Editor;
use crate::theme::*;

/// Enabled Circuits of the open project (refreshed when it or its lockfile changes).
struct Active {
    root: Option<PathBuf>,
    lock_mtime: Option<SystemTime>,
    circuits: Vec<Manifest>,
}

thread_local! {
    // The UI runs on one thread; per-thread also keeps parallel tests apart.
    static ACTIVE: RefCell<Active> = const { RefCell::new(Active { root: None, lock_mtime: None, circuits: Vec::new() }) };
}

fn lock_mtime(root: &std::path::Path) -> Option<SystemTime> {
    std::fs::metadata(kit_circuit::lock::Lock::path(root)).and_then(|m| m.modified()).ok()
}

/// Keep the list of enabled Circuits current (cheap: one file stat per frame).
pub fn sync(ed: &Editor) {
    let root = ed.project.as_ref().map(|p| p.root().to_path_buf());
    let mtime = root.as_deref().and_then(lock_mtime);
    if ACTIVE.with_borrow(|a| a.root == root && a.lock_mtime == mtime) {
        return;
    }
    let circuits = root.as_deref().and_then(|r| kit_circuit::install::enabled(r).ok()).unwrap_or_default();
    ACTIVE.set(Active { root, lock_mtime: mtime, circuits });
}

pub(crate) fn active() -> Vec<Manifest> {
    ACTIVE.with_borrow(|a| a.circuits.clone())
}

pub(crate) fn is_enabled(id: &str) -> bool {
    ACTIVE.with_borrow(|a| a.circuits.iter().any(|circuit| circuit.circuit.id == id))
}

// ------------------------------------------------------------------ components

/// The "Circuits" part of "+ Add component".
pub fn add_component_section(ui: &mut Ui, ed: &mut Editor, e: Entity) {
    let circuits = active();
    if !circuits.iter().any(|m| !m.components.is_empty()) {
        return;
    }
    ui.separator();
    ui.label(RichText::new("CIRCUITS").size(10.5).color(LABEL()));
    let extra = ed.world().get_cloned::<ExtraData>(e).unwrap_or_default();
    for m in &circuits {
        for c in &m.components {
            let has = extra.0.contains_key(&c.id);
            let resp = ui.add_enabled(!has, egui::Button::new(format!("⚡ {}", c.name)));
            let resp = if c.description.is_empty() {
                resp
            } else {
                resp.on_hover_text(format!("{} (from {})", c.description, m.circuit.name))
            };
            if resp.clicked() {
                let mut x = extra.clone();
                x.0.insert(c.id.clone(), c.defaults());
                ed.app.world.insert(e, x);
                ui.close();
            }
        }
    }
}

/// Inspector sections for Circuit components on this entity.
pub fn inspector_sections(ui: &mut Ui, ed: &mut Editor, e: Entity) {
    let Some(mut extra) = ed.world().get_cloned::<ExtraData>(e) else { return };
    let circuits = active();
    let mut changed = false;
    for m in &circuits {
        for c in &m.components {
            let Some(Value::Object(values)) = extra.0.get_mut(&c.id) else { continue };
            let mut edit = false;
            let keep = crate::ui::section(ui, &format!("circuit_{}", c.id), &c.name, "⚡", true, |ui| {
                if !c.description.is_empty() {
                    ui.label(RichText::new(&c.description).size(11.0).color(MUTED()));
                }
                for f in &c.fields {
                    edit |= field(ui, (&c.id, &f.key), &f.key, &f.field, values);
                }
                ui.label(RichText::new(format!("From the {} Circuit", m.circuit.name)).size(10.5).color(MUTED()));
            });
            changed |= edit;
            if !keep {
                extra.0.remove(&c.id);
                changed = true;
            }
        }
    }
    if changed {
        ed.app.world.insert(e, extra);
    }
}

/// One field editor. Returns true when the value changed.
fn field(
    ui: &mut Ui,
    id: impl std::hash::Hash + std::fmt::Debug,
    key: &str,
    f: &ConfigField,
    values: &mut Map<String, Value>,
) -> bool {
    let label = if f.label.is_empty() { key.replace('_', " ") } else { f.label.clone() };
    let v = values.entry(key.to_owned()).or_insert_with(|| f.default_json());
    let mut changed = false;
    ui.horizontal(|ui| {
        let hint = |r: egui::Response| if f.description.is_empty() { r } else { r.on_hover_text(&f.description) };
        match f.kind.as_str() {
            "boolean" => {
                let mut b = v.as_bool().unwrap_or(false);
                if hint(ui.checkbox(&mut b, label.as_str())).changed() {
                    *v = Value::Bool(b);
                    changed = true;
                }
            }
            "integer" | "number" => {
                hint(ui.add_sized([110.0, 20.0], egui::Label::new(RichText::new(&label).size(12.0).color(LABEL()))));
                let mut n = v.as_f64().unwrap_or(0.0);
                let mut d = egui::DragValue::new(&mut n).speed(if f.kind == "integer" { 0.2 } else { 0.05 });
                d = d.range(f.min.unwrap_or(f64::MIN)..=f.max.unwrap_or(f64::MAX));
                if f.kind == "integer" {
                    d = d.fixed_decimals(0);
                }
                if ui.add(d).changed() {
                    *v = if f.kind == "integer" { Value::from(n.round() as i64) } else { Value::from(n) };
                    changed = true;
                }
            }
            "choice" => {
                hint(ui.add_sized([110.0, 20.0], egui::Label::new(RichText::new(&label).size(12.0).color(LABEL()))));
                let cur = v.as_str().unwrap_or_default().to_owned();
                egui::ComboBox::from_id_salt(Id::new(("circuit_field", &id))).selected_text(&cur).show_ui(ui, |ui| {
                    for o in &f.options {
                        if ui.selectable_label(*o == cur, o).clicked() {
                            *v = Value::String(o.clone());
                            changed = true;
                        }
                    }
                });
            }
            _ => {
                hint(ui.add_sized([110.0, 20.0], egui::Label::new(RichText::new(&label).size(12.0).color(LABEL()))));
                let mut s = v.as_str().unwrap_or_default().to_owned();
                let edit = egui::TextEdit::singleline(&mut s).desired_width(ui.available_width()).password(f.secret);
                if ui.add(edit).changed() {
                    *v = Value::String(s);
                    changed = true;
                }
            }
        }
    });
    changed
}

// ------------------------------------------------------------------ settings

/// The open Circuit settings window: Circuit id and the values being edited.
#[derive(Default)]
pub struct SettingsWindow {
    open: Option<(Manifest, Map<String, Value>)>,
    error: Option<String>,
}

impl SettingsWindow {
    pub fn is_open(&self) -> bool {
        self.open.is_some()
    }

    pub fn close(&mut self) {
        self.open = None;
        self.error = None;
    }
}

fn secrets(ed: &Editor) -> Option<PathBuf> {
    let root = ed.project.as_ref()?.root().to_path_buf();
    crate::export::config_dir().map(|d| kit_circuit::settings::secrets_path(&d, &root))
}

/// Project menu items: one per enabled Circuit that has settings or a dashboard.
pub fn project_menu(ui: &mut Ui, ed: &mut Editor, win: &mut SettingsWindow) {
    let circuits: Vec<Manifest> =
        active()
            .into_iter()
            .filter(|m| m.circuit.id != "kit.publish" && (!m.config.is_empty() || m.circuit.dashboard.is_some()))
            .collect();
    for m in circuits {
        if ui
            .button(format!("{}…", m.circuit.name))
            .on_hover_text(format!("{} settings for this project", m.circuit.name))
            .clicked()
        {
            let root = ed.project.as_ref().map(|p| p.root().to_path_buf());
            match root.map(|r| kit_circuit::settings::load(&r, secrets(ed).as_deref(), &m)) {
                Some(Ok(values)) => {
                    win.open = Some((m, values));
                    win.error = None;
                }
                Some(Err(e)) => ed.notify(format!("{e:#}")),
                None => {}
            }
            ui.close();
        }
    }
}

pub fn settings_window(ctx: &egui::Context, ed: &mut Editor, win: &mut SettingsWindow) {
    let Some((m, values)) = win.open.as_mut() else { return };
    let mut save = false;
    egui::Window::new(RichText::new(&m.circuit.name).strong())
        .id(Id::new(("circuit_settings", &m.circuit.id)))
        .default_width(440.0)
        .resizable(false)
        .collapsible(false)
        .frame(
            Frame::new()
                .fill(DIALOG_BG())
                .stroke(Stroke::new(1.0, DIALOG_BORDER()))
                .corner_radius(CornerRadius::same(15))
                .inner_margin(Margin::same(18))
                .shadow(egui::Shadow { offset: [0, 18], blur: 60, spread: 0, color: Color32::from_black_alpha(170) }),
        )
        .show(ctx, |ui| {
            ui.label(RichText::new(&m.circuit.description).size(12.0).color(MUTED()));
            ui.add_space(6.0);
            if !m.config.is_empty() {
                crate::widgets::section_label(ui, "THIS PROJECT");
                for (key, f) in &m.config {
                    field(ui, (&m.circuit.id, key), key, f, values);
                    if !f.description.is_empty() {
                        ui.label(RichText::new(&f.description).size(10.5).color(MUTED()));
                    }
                }
                ui.label(
                    RichText::new("Saved in circuits.settings.toml (secret fields stay on this computer).")
                        .size(10.5)
                        .color(MUTED()),
                );
            }
            if let Some(url) = &m.circuit.dashboard {
                ui.add_space(8.0);
                crate::widgets::section_label(ui, "HOSTED SERVICE");
                ui.label(RichText::new("Usage, keys, limits and live data are on your Kit account.").size(12.0));
                if ui.button("Open dashboard ↗").clicked() {
                    let _ = webbrowser::open(url);
                }
            }
            if let Some(e) = &win.error {
                ui.colored_label(Color32::from_rgb(255, 122, 138), e);
            }
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                if !m.config.is_empty() && crate::widgets::primary(ui, "Save", None).clicked() {
                    save = true;
                }
            });
        });
    if save {
        let root = ed.project.as_ref().map(|p| p.root().to_path_buf());
        let secrets = secrets(ed);
        if let (Some(root), Some((m, values))) = (root, win.open.as_ref()) {
            match kit_circuit::settings::save(&root, secrets.as_deref(), m, values) {
                Ok(()) => {
                    ed.notify(format!("{} settings saved", m.circuit.name));
                    win.open = None;
                    return;
                }
                Err(e) => win.error = Some(format!("{e:#}")),
            }
        }
    }
}

// ------------------------------------------------------------------ test players

/// Does an enabled Circuit network the game (so extra player windows make sense)?
pub fn networked() -> bool {
    active().iter().any(|m| m.circuit.permissions.iter().any(|p| p == "network.client"))
}

/// Open another game window on this project (another player), using its saved files.
pub fn launch_player(ed: &mut Editor) {
    let Some(root) = ed.project.as_ref().map(|p| p.root().to_path_buf()) else { return };
    let exe = std::env::current_exe()
        .ok()
        .and_then(|e| e.parent().map(|d| d.join(if cfg!(windows) { "kit-player.exe" } else { "kit-player" })));
    let Some(player) = exe.filter(|p| p.is_file()) else {
        ed.notify(
            "kit-player wasn't found next to the editor. Reinstall Kit (or build it with cargo build -p kit-player).",
        );
        return;
    };
    match std::process::Command::new(&player).arg(&root).spawn() {
        Ok(_) => ed.notify(if ed.scene_dirty() {
            "Opened another player window. It uses the saved scene, so save to include your latest changes."
        } else {
            "Opened another player window."
        }),
        Err(e) => ed.notify(format!("Couldn't start kit-player: {e}")),
    }
}
