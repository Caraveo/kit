//! # kit_rex
//!
//! Rex is Kit's AI puppy. This module talks to the Kit AI service
//! (`https://api.kitgameengine.win/api/v1`, overridable with `KIT_REX_API`):
//!
//! * **Sign-in** with a device code: Kit shows a short code, you approve it in
//!   the browser, and Kit receives an API token ([`Client::start_device`],
//!   [`Client::poll_device`]). The token is kept in [`TokenStore`].
//! * **Modes** (Buddy, Fetch, Sniff, Dig, Zoomies, Sit & Learn) and the
//!   account's plan and credits ([`Client::modes`], [`Client::me`]).
//! * **Chat** with streamed replies ([`Client::chat`]).
//! * **Snippets**: the x scripts and Diagrams in Rex's replies, checked with
//!   Kit's own parser and validator before anything is added to a project
//!   ([`snippets`], [`check_script`], [`check_diagram`]).
//!
//! It is UI-agnostic and blocking; [`Worker`] runs requests on background
//! threads and hands results back through a channel for the editor.

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::time::Duration;

/// The Kit AI service.
pub const DEFAULT_API: &str = "https://api.kitgameengine.win/api/v1";

/// The same service on its Cloudflare workers.dev address (used when the main
/// address can't be reached, e.g. while new DNS spreads).
pub const FALLBACK_API: &str = "https://kit-ai.dios-39b.workers.dev/api/v1";

/// The API base URL (`KIT_REX_API` overrides it, e.g. for a local `wrangler dev`).
pub fn api_base() -> String {
    std::env::var("KIT_REX_API").ok().filter(|s| !s.trim().is_empty()).unwrap_or_else(|| DEFAULT_API.to_owned())
}

// ------------------------------------------------------------------ API types

