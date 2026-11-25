//! Rex, Kit's AI puppy: the chat panel at the bottom of the center area
//! (key 8). Sign in with a code, pick one of Rex's modes, chat, and drop the
//! scripts and Diagrams he writes straight into the project.

use crate::state::Editor;
use crate::theme::*;
use egui::{Align, Color32, CornerRadius, Frame, Key, Layout, Margin, RichText, Sense, Stroke, Ui, vec2};
use kit_rex::{ChatEvent, ChatRequest, Message, ModeInfo, Part, Role, SnippetKind, Update};
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

const MOODS: [&str; 6] = ["happy", "fetch", "sniff", "dig", "zoomies", "sit"];

fn face_bytes(mood: &str) -> &'static [u8] {
    match mood {
        "fetch" => include_bytes!("../assets/rex/fetch.png"),
        "sniff" => include_bytes!("../assets/rex/sniff.png"),
        "dig" => include_bytes!("../assets/rex/dig.png"),
        "zoomies" => include_bytes!("../assets/rex/zoomies.png"),
        "sit" => include_bytes!("../assets/rex/sit.png"),
        _ => include_bytes!("../assets/rex/happy.png"),
    }
}

/// One line of a conversation.
#[derive(Clone, Debug)]
pub struct Line {
    pub role: Role,
    pub text: String,
    /// Shown instead of text when a request failed.
    pub error: Option<(String, String)>,
    pub credits: Option<i64>,
}

pub enum Auth {
    /// Not signed in.
    SignedOut,
    /// Waiting for the browser approval.
    Pending { code: String, link: String },
    SignedIn,
}

pub struct RexPanel {
    worker: Option<kit_rex::Worker>,
    store: Option<kit_rex::TokenStore>,
    pub auth: Auth,
    modes: Vec<ModeInfo>,
    pub mode: String,
    chats: HashMap<String, Vec<Line>>,
    input: String,
    pub busy: bool,
    cancel: Arc<AtomicBool>,
    me: Option<kit_rex::Me>,
    note: Option<String>,
    faces: HashMap<String, egui::TextureHandle>,
    friendly: bool,
    pub include_context: bool,
    focus_input: bool,
    connected: bool,
    /// Send the typed message as soon as sign-in finishes.
    send_after_sign_in: bool,
    /// Send the typed message once connected (automation: `--rex-ask`).
    pub auto_send: bool,
}

/// The input's placeholder.
pub const INPUT_HINT: &str = "Talk to Rex to help you Code + Create your Game!";

fn mode_label(mode: &ModeInfo, friendly: bool) -> &str {
    if friendly { return &mode.name; }
    match mode.id.as_str() {
        "buddy" => "General",
        "fetch" => "Scripting",
        "sniff" => "Debugging",
        "dig" => "Diagrams",
        "zoomies" => "Game Design",
        "sit" => "Learning",
        _ => "Editor Help",
    }
}

fn plain_rex_text(text: &str) -> String {
    text.chars().filter(|ch| !matches!(ch,
        '\u{1F000}'..='\u{1FAFF}' | '\u{2600}'..='\u{27BF}' |
        '\u{FE0E}' | '\u{FE0F}' | '\u{200D}' | '\u{20E3}' |
        '\u{E0020}'..='\u{E007F}'
    )).collect()
}

fn rex_text(text: &str, friendly: bool) -> String {
    if friendly { crate::markdown::displayable(text) } else { plain_rex_text(text) }
}


impl Default for RexPanel {
    fn default() -> Self {
        Self {
            worker: None,
            store: None,
            auth: Auth::SignedOut,
            modes: kit_rex::builtin_modes(),
            mode: "buddy".into(),
            chats: HashMap::new(),
            input: String::new(),
            busy: false,
            cancel: Arc::new(AtomicBool::new(false)),
            me: None,
            note: None,
            faces: HashMap::new(),
            friendly: true,
            include_context: true,
            focus_input: false,
            connected: false,
            send_after_sign_in: false,
            auto_send: false,
        }
    }
}

impl RexPanel {
    /// Start talking to the service (once, the first time the panel opens).
    fn ensure_started(&mut self) {
        if self.worker.is_some() {
            return;
        }
        let store = crate::export::config_dir().map(|d| kit_rex::TokenStore::new(d.join("rex_token")));
        let token = store.as_ref().and_then(|s| s.load());
        if token.is_some() {
            self.auth = Auth::SignedIn;
        }
        let worker = kit_rex::Worker::new(kit_rex::Client::new(kit_rex::api_base(), token));
        worker.connect();
        self.worker = Some(worker);
        self.store = store;
    }

