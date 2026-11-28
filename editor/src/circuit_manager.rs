//! Project → Circuit Manager: find, install, update, downgrade, disable and
//! remove Circuits for the open project. It manages extensions only; hosted
//! services (like Multiplayer's) are managed on the Kit account website.
//!
//! Network work runs on a background thread; the window shows its progress.
//! Installs are atomic (see `kit_circuit::install`): a failed download,
//! checksum or signature leaves the project exactly as it was.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::mpsc::{Receiver, Sender, channel};

use egui::{Align, Color32, CornerRadius, Frame, Id, Layout, Margin, RichText, Stroke};
use kit_circuit::install::{self, Installed};
use kit_circuit::manifest::{PERMISSIONS, host_platform, kit_version};
use kit_circuit::registry::{ApiError, Circuit, Client, Query, Release, api_base};
use semver::Version;

use crate::state::Editor;
use crate::theme::*;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Tab {
    Available,
    Installed,
    Updates,
}

enum Load<T> {
    Idle,
    Loading,
    Ready(T),
    Failed(ApiError),
}

enum Msg {
    Listed(u64, Result<Vec<Circuit>, ApiError>),
    Releases(String, Result<Vec<Release>, ApiError>),
    Progress(String, f32),
    Finished(Result<String, String>),
}

pub struct CircuitManager {
    pub open: bool,
    tab: Tab,
    search: String,
    official_only: bool,
    list: Load<Vec<Circuit>>,
    list_generation: u64,
    searched: (String, bool),
    selected: Option<String>,
    releases: HashMap<String, Load<Vec<Release>>>,
    chosen: HashMap<String, String>,
    installed: Vec<Installed>,
    installed_error: Option<String>,
    job: Option<(String, f32)>,
    result: Option<Result<String, String>>,
    confirm_uninstall: Option<String>,
    tx: Sender<Msg>,
    rx: Receiver<Msg>,
}

impl Default for CircuitManager {
    fn default() -> Self {
        let (tx, rx) = channel();
        Self {
            open: false,
            tab: Tab::Available,
            search: String::new(),
            official_only: false,
            list: Load::Idle,
            list_generation: 0,
            searched: (String::new(), false),
            selected: None,
            releases: HashMap::new(),
            chosen: HashMap::new(),
            installed: Vec::new(),
            installed_error: None,
            job: None,
            result: None,
            confirm_uninstall: None,
            tx,
            rx,
        }
    }
}

/// The developer's REX sign-in (the same token Rex uses).
fn token() -> Option<String> {
    crate::export::config_dir().and_then(|d| kit_rex::TokenStore::new(d.join("rex_token")).load())
}

fn client() -> Client {
    Client::new(api_base(), token())
}

fn cache_dir() -> Option<PathBuf> {
    crate::export::config_dir().map(|d| d.join("circuits").join("cache"))
}

impl CircuitManager {
    pub fn show_for(&mut self, ed: &mut Editor) {
        if ed.project.is_none() {
            ed.notify("Open or create a project first (Project menu)");
            return;
        }
        self.open = true;
        self.result = None;
        self.refresh_installed(ed);
        self.fetch_list();
    }

    fn refresh_installed(&mut self, ed: &Editor) {
        let Some(root) = ed.project.as_ref().map(|p| p.root().to_path_buf()) else { return };
        match install::installed(&root) {
            Ok(list) => {
                self.installed = list;
                self.installed_error = None;
            }
            Err(e) => self.installed_error = Some(format!("{e:#}")),
        }
    }

    fn fetch_list(&mut self) {
        self.list_generation += 1;
        let generation = self.list_generation;
        self.searched = (self.search.clone(), self.official_only);
        self.list = Load::Loading;
        let q = Query { search: self.search.clone(), official_only: self.official_only, platform: None, page: 1 };
        let tx = self.tx.clone();
        std::thread::spawn(move || {
            let _ = tx.send(Msg::Listed(generation, client().list(&q).map(|p| p.circuits)));
        });
    }