#[derive(Clone, Debug, Deserialize)]
pub struct DeviceStart {
    pub device_code: String,
    pub user_code: String,
    pub verification_uri: String,
    pub verification_uri_complete: String,
    pub expires_in: u64,
    pub interval: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DevicePoll {
    /// Not approved yet: poll again after the interval.
    Pending,
    /// Polling too fast.
    SlowDown,
    /// Approved: the API token.
    Token(String),
    Denied,
    Expired,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
pub struct Balance {
    pub plan: String,
    pub monthly: i64,
    pub topup: i64,
    pub total: i64,
    #[serde(rename = "periodEnd", default)]
    pub period_end: i64,
}

#[derive(Clone, Debug, Deserialize)]
pub struct MeUser {
    pub id: String,
    pub email: String,
    pub name: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct MePlan {
    pub id: String,
    pub name: String,
    pub models: Vec<String>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct MeLinks {
    pub account: String,
    pub pricing: String,
}

#[derive(Clone, Debug, Deserialize)]
pub struct Me {
    pub user: MeUser,
    pub plan: MePlan,
    pub balance: Balance,
    pub links: MeLinks,
}

/// One of Rex's modes, as the service describes it.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct ModeInfo {
    pub id: String,
    pub name: String,
    pub tagline: String,
    pub mood: String,
    pub greeting: String,
    pub examples: Vec<String>,
}

/// The modes built into this version of Kit (used until the service answers).
pub fn builtin_modes() -> Vec<ModeInfo> {
    let m = |id: &str, name: &str, tagline: &str, mood: &str, greeting: &str, examples: &[&str]| ModeInfo {
        id: id.into(),
        name: name.into(),
        tagline: tagline.into(),
        mood: mood.into(),
        greeting: greeting.into(),
        examples: examples.iter().map(|s| s.to_string()).collect(),
    };
    vec![
        m(
            "buddy",
            "Buddy",
            "Chat about anything in your game",
            "happy",
            "Hi! I'm Rex 🐾 Ask me anything about your game.",
            &["How do I make a door open when the player gets close?"],
        ),
        m(
            "fetch",
            "Fetch",
            "Fetches you a ready-to-use x script",
            "fetch",
            "Throw me an idea and I'll fetch you a script! 🐾",
            &["A player that moves with WASD and jumps with Space"],
        ),
        m(
            "sniff",
            "Sniff",
            "Sniffs out bugs and explains errors",
            "sniff",
            "*sniff sniff* Paste the error or the script and I'll track it down 🔍",
            &["My object falls through the floor"],
        ),
        m(
            "dig",
            "Dig",
            "Digs up a Diagram: visual logic, no code",
            "dig",
            "Ooh, let me dig! ✨ Describe what should happen and I'll build a Diagram.",
            &["When the player touches this, turn it green and float up"],
        ),
        m(
            "zoomies",
            "Zoomies",
            "Zooms through game ideas with you",
            "zoomies",
            "ZOOMIES TIME! ⚡ What kind of game are you dreaming about?",
            &["Give me 5 tiny game ideas I can build in a weekend"],
        ),
        m(
            "sit",
            "Sit & Learn",
            "A patient teacher, one step at a time",
            "sit",
            "*sits nicely* Ready to learn together! 🎓",
            &["Teach me x scripting from zero"],
        ),
    ]
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    User,
    Assistant,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Message {
    pub role: Role,
    pub content: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct ChatRequest {
    pub mode: String,
    pub messages: Vec<Message>,
    /// A summary of the open project (scene, selection, scripts, errors).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context: Option<String>,
    pub stream: bool,
}

/// What a streamed reply delivers, in order.
#[derive(Clone, Debug, PartialEq)]
pub enum ChatEvent {
    Start { mode: String, mood: String, model: String },
    Delta(String),
    Done { credits: i64, balance: Balance },
}

/// A failed request, with the service's error code (`out_of_credits`,
/// `rate_limited`, `unauthorized`, …) when it sent one.
#[derive(Clone, Debug, PartialEq)]
pub struct RexError {
    pub status: u16,
    pub code: String,
    pub message: String,
    pub retry_after: Option<u64>,
}

impl std::fmt::Display for RexError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for RexError {}

impl RexError {
    fn network(e: impl std::fmt::Display) -> Self {
        Self {
            status: 0,
            code: "network".into(),
            message: format!("Rex can't reach the Kit AI service ({e}). Check your connection."),
            retry_after: None,
        }
    }
}

// ------------------------------------------------------------------ client

/// A blocking client for the Kit AI service.
#[derive(Clone)]
pub struct Client {
    base: String,
    token: Option<String>,
    agent: ureq::Agent,
}

#[derive(Deserialize)]
struct ErrorBody {
    error: ErrorDetail,
}

#[derive(Deserialize)]
struct ErrorDetail {
    code: String,
    message: String,
    #[serde(rename = "retryAfter")]
    retry_after: Option<u64>,
}

impl Client {
    pub fn new(base: impl Into<String>, token: Option<String>) -> Self {
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .http_status_as_error(false)
            .timeout_connect(Some(Duration::from_secs(15)))
            // A long reply can stream for a while.
            .timeout_global(Some(Duration::from_secs(300)))
            .user_agent(concat!("Kit/", env!("CARGO_PKG_VERSION"), " kit_rex"))
            .build()
            .into();
        Self { base: base.into().trim_end_matches('/').to_owned(), token, agent }
    }

    pub fn signed_in(&self) -> bool {
        self.token.is_some()
    }

    fn url(&self, path: &str) -> String {
        format!("{}{path}", self.base)
    }

    fn auth(
        &self,
        req: ureq::RequestBuilder<ureq::typestate::WithBody>,
    ) -> ureq::RequestBuilder<ureq::typestate::WithBody> {
        match &self.token {
            Some(t) => req.header("authorization", &format!("Bearer {t}")),
            None => req,
        }
    }

    fn auth_get(
        &self,
        req: ureq::RequestBuilder<ureq::typestate::WithoutBody>,
    ) -> ureq::RequestBuilder<ureq::typestate::WithoutBody> {
        match &self.token {
            Some(t) => req.header("authorization", &format!("Bearer {t}")),
            None => req,
        }
    }

    /// Turn a non-2xx reply into a [`RexError`].
    fn error_of(mut resp: ureq::http::Response<ureq::Body>) -> RexError {
        let status = resp.status().as_u16();
        let text = resp.body_mut().read_to_string().unwrap_or_default();
        match serde_json::from_str::<ErrorBody>(&text) {
            Ok(b) => {
                RexError { status, code: b.error.code, message: b.error.message, retry_after: b.error.retry_after }
            }
            Err(_) => RexError {
                status,
                code: format!("http_{status}"),
                message: format!("The Kit AI service answered {status}."),
                retry_after: None,
            },
        }
    }

    fn get_json<T: for<'de> Deserialize<'de>>(&self, path: &str) -> Result<T, RexError> {
        let resp = self.auth_get(self.agent.get(self.url(path))).call().map_err(RexError::network)?;
        if !resp.status().is_success() {
            return Err(Self::error_of(resp));
        }
        let mut resp = resp;
        resp.body_mut().read_json::<T>().map_err(RexError::network)
    }

    pub fn modes(&self) -> Result<Vec<ModeInfo>, RexError> {
        #[derive(Deserialize)]
        struct R {
            modes: Vec<ModeInfo>,
        }
        Ok(self.get_json::<R>("/rex/modes")?.modes)
    }

    pub fn me(&self) -> Result<Me, RexError> {
        self.get_json("/me")
    }

    /// Begin sign-in: show `user_code` and open `verification_uri_complete`.
    pub fn start_device(&self, client_name: &str) -> Result<DeviceStart, RexError> {
        let resp = self
            .agent
            .post(self.url("/device/start"))
            .send_json(serde_json::json!({ "client_name": client_name }))
            .map_err(RexError::network)?;
        if !resp.status().is_success() {
            return Err(Self::error_of(resp));
        }
        let mut resp = resp;
        resp.body_mut().read_json().map_err(RexError::network)
    }

    pub fn poll_device(&self, device_code: &str) -> Result<DevicePoll, RexError> {
        let mut resp = self
            .agent
            .post(self.url("/device/token"))
            .send_json(serde_json::json!({ "device_code": device_code }))
            .map_err(RexError::network)?;
        let v: serde_json::Value = resp.body_mut().read_json().map_err(RexError::network)?;
        if let Some(t) = v.get("access_token").and_then(|t| t.as_str()) {
            return Ok(DevicePoll::Token(t.to_owned()));
        }
        Ok(match v.get("error").and_then(|e| e.as_str()).unwrap_or("") {
            "authorization_pending" => DevicePoll::Pending,
            "slow_down" => DevicePoll::SlowDown,
            "access_denied" => DevicePoll::Denied,
            _ => DevicePoll::Expired,
        })
    }

    /// Revoke this token on the service.
    pub fn sign_out(&self) -> Result<(), RexError> {
        let resp = self.auth(self.agent.post(self.url("/logout"))).send_empty().map_err(RexError::network)?;
        if resp.status().is_success() || resp.status().as_u16() == 401 { Ok(()) } else { Err(Self::error_of(resp)) }
    }

    /// Chat with Rex. `on_event` is called for each streamed piece; returning
    /// `false` stops reading (the request is still charged for what was made).
    pub fn chat(&self, req: &ChatRequest, mut on_event: impl FnMut(ChatEvent) -> bool) -> Result<(), RexError> {
        let req = ChatRequest { stream: true, ..req.clone() };
        let resp = self.auth(self.agent.post(self.url("/rex/chat"))).send_json(&req).map_err(RexError::network)?;
        if !resp.status().is_success() {
            return Err(Self::error_of(resp));
        }
        let reader = BufReader::new(resp.into_body().into_reader());
        for line in reader.lines() {
            let line = line.map_err(RexError::network)?;
            let Some(data) = line.strip_prefix("data: ") else { continue };
            match parse_stream_event(data) {
                Some(Ok(ev)) => {
                    let done = matches!(ev, ChatEvent::Done { .. });
                    if !on_event(ev) || done {
                        break;
                    }
                }
                Some(Err(e)) => return Err(e),
                None => {}
            }
        }
        Ok(())
    }
}

/// Parse one `data:` payload of Rex's stream.
pub fn parse_stream_event(data: &str) -> Option<Result<ChatEvent, RexError>> {
    let v: serde_json::Value = serde_json::from_str(data).ok()?;
    if let Some(e) = v.get("error") {
        return Some(Err(RexError {
            status: 200,
            code: e.get("code").and_then(|c| c.as_str()).unwrap_or("error").to_owned(),
            message: e.get("message").and_then(|c| c.as_str()).unwrap_or("Rex got confused. Try again.").to_owned(),
            retry_after: None,
        }));
    }
    if let Some(d) = v.get("delta").and_then(|d| d.as_str()) {
        return Some(Ok(ChatEvent::Delta(d.to_owned())));
    }
    if v.get("start").is_some() {
        let s = |k: &str| v.get(k).and_then(|x| x.as_str()).unwrap_or_default().to_owned();
        return Some(Ok(ChatEvent::Start { mode: s("mode"), mood: s("mood"), model: s("model") }));
    }
    if v.get("done").is_some() {
        let balance = v.get("balance").and_then(|b| serde_json::from_value(b.clone()).ok()).unwrap_or_default();
        return Some(Ok(ChatEvent::Done { credits: v.get("credits").and_then(|c| c.as_i64()).unwrap_or(0), balance }));
    }
    None
}

// ------------------------------------------------------------------ token storage

/// Where the API token lives (a private file in Kit's settings folder).
#[derive(Clone, Debug)]
pub struct TokenStore {
    pub path: PathBuf,
}

impl TokenStore {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    pub fn load(&self) -> Option<String> {
        let t = std::fs::read_to_string(&self.path).ok()?;
        let t = t.trim();
        (t.starts_with("kit_") && t.len() == 44).then(|| t.to_owned())
    }

    pub fn save(&self, token: &str) -> Result<()> {
        if let Some(dir) = self.path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let tmp = self.path.with_extension("tmp");
        {
            #[cfg(unix)]
            let mut f = {
                use std::os::unix::fs::OpenOptionsExt;
                std::fs::OpenOptions::new().write(true).create(true).truncate(true).mode(0o600).open(&tmp)?
            };
            #[cfg(not(unix))]
            let mut f = std::fs::File::create(&tmp)?;
            use std::io::Write;
            f.write_all(token.as_bytes())?;
        }
        std::fs::rename(&tmp, &self.path).context("saving the Rex sign-in")?;
        Ok(())
    }

    pub fn clear(&self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

// ------------------------------------------------------------------ snippets

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SnippetKind {
    /// An x script (```x).
    Script,
    /// A Diagram (```kdiagram or JSON that parses as one).
    Diagram,
    Other,
}

/// A piece of a reply: plain text, or a fenced code block.
#[derive(Clone, Debug, PartialEq)]
pub enum Part {
    Text(String),
    Code { kind: SnippetKind, lang: String, code: String },
}

/// Split a reply into text and code blocks (an unfinished block at the end,
/// while streaming, counts as code).
pub fn parts(reply: &str) -> Vec<Part> {
    let mut out = Vec::new();
    let mut rest = reply;
    while let Some(start) = rest.find("```") {
        let before = &rest[..start];
        if !before.trim().is_empty() {
            out.push(Part::Text(before.trim_matches('\n').to_owned()));
        }
        let after = &rest[start + 3..];
        let (lang, body) = match after.find('\n') {
            Some(nl) => (after[..nl].trim().to_lowercase(), &after[nl + 1..]),
            None => (after.trim().to_lowercase(), ""),
        };
        let (code, next) = match body.find("```") {
            Some(end) => (&body[..end], &body[end + 3..]),
            None => (body, ""),
        };
        let code = code.trim_end_matches('\n').to_owned();
        let kind = match lang.as_str() {
            "x" | "xpp" => SnippetKind::Script,
            "kdiagram" | "diagram" => SnippetKind::Diagram,
            "json" if kit_diagram::Graph::from_json(&code).is_ok() => SnippetKind::Diagram,
            _ => SnippetKind::Other,
        };
        out.push(Part::Code { kind, lang, code });
        rest = next;
    }
    if !rest.trim().is_empty() {
        out.push(Part::Text(rest.trim_matches('\n').to_owned()));
    }
    out
}

/// The x scripts and Diagrams in a reply.
pub fn snippets(reply: &str) -> Vec<(SnippetKind, String)> {
    parts(reply)
        .into_iter()
        .filter_map(|p| match p {
            Part::Code { kind: k @ (SnippetKind::Script | SnippetKind::Diagram), code, .. } => Some((k, code)),
            _ => None,
        })
        .collect()
}

/// Check an x script with Kit's parser. Ok(class name, if any).
pub fn check_script(code: &str) -> Result<Option<String>, String> {
    let program = kit_x::parse(code).map_err(|e| e.to_string())?;
    let class = program.stmts.iter().find_map(|s| match s {
        kit_x::ast::Stmt::Class(c) => Some(c.name.to_string()),
        _ => None,
    });
    Ok(class)
}

/// Check a Diagram: it must parse, and validate with no errors against the
/// project's game data.
pub fn check_diagram(json: &str, gd: &kit_gamedata::GameData) -> Result<kit_diagram::Graph, String> {
    let mut graph = kit_diagram::Graph::from_json(json).map_err(|e| format!("Not a valid Diagram: {e}"))?;
    graph.next_id = graph.next_id.max(graph.nodes.iter().map(|n| n.id).max().unwrap_or(0));
    let errors: Vec<String> = kit_diagram::validate(&graph, gd)
        .into_iter()
        .filter(|d| d.severity == kit_diagram::Severity::Error)
        .map(|d| d.message)
        .collect();
    if errors.is_empty() { Ok(graph) } else { Err(errors.join("\n")) }
}

/// A file name for a script from its class (`SpinningCoin` → `spinning_coin`).
pub fn script_file_stem(class: &str) -> String {
    let mut out = String::new();
    for (i, c) in class.chars().enumerate() {
        if c.is_uppercase() && i > 0 && !out.ends_with('_') {
            out.push('_');
        }
        if c.is_alphanumeric() {
            out.extend(c.to_lowercase());
        } else if !out.ends_with('_') {
            out.push('_');
        }
    }
    let out = out.trim_matches('_').to_owned();
    if out.is_empty() { "rex_script".into() } else { out }
}

// ------------------------------------------------------------------ background worker

/// Results from background requests.
#[derive(Debug)]
pub enum Update {
    Modes(Vec<ModeInfo>),
    Me(Me),
    Device(DeviceStart),
    /// Sign-in finished: the token to store.
    SignedIn(String),
    SignInFailed(String),
    Chat(ChatEvent),
    ChatFailed(RexError),
    ChatEnded,
    Failed(RexError),
    /// The service address that answered (see [`Worker::connect`]).
    Connected(String),
}

/// Runs requests on background threads; poll [`Worker::updates`] each frame.
pub struct Worker {
    tx: Sender<Update>,
    rx: Receiver<Update>,
    pub client: Client,
}

impl Worker {
    pub fn new(client: Client) -> Self {
        let (tx, rx) = channel();
        Self { tx, rx, client }
    }

    pub fn set_token(&mut self, token: Option<String>) {
        self.client = Client::new(self.client.base.clone(), token);
    }

    /// Everything that arrived since the last call.
    pub fn updates(&self) -> Vec<Update> {
        self.rx.try_iter().collect()
    }

    fn spawn(&self, f: impl FnOnce(Client, Sender<Update>) + Send + 'static) {
        let (c, tx) = (self.client.clone(), self.tx.clone());
        std::thread::Builder::new().name("rex".into()).spawn(move || f(c, tx)).ok();
    }

    /// Find a service address that answers (the main one, else the fallback),
    /// then fetch the modes, and the account when signed in.
    pub fn connect(&self) {
        let explicit = std::env::var("KIT_REX_API").is_ok();
        self.spawn(move |c, tx| {
            let mut bases = vec![c.base.clone()];
            if !explicit && c.base == DEFAULT_API {
                bases.push(FALLBACK_API.to_owned());
            }
            for base in bases {
                let probe = Client { base: base.clone(), ..c.clone() };
                if let Ok(modes) = probe.modes() {
                    let _ = tx.send(Update::Connected(base));
                    let _ = tx.send(Update::Modes(modes));
                    if probe.signed_in() {
                        let _ = tx.send(match probe.me() {
                            Ok(me) => Update::Me(me),
                            Err(e) => Update::Failed(e),
                        });
                    }
                    return;
                }
            }
            let _ = tx.send(Update::Failed(RexError::network("no answer")));
        });
    }

    /// Use a different service address (after [`Update::Connected`]).
    pub fn set_base(&mut self, base: String) {
        self.client = Client::new(base, self.client.token.clone());
    }

    pub fn fetch_modes(&self) {
        self.spawn(|c, tx| {
            if let Ok(m) = c.modes() {
                let _ = tx.send(Update::Modes(m));
            }
        });
    }

    pub fn fetch_me(&self) {
        self.spawn(|c, tx| {
            let _ = tx.send(match c.me() {
                Ok(me) => Update::Me(me),
                Err(e) => Update::Failed(e),
            });
        });
    }

    /// Start the device sign-in and keep polling until it's approved, denied or expired.
    pub fn sign_in(&self, client_name: String) {
        self.spawn(move |c, tx| {
            let start = match c.start_device(&client_name) {
                Ok(s) => s,
                Err(e) => {
                    let _ = tx.send(Update::SignInFailed(e.message));
                    return;
                }
            };
            let _ = tx.send(Update::Device(start.clone()));
            let mut interval = start.interval.max(2);
            let deadline = std::time::Instant::now() + Duration::from_secs(start.expires_in);
            while std::time::Instant::now() < deadline {
                std::thread::sleep(Duration::from_secs(interval));
                match c.poll_device(&start.device_code) {
                    Ok(DevicePoll::Pending) => {}
                    Ok(DevicePoll::SlowDown) => interval += 2,
                    Ok(DevicePoll::Token(t)) => {
                        let _ = tx.send(Update::SignedIn(t));
                        return;
                    }
                    Ok(DevicePoll::Denied) => {
                        let _ = tx.send(Update::SignInFailed("Sign-in was cancelled.".into()));
                        return;
                    }
                    Ok(DevicePoll::Expired) => break,
                    Err(_) => {} // network blip: keep trying until the deadline
                }
            }
            let _ = tx.send(Update::SignInFailed("The sign-in code expired. Try again.".into()));
        });
    }

    pub fn sign_out(&self) {
        self.spawn(|c, _| {
            let _ = c.sign_out();
        });
    }

    /// Send a chat; replies arrive as [`Update::Chat`] events, then [`Update::ChatEnded`].
    pub fn chat(&self, req: ChatRequest, cancel: std::sync::Arc<std::sync::atomic::AtomicBool>) {
        self.spawn(move |c, tx| {
            let r = c.chat(&req, |ev| {
                let _ = tx.send(Update::Chat(ev));
                !cancel.load(std::sync::atomic::Ordering::Relaxed)
            });
            if let Err(e) = r {
                let _ = tx.send(Update::ChatFailed(e));
            }
            let _ = tx.send(Update::ChatEnded);
        });
    }
}

/// Build the project summary Rex sees (kept short; the editor fills it).
pub fn context_block(sections: &[(&str, String)], max_chars: usize) -> Option<String> {
    let mut out = String::new();
    for (title, body) in sections {
        if body.trim().is_empty() {
            continue;
        }
        out.push_str(&format!("## {title}\n{}\n\n", body.trim()));
    }
    if out.is_empty() {
        return None;
    }
    if out.len() > max_chars {
        let mut cut = max_chars;
        while !out.is_char_boundary(cut) {
            cut -= 1;
        }
        out.truncate(cut);
        out.push_str("\n…");
    }
    Some(out)
}

/// Fail early with a clear message when a base URL is obviously wrong.
pub fn check_base(base: &str) -> Result<()> {
    if !(base.starts_with("https://") || base.starts_with("http://127.0.0.1") || base.starts_with("http://localhost")) {
        bail!("Rex needs an https:// service address");
    }
    Ok(())
}

#[cfg(test)]
mod tests;