    fn face(&mut self, ctx: &egui::Context, mood: &str) -> egui::TextureHandle {
        let mood = if MOODS.contains(&mood) { mood } else { "happy" };
        self.faces
            .entry(mood.to_owned())
            .or_insert_with(|| {
                let img = image::load_from_memory(face_bytes(mood)).expect("bundled Rex face").into_rgba8();
                let size = [img.width() as usize, img.height() as usize];
                ctx.load_texture(format!("rex_{mood}"), egui::ColorImage::from_rgba_unmultiplied(size, &img), egui::TextureOptions::LINEAR)
            })
            .clone()
    }

    fn current_mode(&self) -> ModeInfo {
        self.modes.iter().find(|m| m.id == self.mode).cloned().unwrap_or_else(|| self.modes[0].clone())
    }

    fn chat(&mut self) -> &mut Vec<Line> {
        self.chats.entry(self.mode.clone()).or_default()
    }

    /// Start the Kit sign-in (device code in the browser), e.g. from Circuit Manager.
    pub fn sign_in(&mut self) {
        if let Some(w) = &self.worker {
            w.sign_in(format!("Kit Editor ({})", std::env::consts::OS));
        }
    }

    /// Put the cursor in the message box.
    pub fn ask_focus(&mut self) {
        self.focus_input = true;
    }

    /// Ask Rex something in a given mode (from elsewhere in the editor, e.g. "Ask Rex" on an error).
    pub fn ask(&mut self, mode: &str, text: String) {
        self.mode = mode.to_owned();
        self.input = text;
        self.focus_input = true;
    }

    fn handle_updates(&mut self) {
        let Some(updates) = self.worker.as_ref().map(|w| w.updates()) else { return };
        for u in updates {
            let worker = self.worker.as_mut().expect("worker");
            match u {
                Update::Connected(base) => {
                    worker.set_base(base);
                    self.connected = true;
                }
                Update::Modes(m) if !m.is_empty() => self.modes = m,
                Update::Modes(_) => {}
                Update::Me(me) => self.me = Some(me),
                Update::Device(d) => {
                    let _ = webbrowser::open(&d.verification_uri_complete);
                    self.auth = Auth::Pending { code: d.user_code, link: d.verification_uri_complete };
                }
                Update::SignedIn(token) => {
                    if let Some(s) = &self.store {
                        if let Err(e) = s.save(&token) {
                            self.note = Some(format!("Couldn't remember the sign-in: {e:#}"));
                        }
                    }
                    worker.set_token(Some(token));
                    worker.fetch_me();
                    self.auth = Auth::SignedIn;
                    self.note = None;
                }
                Update::SignInFailed(why) => {
                    self.auth = Auth::SignedOut;
                    self.send_after_sign_in = false;
                    self.note = Some(why);
                }
                Update::Chat(ChatEvent::Start { .. }) => {}
                Update::Chat(ChatEvent::Delta(d)) => {
                    if let Some(last) = self.chats.get_mut(&self.mode).and_then(|c| c.last_mut()) {
                        last.text.push_str(&d);
                    }
                }
                Update::Chat(ChatEvent::Done { credits, balance }) => {
                    if let Some(last) = self.chats.get_mut(&self.mode).and_then(|c| c.last_mut()) {
                        last.credits = Some(credits);
                    }
                    if let Some(me) = &mut self.me {
                        me.balance = balance;
                    }
                }
                Update::ChatFailed(e) => {
                    if e.code == "unauthorized" {
                        self.sign_out_local();
                        self.note = Some("Please sign in again.".into());
                    }
                    if let Some(last) = self.chats.get_mut(&self.mode).and_then(|c| c.last_mut()) {
                        if last.text.is_empty() {
                            last.error = Some((e.code.clone(), e.message.clone()));
                        }
                    }
                    if let Some(b) = e.retry_after.filter(|_| e.code == "out_of_credits") {
                        let _ = b;
                    }
                }
                Update::ChatEnded => self.busy = false,
                Update::Failed(e) => {
                    if e.code == "unauthorized" {
                        self.sign_out_local();
                    } else if e.code == "network" {
                        self.note = Some(e.message);
                    }
                }
            }
        }
    }

    fn sign_out_local(&mut self) {
        if let Some(s) = &self.store {
            s.clear();
        }
        if let Some(w) = self.worker.as_mut() {
            w.set_token(None);
        }
        self.auth = Auth::SignedOut;
        self.me = None;
    }