    fn fetch_releases(&mut self, id: &str) {
        if matches!(self.releases.get(id), Some(Load::Loading | Load::Ready(_))) {
            return;
        }
        self.releases.insert(id.to_owned(), Load::Loading);
        let (tx, id) = (self.tx.clone(), id.to_owned());
        std::thread::spawn(move || {
            let r = client().releases(&id);
            let _ = tx.send(Msg::Releases(id, r));
        });
    }

    fn start_install(&mut self, ed: &Editor, circuit: &Circuit, release: &Release) {
        let Some(root) = ed.project.as_ref().map(|p| p.root().to_path_buf()) else { return };
        let tx = self.tx.clone();
        let rel = release.release.clone();
        let keys = circuit.publisher.keys.clone();
        let (id, version, name) = (circuit.id.clone(), release.summary.version.clone(), circuit.name.clone());
        self.job = Some((format!("Preparing {name} {version}"), 0.0));
        self.result = None;
        std::thread::spawn(move || {
            let c = client();
            let source = c.download_url(&id, &version);
            let cached = cache_dir().map(|d| d.join(format!("{}.kitcircuit", rel.package.sha256)));
            let bytes = match cached
                .as_ref()
                .and_then(|p| std::fs::read(p).ok())
                .filter(|b| kit_circuit::package::sha256_hex(b) == rel.package.sha256)
            {
                Some(b) => b,
                None => {
                    let _ = tx.send(Msg::Progress(format!("Downloading {name} {version}"), 0.0));
                    let mut last = 0.0f32;
                    let r = c.download(&id, &version, |done, total| {
                        let f = if total > 0 { done as f32 / total as f32 } else { 0.0 };
                        if f - last > 0.02 {
                            last = f;
                            let _ = tx.send(Msg::Progress(format!("Downloading {name} {version}"), f * 0.8));
                        }
                    });
                    match r {
                        Ok(b) => b,
                        Err(e) => {
                            let _ = tx.send(Msg::Finished(Err(if e.needs_sign_in() {
                                "Your Kit sign-in is missing or expired. Sign in and try again.".into()
                            } else {
                                e.message
                            })));
                            return;
                        }
                    }
                }
            };
            let _ = tx.send(Msg::Progress(format!("Verifying {name} {version}"), 0.85));
            if let Err(e) = kit_circuit::package::verify(&bytes, &rel, &keys) {
                let _ = tx.send(Msg::Finished(Err(format!("{e:#}. Nothing was changed."))));
                return;
            }
            if let Some(p) = &cached {
                let _ = std::fs::create_dir_all(p.parent().unwrap());
                let _ = std::fs::write(p, &bytes);
            }
            let _ = tx.send(Msg::Progress(format!("Installing {name} {version}"), 0.92));
            let r = install::install(&root, &rel, &bytes, &keys, &source)
                .map(|()| format!("{name} {version} is installed in this project."))
                .map_err(|e| format!("{e:#}. Nothing was changed."));
            let _ = tx.send(Msg::Finished(r));
        });
    }

    fn poll(&mut self, ed: &Editor) {
        while let Ok(m) = self.rx.try_recv() {
            match m {
                Msg::Listed(g, r) if g == self.list_generation => {
                    self.list = match r {
                        Ok(v) => Load::Ready(v),
                        Err(e) => Load::Failed(e),
                    }
                }
                Msg::Listed(..) => {}
                Msg::Releases(id, r) => {
                    let load = match r {
                        Ok(v) => Load::Ready(v),
                        Err(e) => Load::Failed(e),
                    };
                    self.releases.insert(id, load);
                }
                Msg::Progress(text, f) => self.job = Some((text, f)),
                Msg::Finished(r) => {
                    self.job = None;
                    self.result = Some(r);
                    self.refresh_installed(ed);
                }
            }
        }
    }

    fn installed_version(&self, id: &str) -> Option<&Installed> {
        self.installed.iter().find(|i| i.lock.id == id)
    }
}

