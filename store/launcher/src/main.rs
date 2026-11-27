use eframe::egui;
use kit_launcher_core::{ApiClient, CatalogEntry, InstallLayout, InstalledGame, install, launch, uninstall};
use std::sync::mpsc::{Receiver, TryRecvError, channel};
use std::thread;

enum Event {
    Catalog(Result<Vec<CatalogEntry>, String>),
    Install { id: String, result: Result<InstalledGame, String> },
}

struct StoreApp {
    games: Vec<CatalogEntry>,
    selected: Option<usize>,
    status: String,
    progress: Option<(u64, u64)>,
    events: Receiver<Event>,
    tx: std::sync::mpsc::Sender<Event>,
    layout: InstallLayout,
}

impl StoreApp {
    fn new() -> Self {
        let (tx, events) = channel();
        let catalog_tx = tx.clone();
        thread::spawn(move || {
            let result = ApiClient::default().catalog().map(|catalog| catalog.games).map_err(|error| error.to_string());
            let _ = catalog_tx.send(Event::Catalog(result));
        });
        Self {
            games: Vec::new(),
            selected: None,
            status: "Loading catalog…".into(),
            progress: None,
            events,
            tx,
            layout: InstallLayout::for_current_user().unwrap_or_else(|_| InstallLayout::new(".kit-store")),
        }
    }

    fn installed(&self, game: &CatalogEntry) -> bool {
        self.layout.root.join(&game.id).join("active").exists()
    }

    fn install(&mut self, game: CatalogEntry) {
        let Some(release) = game.latest.clone() else {
            self.status = "This game has no approved Linux release.".into();
            return;
        };
        let Some(size) = release.size else {
            self.status = "This release is missing its download size.".into();
            return;
        };
        let id = game.id.clone();
        let tx = self.tx.clone();
        self.status = format!("Downloading {}…", game.name);
        self.progress = Some((0, size));
        thread::spawn(move || {
            let result = (|| {
                let client = ApiClient::default();
                let bytes = client.download_release(&release, |_done, _total| {}).map_err(|error| error.to_string())?;
                let layout = InstallLayout::for_current_user().map_err(|error| error.to_string())?;
                install(&layout, &id, &release.version, &bytes, release.sha256.as_deref().unwrap_or_default())
                    .map_err(|error| error.to_string())
            })();
            let _ = tx.send(Event::Install { id, result });
        });
    }
}

impl eframe::App for StoreApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        loop {
            match self.events.try_recv() {
                Ok(Event::Catalog(result)) => match result {
                    Ok(games) => {
                        self.status = format!("{} games available", games.len());
                        self.games = games;
                    }
                    Err(error) => self.status = format!("Catalog error: {error}"),
                },
                Ok(Event::Install { id, result }) => match result {
                    Ok(_) => {
                        self.status = format!("Installed {id}");
                        self.progress = None;
                    }
                    Err(error) => {
                        self.status = format!("Install failed: {error}");
                        self.progress = None;
                    }
                },
                Err(TryRecvError::Empty) | Err(TryRecvError::Disconnected) => break,
            }
        }

        ui.heading("Kit Store");
        ui.label("Free Linux games from publish.kitgameengine.win");
        ui.separator();
        ui.label(&self.status);
        ui.separator();
        ui.horizontal(|ui| {
            ui.group(|ui| {
                ui.heading("Catalog");
                for (index, game) in self.games.iter().enumerate() {
                    if ui.selectable_label(self.selected == Some(index), &game.name).clicked() {
                        self.selected = Some(index);
                    }
                }
                ui.separator();
                ui.heading("Installed");
                for game in &self.games {
                    if self.installed(game) {
                        ui.label(&game.name);
                    }
                }
            });
            ui.separator();
            ui.vertical(|ui| {
                let Some(index) = self.selected else {
                    ui.label("Select a game to view its details.");
                    return;
                };
                let game = self.games[index].clone();
                ui.heading(&game.name);
                ui.add_space(8.0);
                ui.label(game.description.as_deref().unwrap_or("No description available."));
                if let Some(release) = &game.latest {
                    ui.label(format!("Version {}", release.version));
                    if let Some(size) = release.size {
                        ui.label(format!("{:.1} MB download", size as f64 / 1_000_000.0));
                    }
                    ui.horizontal(|ui| {
                        if self.installed(&game) {
                            if ui.button("Play").clicked() {
                                let active = self.layout.root.join(&game.id).join("active");
                                let executable = release.executable.as_deref().unwrap_or("kit-player");
                                let installed = InstalledGame {
                                    id: game.id.clone(),
                                    version: release.version.clone(),
                                    active,
                                    saves: self.layout.root.join(&game.id).join("saves"),
                                };
                                self.status = match launch(&installed, executable, std::iter::empty::<&str>()) {
                                    Ok(_) => format!("Playing {}", game.name),
                                    Err(error) => format!("Launch failed: {error}"),
                                };
                            }
                            if ui.button("Uninstall").clicked() {
                                self.status = match uninstall(&self.layout, &game.id) {
                                    Ok(()) => format!("Uninstalled {}", game.name),
                                    Err(error) => format!("Uninstall failed: {error}"),
                                };
                            }
                            if ui.button("Update").clicked() {
                                self.install(game.clone());
                            }
                        } else if ui.button("Install").clicked() {
                            self.install(game.clone());
                        }
                    });
                }
                if let Some((done, total)) = self.progress {
                    ui.add(egui::ProgressBar::new(done as f32 / total.max(1) as f32).show_percentage());
                }
            });
        });
        ui.ctx().request_repaint_after(std::time::Duration::from_millis(100));
    }
}

fn main() -> eframe::Result {
    eframe::run_native("Kit Store", eframe::NativeOptions::default(), Box::new(|_| Ok(Box::new(StoreApp::new()))))
}