    fn send(&mut self, ed: &Editor) {
        let text = self.input.trim().to_owned();
        if text.is_empty() || self.busy {
            return;
        }
        self.input.clear();
        let mut context = self.include_context.then(|| project_context(ed)).flatten();
        if !ed.rex_friendly() {
            let preference = "Response style preference: use concise, professional editor language. Avoid puppy roleplay, references to treats, and emoji.";
            let current = context.get_or_insert_with(String::new);
            if !current.is_empty() { current.push_str("\n\n"); }
            current.push_str(preference);
        }
        let chat = self.chat();
        chat.push(Line { role: Role::User, text, error: None, credits: None });
        let messages: Vec<Message> = chat
            .iter()
            .filter(|l| l.error.is_none() && !l.text.is_empty())
            .map(|l| Message { role: l.role.clone(), content: l.text.clone() })
            .collect();
        chat.push(Line { role: Role::Assistant, text: String::new(), error: None, credits: None });
        self.busy = true;
        self.cancel = Arc::new(AtomicBool::new(false));
        let req = ChatRequest { mode: self.mode.clone(), messages, context, stream: true };
        if let Some(w) = &self.worker {
            w.chat(req, self.cancel.clone());
        }
    }
}

/// What Rex sees of the open project: scene objects, the selection (and its
/// script), and recent console problems.
pub fn project_context(ed: &Editor) -> Option<String> {
    let world = ed.world();
    let mut objects = Vec::new();
    for r in kit::scene::roots(world) {
        for e in kit::scene::descendants_and_self(world, r) {
            if objects.len() >= 60 {
                break;
            }
            let mut parts = vec![ed.entity_label(e)];
            if world.has::<kit::gamesys::Camera>(e) { parts.push("Camera".into()) }
            if world.has::<kit::gamesys::Light>(e) { parts.push("Light".into()) }
            if world.has::<kit::gamesys::Model>(e) { parts.push("Model".into()) }
            if world.has::<kit::gamesys::Sprite>(e) { parts.push("Sprite".into()) }
            if world.has::<kit::gamesys::Collider>(e) { parts.push("Collider".into()) }
            if world.has::<kit::gamesys::RigidBody>(e) { parts.push("Rigid Body".into()) }
            if let Some(s) = world.get::<kit::gamesys::Script>(e) { parts.push(format!("Script {}", s.path)) }
            if let Some(d) = world.get::<kit::gamesys::Diagram>(e) { parts.push(format!("Diagram {}", d.graph)) }
            objects.push(format!("- {}", parts.join(", ")));
        }
    }
    let mut selection = String::new();
    if let Some(e) = ed.selected.filter(|e| world.is_alive(*e)) {
        selection.push_str(&format!("Selected: {}\n", ed.entity_label(e)));
        if let (Some(s), Some(p)) = (world.get::<kit::gamesys::Script>(e), ed.project.as_ref()) {
            if let Ok(src) = p.resources.read_text(&s.path) {
                selection.push_str(&format!("Its script {}:\n```x\n{}\n```\n", s.path, src.chars().take(3000).collect::<String>()));
            }
        }
    }
    let problems: Vec<String> = {
        let c = world.resource::<kit::core::Console>();
        c.lines()
            .iter()
            .rev()
            .filter(|l| !matches!(l.level, kit::core::LogLevel::Info))
            .take(8)
            .map(|l| format!("- [{}] {}", l.source, l.text))
            .collect()
    };
    kit_rex::context_block(
        &[
            ("Project", ed.project_title().to_owned()),
            ("Scene objects", objects.join("\n")),
            ("Selection", selection),
            ("Recent console problems", problems.join("\n")),
        ],
        10_000,
    )
}

fn pill(ui: &mut Ui, text: &str, color: Color32) {
    Frame::new()
        .fill(color.gamma_multiply(0.18))
        .stroke(Stroke::new(1.0, color.gamma_multiply(0.6)))
        .corner_radius(CornerRadius::same(10))
        .inner_margin(Margin::symmetric(8, 2))
        .show(ui, |ui| {
            ui.label(RichText::new(text).size(11.0).color(color));
        });
}