fn newest_compatible(releases: &[Release]) -> Option<&Release> {
    let (kit, platform) = (kit_version(), host_platform());
    releases
        .iter()
        .filter(|r| {
            !r.summary.yanked
                && r.summary.channel == "stable"
                && r.release.manifest.incompatibility(&kit, platform).is_none()
        })
        .max_by(|a, b| version(&a.summary.version).cmp(&version(&b.summary.version)))
}

fn version(v: &str) -> Version {
    Version::parse(v).unwrap_or(Version::new(0, 0, 0))
}

fn badge(ui: &mut egui::Ui, text: &str, color: Color32) {
    Frame::new()
        .stroke(Stroke::new(1.0, color))
        .corner_radius(CornerRadius::same(6))
        .inner_margin(Margin::symmetric(5, 1))
        .show(ui, |ui| {
            ui.label(RichText::new(text).size(10.5).color(color));
        });
}

pub fn window(ctx: &egui::Context, ed: &mut Editor, cm: &mut CircuitManager, rex: &mut crate::rex_panel::RexPanel) {
    if !cm.open {
        return;
    }
    cm.poll(ed);
    if cm.job.is_some() || matches!(cm.list, Load::Loading) || cm.releases.values().any(|r| matches!(r, Load::Loading))
    {
        ctx.request_repaint_after(std::time::Duration::from_millis(100));
    }
    let mut open = cm.open;
    egui::Window::new(RichText::new("Circuit Manager").strong())
        .id(Id::new("circuit_manager"))
        .open(&mut open)
        .default_size([820.0, 540.0])
        .collapsible(false)
        .frame(
            Frame::new()
                .fill(DIALOG_BG())
                .stroke(Stroke::new(1.0, DIALOG_BORDER()))
                .corner_radius(CornerRadius::same(15))
                .inner_margin(Margin::same(18)),
        )
        .show(ctx, |ui| body(ui, ed, cm, rex));
    cm.open = open;
}