/// Draw the panel.
pub fn show(ui: &mut Ui, ed: &mut Editor, rex: &mut RexPanel) {
    rex.friendly = ed.rex_friendly();
    if !rex.friendly { rex.faces.clear(); }
    rex.ensure_started();
    rex.handle_updates();
    if rex.busy || matches!(rex.auth, Auth::Pending { .. }) {
        ui.ctx().request_repaint_after(std::time::Duration::from_millis(60));
    }
    let ctx = ui.ctx().clone();
    let mode = rex.current_mode();
    let t = ctx.input(|i| i.time);
    // Never ask for more room than the panel has (or it would keep growing).
    let height = ui.available_height().floor() - 2.0;
    ui.set_max_height(height);

    ui.horizontal_top(|ui| {
        ui.set_max_height(height);
        // ---- Left: Rex himself.
        ui.allocate_ui_with_layout(vec2(150.0, height), Layout::top_down(Align::Center), |ui| {
            ui.set_max_height(height);
            if rex.friendly {
                let mood = if rex.busy { mode.mood.as_str() } else if matches!(rex.auth, Auth::SignedIn) { mode.mood.as_str() } else { "happy" };
                let tex = rex.face(&ctx, mood);
                // A little bounce while he's answering.
                let bob = if rex.busy { ((t * 6.0).sin() * 4.0) as f32 } else { ((t * 1.5).sin() * 1.5) as f32 };
                ui.add_space(4.0 + bob.max(0.0));
                ui.add(egui::Image::new(&tex).fit_to_exact_size(vec2(96.0, 96.0)));
                ui.add_space(4.0 - bob.min(0.0).abs().min(4.0));
            }
            ui.label(RichText::new(ed.rex_label()).size(16.0).strong());
            ui.label(RichText::new(if rex.busy { "Working…" } else { mode_label(&mode, rex.friendly) }).size(11.0).color(MUTED()));
            ui.add_space(6.0);
            if let Some(me) = &rex.me {
                pill(ui, &format!("{} credits", me.balance.total), ACCENT());
                ui.label(RichText::new(&me.plan.name).size(10.5).color(MUTED()));
                let links = me.links.clone();
                ui.horizontal(|ui| {
                    if ui.small_button("Account").clicked() {
                        let _ = webbrowser::open(&links.account);
                    }
                    if ui.small_button("Sign out").clicked() {
                        if let Some(w) = &rex.worker {
                            w.sign_out();
                        }
                        rex.sign_out_local();
                    }
                });
            }
        });
        ui.separator();

        // ---- Right: modes, chat, input.
        ui.vertical(|ui| {
            ui.set_max_height(height);
            match &rex.auth {
            Auth::SignedOut => signed_out(ui, ed, rex),
            Auth::Pending { code, link } => {
                let (code, link) = (code.clone(), link.clone());
                ui.add_space(10.0);
                ui.label(RichText::new("Approve Kit in your browser").size(16.0).strong());
                ui.label(RichText::new("Check that the page shows this code, then click Connect:").color(MUTED()));
                ui.add_space(6.0);
                Frame::new().stroke(Stroke::new(1.5, ACCENT())).corner_radius(CornerRadius::same(10)).inner_margin(Margin::symmetric(18, 8)).show(ui, |ui| {
                    ui.label(RichText::new(&code).size(26.0).monospace().strong().color(ACCENT()));
                });
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    if ui.button("Open the page again").clicked() {
                        let _ = webbrowser::open(&link);
                    }
                    if ui.button("Cancel").clicked() {
                        rex.auth = Auth::SignedOut;
                    }
                    ui.spinner();
                });
            }
            Auth::SignedIn => signed_in(ui, ed, rex, &mode),
            }
        });
    });
}

/// The message box (shared by the signed-in and signed-out views). Returns
/// true when the user pressed Enter or Send.
fn input_row(ui: &mut Ui, rex: &mut RexPanel, send_label: &str) -> bool {
    let mut send = false;
    ui.horizontal(|ui| {
        let w = ui.available_width() - 110.0;
        let edit = egui::TextEdit::multiline(&mut rex.input)
            .hint_text(RichText::new(if rex.friendly { INPUT_HINT } else { "Ask about your project" }).color(MUTED()))
            .desired_rows(2)
            .desired_width(w)
            .margin(Margin::symmetric(10, 8))
            .lock_focus(true);
        let r = ui.add(edit);
        crate::ui::mark_rect(ui, "rex_input", r.rect);
        if std::mem::take(&mut rex.focus_input) {
            r.request_focus();
        }
        let enter = r.has_focus() && ui.input(|i| i.key_pressed(Key::Enter) && !i.modifiers.shift);
        if enter {
            // The TextEdit already added the newline.
            while rex.input.ends_with('\n') {
                rex.input.pop();
            }
        }
        ui.vertical(|ui| {
            if rex.busy {
                if ui.button("Stop").clicked() {
                    rex.cancel.store(true, Ordering::Relaxed);
                }
            } else if (crate::widgets::primary(ui, send_label, None).clicked() || enter) && !rex.input.trim().is_empty() {
                send = true;
            }
            ui.checkbox(&mut rex.include_context, RichText::new("Share scene").size(10.5))
                .on_hover_text("Include the active scene, selected object’s script, and recent console errors");
        });
    });
    send
}

fn signed_out(ui: &mut Ui, _ed: &mut Editor, rex: &mut RexPanel) {
    ui.add_space(10.0);
    ui.label(RichText::new(if rex.friendly { "Meet Rex, your game-making puppy 🐾" } else { "Kit Editor Assistant" }).size(17.0).strong());
    ui.label(
        RichText::new(if rex.friendly {
            "He writes x scripts, builds Diagrams, sniffs out bugs, brainstorms ideas and teaches. Sign in with your Kit account (free to start)."
        } else {
            "Sign in with your Kit account for help with scripts, diagrams, debugging, and game design."
        })
            .color(MUTED()),
    );
    ui.add_space(8.0);
    ui.horizontal(|ui| {
        if crate::widgets::primary(ui, "Sign in to Kit", None).clicked() {
            if let Some(w) = &rex.worker {
                w.sign_in(format!("Kit Editor ({})", std::env::consts::OS));
                rex.note = None;
            }
        }
        if !rex.connected {
            ui.label(RichText::new("connecting…").size(11.0).color(MUTED()));
        }
    });
    if let Some(n) = &rex.note {
        ui.label(RichText::new(rex_text(n, rex.friendly)).size(11.5).color(WARN()));
    }
    ui.add_space(10.0);
    mode_chips(ui, rex);
    if rex.friendly {
        ui.label(RichText::new(crate::markdown::displayable(&rex.current_mode().tagline)).size(11.5).color(MUTED()));
    }
    ui.add_space((ui.available_height() - 72.0).max(4.0));
    // Typing first works too: sign in, then the message goes out.
    if input_row(ui, rex, "Send") {
        if let Some(w) = &rex.worker {
            w.sign_in(format!("Kit Editor ({})", std::env::consts::OS));
            rex.send_after_sign_in = true;
            rex.note = None;
        }
    }
}

/// Rex's modes as chips with his face for each.
fn mode_chips(ui: &mut Ui, rex: &mut RexPanel) {
    let ctx = ui.ctx().clone();
    ui.horizontal_wrapped(|ui| {
        for m in rex.modes.clone() {
            let on = m.id == rex.mode;
            let tex = rex.friendly.then(|| rex.face(&ctx, &m.mood));
            let fill = if on { ACCENT().gamma_multiply(0.22) } else { Color32::TRANSPARENT };
            let stroke = if on { Stroke::new(1.5, ACCENT()) } else { Stroke::new(1.0, CARD_BORDER()) };
            let r = Frame::new().fill(fill).stroke(stroke).corner_radius(CornerRadius::same(14)).inner_margin(Margin { left: 4, right: 10, top: 2, bottom: 2 }).show(ui, |ui| {
                ui.horizontal(|ui| {
                    if let Some(tex) = &tex { ui.add(egui::Image::new(tex).fit_to_exact_size(vec2(22.0, 22.0))); }
                    ui.label(RichText::new(mode_label(&m, rex.friendly)).size(12.5).color(if on { TEXT_STRONG() } else { LABEL() }));
                });
            });
            let r = ui.interact(r.response.rect, ui.id().with(("rex_mode", &m.id)), Sense::click()).on_hover_text(if rex.friendly { crate::markdown::displayable(&m.tagline) } else { mode_label(&m, false).into() });
            if r.clicked() && !rex.busy {
                rex.mode = m.id.clone();
            }
        }
    });
}