fn body(ui: &mut egui::Ui, ed: &mut Editor, cm: &mut CircuitManager, rex: &mut crate::rex_panel::RexPanel) {
    if ed.project.is_none() {
        ui.label("Open a project to manage its Circuits.");
        return;
    }
    let signed_in = token().is_some();
    ui.horizontal(|ui| {
        ui.selectable_value(&mut cm.tab, Tab::Available, "Available");
        ui.selectable_value(&mut cm.tab, Tab::Installed, format!("Installed ({})", cm.installed.len()));
        let updates = updates(cm).len();
        ui.selectable_value(
            &mut cm.tab,
            Tab::Updates,
            if updates > 0 { format!("Updates ({updates})") } else { "Updates".into() },
        );
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if !signed_in {
                if crate::widgets::primary(ui, "Sign in to Kit", None)
                    .on_hover_text("Uses your Kit (Rex) account")
                    .clicked()
                {
                    rex.sign_in();
                }
                ui.label(RichText::new("Sign in to download Circuits").color(MUTED()).size(12.0));
            } else if ui.small_button("↻").on_hover_text("Refresh").clicked() {
                cm.releases.clear();
                cm.refresh_installed(ed);
                cm.fetch_list();
            }
        });
    });
    ui.horizontal(|ui| {
        let r = ui.add(egui::TextEdit::singleline(&mut cm.search).hint_text("Search Circuits").desired_width(260.0));
        ui.checkbox(&mut cm.official_only, "Official only");
        if (r.changed() || cm.searched.1 != cm.official_only)
            && (cm.searched.0 != cm.search || cm.searched.1 != cm.official_only)
        {
            cm.fetch_list();
        }
    });
    if let Some((text, f)) = &cm.job {
        ui.add(egui::ProgressBar::new(*f).text(text.as_str()).animate(true));
    }
    match &cm.result {
        Some(Ok(t)) => {
            ui.colored_label(Color32::from_rgb(98, 214, 160), t);
        }
        Some(Err(t)) => {
            ui.colored_label(Color32::from_rgb(255, 122, 138), t);
        }
        None => {}
    }
    if let Some(e) = &cm.installed_error {
        ui.colored_label(Color32::from_rgb(255, 122, 138), format!("circuits.lock: {e}"));
    }
    ui.separator();

    let rows: Vec<Row> = rows(cm);
    ui.columns(2, |cols| {
        egui::ScrollArea::vertical().id_salt("circuit_list").show(&mut cols[0], |ui| {
            match (&cm.list, cm.tab) {
                (Load::Loading, Tab::Available) => {
                    ui.spinner();
                }
                (Load::Failed(e), _) => {
                    ui.colored_label(Color32::from_rgb(255, 122, 138), e.to_string());
                    if e.needs_sign_in() && ui.button("Sign in to Kit").clicked() {
                        rex.sign_in();
                    }
                }
                _ => {}
            }
            if rows.is_empty() && !matches!(cm.list, Load::Loading) {
                ui.label(
                    RichText::new(match cm.tab {
                        Tab::Available => "No Circuits match.",
                        Tab::Installed => "No Circuits installed in this project.",
                        Tab::Updates => "Everything is up to date.",
                    })
                    .color(MUTED()),
                );
            }
            for row in &rows {
                let selected = cm.selected.as_deref() == Some(row.id.as_str());
                let resp = Frame::new()
                    .fill(if selected { CARD() } else { Color32::TRANSPARENT })
                    .stroke(Stroke::new(1.0, if selected { ACCENT() } else { CARD_BORDER() }))
                    .corner_radius(CornerRadius::same(10))
                    .inner_margin(Margin::same(10))
                    .show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.horizontal(|ui| {
                            ui.label(RichText::new(&row.name).strong().color(TEXT_STRONG()));
                            if row.official {
                                badge(ui, "Official", ACCENT());
                            } else {
                                badge(ui, "Community", MUTED());
                            }
                            if let Some(v) = &row.installed {
                                badge(ui, &format!("installed {v}"), Color32::from_rgb(98, 214, 160));
                            }
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                if row.tags.iter().any(|tag| tag.eq_ignore_ascii_case("Pro")) {
                                    badge(ui, "Pro", ACCENT());
                                }
                            });
                        });
                        ui.label(RichText::new(&row.description).size(12.0).color(TEXT()));
                        ui.label(
                            RichText::new(format!(
                                "{} · {}",
                                row.publisher,
                                row.latest
                                    .as_deref()
                                    .map(|v| format!("latest {v}"))
                                    .unwrap_or_else(|| "no releases yet".into())
                            ))
                            .size(11.0)
                            .color(MUTED()),
                        );
                    })
                    .response
                    .interact(egui::Sense::click());
                if resp.clicked() {
                    cm.selected = Some(row.id.clone());
                }
                ui.add_space(6.0);
            }
        });
        egui::ScrollArea::vertical().id_salt("circuit_detail").show(&mut cols[1], |ui| detail(ui, ed, cm));
    });
}

struct Row {
    id: String,
    name: String,
    description: String,
    publisher: String,
    official: bool,
    tags: Vec<String>,
    latest: Option<String>,
    installed: Option<String>,
}

fn rows(cm: &CircuitManager) -> Vec<Row> {
    let listed: &[Circuit] = match &cm.list {
        Load::Ready(v) => v,
        _ => &[],
    };
    let from = |c: &Circuit| Row {
        id: c.id.clone(),
        name: c.name.clone(),
        description: c.description.clone(),
        publisher: c.publisher.name.clone(),
        official: c.official,
        tags: c.tags.clone(),
        latest: c.latest.as_ref().map(|l| l.version.clone()),
        installed: cm.installed_version(&c.id).map(|i| i.lock.version.clone()),
    };
    match cm.tab {
        Tab::Available => listed.iter().map(from).collect(),
        Tab::Installed => cm
            .installed
            .iter()
            .map(|i| match listed.iter().find(|c| c.id == i.lock.id) {
                Some(c) => from(c),
                None => Row {
                    id: i.lock.id.clone(),
                    name: i.manifest.as_ref().map(|m| m.circuit.name.clone()).unwrap_or_else(|| i.lock.id.clone()),
                    description: i.manifest.as_ref().map(|m| m.circuit.description.clone()).unwrap_or_default(),
                    publisher: i.manifest.as_ref().map(|m| m.circuit.publisher.clone()).unwrap_or_default(),
                    official: false,
                    tags: i.manifest.as_ref().map(|m| m.circuit.tags.clone()).unwrap_or_default(),
                    latest: None,
                    installed: Some(i.lock.version.clone()),
                },
            })
            .collect(),
        Tab::Updates => updates(cm).into_iter().filter_map(|id| listed.iter().find(|c| c.id == id)).map(from).collect(),
    }
}