fn signed_in(ui: &mut Ui, ed: &mut Editor, rex: &mut RexPanel, mode: &ModeInfo) {
    mode_chips(ui, rex);
    ui.add_space(4.0);

    let dig_face = rex.friendly.then(|| rex.face(ui.ctx(), "dig"));
    let avatar = rex.friendly.then(|| rex.face(ui.ctx(), &mode.mood));
    // Input row (bottom), conversation above it.
    let input_h = 78.0;
    let chat_h = (ui.available_height() - input_h).max(60.0);
    egui::ScrollArea::vertical().id_salt(("rex_chat", &rex.mode)).max_height(chat_h).auto_shrink([false, false]).stick_to_bottom(true).show(ui, |ui| {
        let lines = rex.chats.get(&rex.mode).cloned().unwrap_or_default();
        if lines.is_empty() {
            bubble(ui, false, avatar.as_ref(), |ui| {
                crate::markdown::show(ui, if rex.friendly { &mode.greeting } else { "Describe what you want to build or fix." });
            });
            ui.horizontal_wrapped(|ui| {
                for ex in &mode.examples {
                    let ex = rex_text(ex, rex.friendly);
                    if ui.button(RichText::new(format!("“{ex}”")).size(11.5)).clicked() {
                        rex.input = ex.clone();
                        rex.focus_input = true;
                    }
                }
            });
        }
        let last = lines.len().saturating_sub(1);
        for (i, line) in lines.iter().enumerate() {
            let mine = line.role == Role::User;
            bubble(ui, mine, avatar.as_ref(), |ui| {
                if let Some((code, msg)) = &line.error {
                    ui.label(RichText::new(rex_text(msg, rex.friendly)).color(ERROR()));
                    if code == "out_of_credits" {
                        if let Some(me) = &rex.me {
                            if ui.button(if rex.friendly { "Get more treats (credits)" } else { "Get more credits" }).clicked() {
                                let _ = webbrowser::open(&me.links.pricing);
                            }
                        }
                    }
                } else if mine {
                    ui.label(RichText::new(crate::markdown::displayable(&line.text)).color(TEXT_STRONG()));
                } else if line.text.is_empty() && rex.busy && i == last {
                    ui.horizontal(|ui| {
                        ui.spinner();
                        ui.label(RichText::new(if rex.friendly { "Rex is thinking…" } else { "Generating response…" }).color(MUTED()));
                    });
                } else {
                    reply(ui, ed, &line.text, rex.busy && i == last, dig_face.as_ref());
                    if let Some(c) = line.credits.filter(|c| *c > 0) {
                        ui.label(RichText::new(format!("{c} credit{}", if c == 1 { "" } else { "s" })).size(10.0).color(MUTED()));
                    }
                }
            });
        }
    });

    // Input (a message typed before signing in goes out now).
    ui.add_space(4.0);
    let auto = rex.auto_send && rex.connected && !rex.busy;
    if auto {
        rex.auto_send = false;
    }
    if (std::mem::take(&mut rex.send_after_sign_in) || auto) && !rex.input.trim().is_empty() {
        rex.send(ed);
    }
    if input_row(ui, rex, "Send") {
        rex.send(ed);
    }
    if let Some(n) = &rex.note {
        ui.label(RichText::new(rex_text(n, rex.friendly)).size(11.0).color(WARN()));
    }
}

/// A chat message as a Material-style card: Rex's on the left with his face,
/// yours on the right in the accent color.
fn bubble(ui: &mut Ui, mine: bool, avatar: Option<&egui::TextureHandle>, add: impl FnOnce(&mut Ui)) {
    let layout = if mine { Layout::right_to_left(Align::Min) } else { Layout::left_to_right(Align::Min) };
    ui.with_layout(layout, |ui| {
        if let (false, Some(face)) = (mine, avatar) {
            Frame::new().fill(ACCENT().gamma_multiply(0.14)).corner_radius(CornerRadius::same(18)).inner_margin(Margin::same(2)).show(ui, |ui| {
                ui.add(egui::Image::new(face).fit_to_exact_size(vec2(32.0, 32.0)));
            });
            ui.add_space(4.0);
        }
        let max = (ui.available_width() * if mine { 0.78 } else { 0.92 }).max(200.0);
        Frame::new()
            .fill(if mine { ACCENT().gamma_multiply(0.22) } else { CARD() })
            .stroke(Stroke::new(1.0, if mine { ACCENT().gamma_multiply(0.35) } else { CARD_BORDER().gamma_multiply(0.7) }))
            .corner_radius(CornerRadius { nw: 18, ne: 18, sw: if mine { 18 } else { 6 }, se: if mine { 6 } else { 18 } })
            .shadow(egui::Shadow { offset: [0, 2], blur: 10, spread: 0, color: Color32::from_black_alpha(if crate::theme::is_light() { 28 } else { 70 }) })
            .inner_margin(Margin { left: 16, right: 16, top: 10, bottom: 10 })
            .show(ui, |ui| {
                ui.set_max_width(max);
                ui.with_layout(Layout::top_down(Align::Min), add);
            });
    });
    ui.add_space(10.0);
}