/// Installed Circuits with a newer stable release (from the listing).
fn updates(cm: &CircuitManager) -> Vec<String> {
    let Load::Ready(listed) = &cm.list else { return Vec::new() };
    cm.installed
        .iter()
        .filter(|i| {
            listed.iter().any(|c| {
                c.id == i.lock.id
                    && c.latest.as_ref().is_some_and(|l| {
                        l.channel == "stable" && !l.yanked && version(&l.version) > version(&i.lock.version)
                    })
            })
        })
        .map(|i| i.lock.id.clone())
        .collect()
}

fn detail(ui: &mut egui::Ui, ed: &mut Editor, cm: &mut CircuitManager) {
    let Some(id) = cm.selected.clone() else {
        ui.label(RichText::new("Select a Circuit to see its releases.").color(MUTED()));
        return;
    };
    let circuit = match &cm.list {
        Load::Ready(v) => v.iter().find(|c| c.id == id).cloned(),
        _ => None,
    };
    let installed = cm.installed_version(&id).cloned();
    let Some(circuit) = circuit else {
        // Installed but not listed (offline, removed from the registry, or private).
        if let Some(i) = installed {
            installed_controls(ui, ed, cm, &i);
        }
        return;
    };
    cm.fetch_releases(&id);
    ui.heading(RichText::new(&circuit.name).color(TEXT_STRONG()));
    ui.label(RichText::new(&circuit.description).color(TEXT()));
    ui.add_space(4.0);
    ui.horizontal_wrapped(|ui| {
        ui.label(
            RichText::new(format!(
                "{} {}",
                circuit.publisher.name,
                if circuit.publisher.verified { "✔ verified" } else { "" }
            ))
            .size(12.0)
            .color(MUTED()),
        );
        if let Some(l) = &circuit.license {
            ui.label(RichText::new(format!("· {l}")).size(12.0).color(MUTED()));
        }
    });
    ui.horizontal_wrapped(|ui| {
        if let Some(u) = &circuit.documentation {
            ui.hyperlink_to("Documentation", u);
        }
        if let Some(u) = &circuit.repository {
            ui.hyperlink_to("Source", u);
        }
    });
    if let Some(i) = &installed {
        ui.add_space(6.0);
        installed_controls(ui, ed, cm, i);
    }
    ui.add_space(8.0);
    crate::widgets::section_label(ui, "RELEASES");
    let releases = match cm.releases.get(&id) {
        Some(Load::Ready(r)) => r.clone(),
        Some(Load::Failed(e)) => {
            ui.colored_label(Color32::from_rgb(255, 122, 138), e.to_string());
            return;
        }
        _ => {
            ui.spinner();
            return;
        }
    };
    if releases.is_empty() {
        ui.label(RichText::new("No published releases yet.").color(MUTED()));
        return;
    }
    let default = newest_compatible(&releases)
        .map(|r| r.summary.version.clone())
        .unwrap_or_else(|| releases[0].summary.version.clone());
    let chosen = cm.chosen.entry(id.clone()).or_insert(default).clone();
    egui::ComboBox::from_id_salt(("circuit_version", &id)).selected_text(&chosen).show_ui(ui, |ui| {
        for r in &releases {
            let label = format!(
                "{} · {}{}",
                r.summary.version,
                r.summary.channel,
                if r.summary.yanked { " · withdrawn" } else { "" }
            );
            ui.selectable_value(cm.chosen.get_mut(&id).unwrap(), r.summary.version.clone(), label);
        }
    });
    let Some(release) = releases.iter().find(|r| r.summary.version == chosen) else { return };
    let m = &release.release.manifest;
    let problem = m.incompatibility(&kit_version(), host_platform());
    ui.label(
        RichText::new(format!(
            "Kit {} · {} · {:.1} MB",
            m.circuit.kit,
            m.circuit.platforms.join(", "),
            release.release.package.size as f64 / 1e6
        ))
        .size(12.0)
        .color(MUTED()),
    );
    if !m.circuit.permissions.is_empty() {
        ui.label(RichText::new("Asks to:").size(12.0).color(LABEL()));
        for p in &m.circuit.permissions {
            let text = PERMISSIONS.iter().find(|(id, _)| id == p).map(|(_, t)| *t).unwrap_or(p);
            ui.label(RichText::new(format!("• {text}")).size(12.0));
        }
    }
    if !m.dependencies.is_empty() {
        ui.label(
            RichText::new(format!(
                "Needs: {}",
                m.dependencies.iter().map(|(k, v)| format!("{k} {v}")).collect::<Vec<_>>().join(", ")
            ))
            .size(12.0),
        );
    }
    if release.release.package.signature.is_some() {
        ui.label(RichText::new("Signed by its publisher").size(12.0).color(Color32::from_rgb(98, 214, 160)));
    }
    if let Some(p) = &problem {
        ui.colored_label(Color32::from_rgb(255, 207, 102), format!("Not compatible: {p}"));
    }
    let action = match &installed {
        None => "Install",
        Some(i) if i.lock.version == chosen => "Reinstall",
        Some(i) if version(&chosen) > version(&i.lock.version) => "Update",
        Some(_) => "Downgrade",
    };
    ui.add_space(4.0);
    let busy = cm.job.is_some();
    if ui
        .add_enabled(!busy && problem.is_none() && token().is_some(), egui::Button::new(format!("{action} {chosen}")))
        .clicked()
    {
        cm.start_install(ed, &circuit, release);
    }
    if let Some(notes) = release.release.notes.as_deref().filter(|n| !n.trim().is_empty()) {
        ui.add_space(8.0);
        crate::widgets::section_label(ui, "RELEASE NOTES");
        ui.label(RichText::new(notes).size(12.0));
    }
}

fn installed_controls(ui: &mut egui::Ui, ed: &mut Editor, cm: &mut CircuitManager, i: &Installed) {
    let Some(root) = ed.project.as_ref().map(|p| p.root().to_path_buf()) else { return };
    ui.label(
        RichText::new(format!("Installed {} in this project", i.lock.version)).color(Color32::from_rgb(98, 214, 160)),
    );
    if let Some(p) = &i.problem {
        ui.colored_label(Color32::from_rgb(255, 207, 102), p);
    }
    ui.horizontal(|ui| {
        let mut enabled = i.lock.enabled;
        if ui
            .checkbox(&mut enabled, "Enabled")
            .on_hover_text("Disabled Circuits stay installed but the game doesn't load them")
            .changed()
        {
            cm.result = Some(
                install::set_enabled(&root, &i.lock.id, enabled)
                    .map(|()| format!("{} {}", i.lock.id, if enabled { "enabled" } else { "disabled" }))
                    .map_err(|e| format!("{e:#}")),
            );
            cm.refresh_installed(ed);
        }
        if cm.confirm_uninstall.as_deref() == Some(i.lock.id.as_str()) {
            ui.label("Remove it from this project?");
            if ui.button("Uninstall").clicked() {
                cm.result = Some(
                    install::uninstall(&root, &i.lock.id)
                        .map(|()| format!("{} was removed. Your scenes and scripts were not changed.", i.lock.id))
                        .map_err(|e| format!("{e:#}")),
                );
                cm.confirm_uninstall = None;
                cm.refresh_installed(ed);
            }
            if ui.button("Cancel").clicked() {
                cm.confirm_uninstall = None;
            }
        } else if ui.button("Uninstall…").clicked() {
            cm.confirm_uninstall = Some(i.lock.id.clone());
        }
    });
}