/// Rex's reply: text, plus code blocks with checks and one-click actions.
fn reply(ui: &mut Ui, ed: &mut Editor, text: &str, streaming: bool, dig_face: Option<&egui::TextureHandle>) {
    let gd: kit::gamedata::GameData = ed.world().try_resource::<kit::gamedata::GameData>().map(|g| g.clone()).unwrap_or_default();
    for (i, part) in kit_rex::parts(text).into_iter().enumerate() {
        match part {
            Part::Text(t) => crate::markdown::show(ui, &rex_text(&t, ed.rex_friendly())),
            Part::Code { kind, lang, code }
                if kind == SnippetKind::Diagram || (streaming && matches!(lang.as_str(), "kdiagram" | "diagram" | "json")) =>
            {
                // Diagrams show as a card, not JSON (the JSON is kept for Use / Copy).
                diagram_card(ui, ed, &code, &gd, streaming, dig_face);
            }
            Part::Code { kind, lang, code } => {
                Frame::new().fill(FIELD_BG()).corner_radius(CornerRadius::same(8)).inner_margin(Margin::same(8)).show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    let job = if kind == SnippetKind::Script { crate::code::highlight(&code) } else {
                        let mut j = egui::text::LayoutJob::default();
                        j.append(&code, 0.0, egui::TextFormat { font_id: egui::FontId::monospace(12.0), color: TEXT(), ..Default::default() });
                        j
                    };
                    egui::ScrollArea::vertical().id_salt(("rex_code", i, code.len() / 64)).max_height(260.0).show(ui, |ui| {
                        ui.add(egui::Label::new(job).wrap());
                    });
                });
                if streaming {
                    continue;
                }
                ui.horizontal_wrapped(|ui| {
                    if ui.small_button("Copy").clicked() {
                        ui.ctx().copy_text(code.clone());
                    }
                    match kind {
                        SnippetKind::Script => script_actions(ui, ed, &code),
                        SnippetKind::Diagram => diagram_actions(ui, ed, &code, &gd),
                        SnippetKind::Other => {
                            if !lang.is_empty() {
                                ui.label(RichText::new(lang).size(10.0).color(MUTED()));
                            }
                        }
                    }
                });
            }
        }
    }
}

/// The nodes of a Diagram in the order they run (events first, following the
/// white wires), then any value nodes.
fn flow(graph: &kit::diagram::Graph, gd: &kit::gamedata::GameData) -> Vec<String> {
    let mut order = Vec::new();
    let mut seen = std::collections::HashSet::new();
    let mut events: Vec<_> = graph.nodes.iter().filter(|n| n.kind.is_event()).map(|n| n.id).collect();
    events.sort();
    for start in events {
        let mut stack = vec![start];
        while let Some(id) = stack.pop() {
            if !seen.insert(id) {
                continue;
            }
            order.push(id);
            let mut next: Vec<_> = graph.links.iter().filter(|l| l.from.node == id && l.to.pin == "exec").map(|l| l.to.node).collect();
            next.reverse();
            stack.extend(next);
        }
    }
    let mut titles: Vec<String> = order.iter().filter_map(|id| graph.node(*id)).map(|n| n.kind.title(gd)).collect();
    let rest = graph.nodes.iter().filter(|n| !seen.contains(&n.id)).count();
    if rest > 0 {
        titles.push(format!("+{rest} value node{}", if rest == 1 { "" } else { "s" }));
    }
    titles
}

/// A Diagram from Rex: a summary card with Use / Copy (no JSON on screen).
fn diagram_card(ui: &mut Ui, ed: &mut Editor, code: &str, gd: &kit::gamedata::GameData, streaming: bool, face: Option<&egui::TextureHandle>) {
    Frame::new()
        .fill(ACCENT().gamma_multiply(0.10))
        .stroke(Stroke::new(1.0, ACCENT().gamma_multiply(0.45)))
        .corner_radius(CornerRadius::same(10))
        .inner_margin(Margin::same(10))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                if let Some(face) = face { ui.add(egui::Image::new(face).fit_to_exact_size(vec2(40.0, 40.0))); }
                ui.vertical(|ui| {
                    if streaming {
                        ui.horizontal(|ui| {
                            ui.spinner();
                            ui.label(RichText::new(if face.is_some() { "Rex is digging up a Diagram…" } else { "Generating diagram…" }).strong());
                        });
                        return;
                    }
                    match kit_rex::check_diagram(code, gd) {
                        Ok(graph) => {
                            ui.label(RichText::new(format!("Diagram · {} nodes", graph.nodes.len())).strong().color(TEXT_STRONG()));
                            ui.label(RichText::new(flow(&graph, gd).join("  →  ")).size(12.0).color(LABEL()));
                            ui.add_space(4.0);
                            ui.horizontal_wrapped(|ui| {
                                diagram_actions(ui, ed, code, gd);
                                if ui.small_button("Copy").on_hover_text("Copy the Diagram (.kdiagram JSON)").clicked() {
                                    ui.ctx().copy_text(code.to_owned());
                                }
                            });
                        }
                        Err(why) => {
                            ui.label(RichText::new("Diagram").strong());
                            ui.label(RichText::new(if face.is_some() { "Rex's Diagram needs fixing. Ask him to try again." } else { "The generated diagram is invalid. Try again." }).size(12.0).color(WARN())).on_hover_text(why);
                            if ui.small_button("Copy").clicked() {
                                ui.ctx().copy_text(code.to_owned());
                            }
                        }
                    }
                });
            });
        });
    ui.add_space(4.0);
}

fn unique_path(ed: &Editor, dir: &str, stem: &str, ext: &str) -> String {
    let Some(p) = &ed.project else { return format!("{dir}/{stem}.{ext}") };
    let mut path = format!("{dir}/{stem}.{ext}");
    for n in 2.. {
        if !p.resources.exists(&path) {
            break;
        }
        path = format!("{dir}/{stem}_{n}.{ext}");
    }
    path
}

fn script_actions(ui: &mut Ui, ed: &mut Editor, code: &str) {
    match kit_rex::check_script(code) {
        Ok(class) => {
            ui.label(RichText::new("valid x").size(11.0).color(Color32::from_rgb(98, 214, 160)));
            let stem = kit_rex::script_file_stem(class.as_deref().unwrap_or("rex_script"));
            let selected = ed.selected.filter(|e| ed.world().is_alive(*e));
            let label = match selected {
                Some(e) => format!("Add to {}", ed.entity_label(e)),
                None => "Save script".into(),
            };
            if ui.add_enabled(ed.project.is_some() && !ed.playing, egui::Button::new(label)).on_hover_text("Saves it in scripts/ and attaches it to the selected object").clicked() {
                let path = unique_path(ed, "scripts", &stem, "x");
                let result = ed.project.as_ref().unwrap().resources.write_text(&path, &format!("{}\n", code.trim_end()));
                match result {
                    Ok(()) => {
                        ed.refresh_assets(true);
                        if let Some(e) = selected {
                            ed.app.world.insert(e, kit::gamesys::Script::new(path.clone()));
                        }
                        ed.open_script(&path);
                        ed.notify(if ed.rex_friendly() { format!("Rex fetched {path} 🐾") } else { format!("Saved script {path}") });
                    }
                    Err(e) => ed.report(Err(e)),
                }
            }
        }
        Err(why) => {
            ui.label(RichText::new("⚠ doesn't parse").size(11.0).color(WARN())).on_hover_text(why);
        }
    }
}

fn diagram_actions(ui: &mut Ui, ed: &mut Editor, code: &str, gd: &kit::gamedata::GameData) {
    match kit_rex::check_diagram(code, gd) {
        Ok(graph) => {
            let Some(e) = ed.selected.filter(|e| ed.world().is_alive(*e)) else {
                ui.label(RichText::new("Select an object to use it").size(11.0).color(MUTED()));
                return;
            };
            let name = ed.entity_label(e);
            if ui.add_enabled(ed.project.is_some() && !ed.playing, egui::Button::new(format!("Use on {name}"))).on_hover_text("Saves it in diagrams/, attaches it, and opens the Diagram view").clicked() {
                let stem: String = name.chars().map(|c| if c.is_alphanumeric() { c.to_ascii_lowercase() } else { '_' }).collect();
                let path = unique_path(ed, "diagrams", if stem.trim_matches('_').is_empty() { "rex" } else { &stem }, "kdiagram");
                match ed.project.as_ref().unwrap().resources.write_text(&path, &graph.to_json()) {
                    Ok(()) => {
                        ed.app.world.insert(e, kit::gamesys::Diagram { graph: path.clone() });
                        ed.refresh_assets(true);
                        ed.settings.diagram = true;
                        ed.notify(if ed.rex_friendly() { format!("Rex dug up {path} 🐾 (press 7 to see it)") } else { format!("Saved diagram {path}") });
                    }
                    Err(err) => ed.report(Err(err)),
                }
            }
        }
        Err(why) => {
            ui.label(RichText::new("⚠ needs fixing").size(11.0).color(WARN())).on_hover_text(why);
        }
    }
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_mode_keeps_the_mode_id_and_strips_emoji_from_prose() {
        let mode = kit_rex::builtin_modes().into_iter().find(|mode| mode.id == "sniff").unwrap();
        assert_eq!(mode_label(&mode, true), "Sniff");
        assert_eq!(mode_label(&mode, false), "Debugging");
        assert_eq!(plain_rex_text("Check the café script 🐾✨ — now"), "Check the café script  — now");
    }
}
