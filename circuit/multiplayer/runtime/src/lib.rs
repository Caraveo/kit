//! The Multiplayer Circuit inside a running Kit game.
//!
//! Dormant unless the project has `kit.multiplayer` installed and enabled
//! (circuits.lock) and an environment set (Project → Multiplayer…). Then:
//!
//! - it joins the world and the current scene's sector, and follows
//!   `Scene.Load` to the next sector;
//! - every object with **Realtime Sync** is shared:
//!   - **One per player** objects (player characters): each game owns its
//!     own; other players appear as copies of it (without scripts or
//!     cameras, moved smoothly by the network);
//!   - other objects are **shared**: whoever arrives first owns them, everyone
//!     else follows. If the owner leaves, the next player takes over.
//! - owners send only what changed; followers glide toward the latest
//!   transform (physics on followers is kinematic: see networked-physics.md).

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;

use glam::Vec3;
use kit_core::{App, Console, Module, Stage};
use kit_ecs::{Entity, World};
use kit_gamesys::ExtraData;
use kit_realtime::event::{Event, Presence};
use kit_realtime::protocol::{ENTITY_UNIQUE, NetTransform};
use kit_realtime::sync::{DirtyTracker, Interpolator, RealtimeSync};
use kit_realtime::{Channel, Config, Realtime};
use kit_scene::Transform;

pub const CIRCUIT_ID: &str = "kit.multiplayer";
const SOURCE: &str = "multiplayer";

pub struct MultiplayerModule;

impl Module for MultiplayerModule {
    fn name(&self) -> &'static str {
        "multiplayer"
    }
    fn dependencies(&self) -> &'static [&'static str] {
        &["gamesys"]
    }
    fn build(&self, app: &mut App) {
        app.world.insert_resource(Multiplayer::default());
        app.add_play_system(Stage::PreUpdate, "multiplayer_receive", receive);
        app.add_play_system(Stage::PostUpdate, "multiplayer_send", send);
        // After scene_requests (gamesys registers it first), so Scene.Load is seen this frame.
        app.add_play_system(Stage::Last, "multiplayer_scenes", scenes);
    }
}

/// The project's Multiplayer settings (circuits.settings.toml → [kit.multiplayer]).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Settings {
    pub environment: String,
    pub world: String,
    pub tokens: String,
    pub token_url: String,
    pub connect_url: String,
}

impl Settings {
    pub fn parse(text: &str) -> Self {
        let t: toml::Table = toml::from_str(text).unwrap_or_default();
        let m = t.get(CIRCUIT_ID).and_then(|v| v.as_table()).cloned().unwrap_or_default();
        let s = |k: &str, d: &str| m.get(k).and_then(|v| v.as_str()).filter(|s| !s.trim().is_empty()).unwrap_or(d).trim().to_owned();
        Self {
            environment: s("environment", ""),
            world: s("world", "main"),
            tokens: s("tokens", "anonymous"),
            token_url: s("token_url", ""),
            connect_url: s("connect_url", "wss://circuit.kitgameengine.win/api/v1/realtime/connect"),
        }
    }

    /// The Circuit API base for this Realtime address (`wss://h/api/v1/realtime/connect` → `https://h/api/v1`).
    pub fn api_base(&self) -> String {
        let http = self.connect_url.replacen("wss://", "https://", 1).replacen("ws://", "http://", 1);
        http.trim_end_matches('/').trim_end_matches("/realtime/connect").to_owned()
    }
}

/// Is Multiplayer installed and enabled in this project's circuits.lock?
pub fn enabled_in_lock(text: &str) -> bool {
    let t: toml::Table = toml::from_str(text).unwrap_or_default();
    t.get("circuit").and_then(|v| v.as_array()).is_some_and(|list| {
        list.iter().any(|c| c.get("id").and_then(|v| v.as_str()) == Some(CIRCUIT_ID) && c.get("enabled").and_then(|v| v.as_bool()).unwrap_or(true))
    })
}

/// A random session id for this game process (keeps ownership across reconnects).
fn session_id() -> String {
    use std::hash::{BuildHasher, RandomState};
    let a = RandomState::new().hash_one(std::process::id());
    let b = RandomState::new().hash_one(Instant::now());
    format!("p-{a:016x}{b:08x}")
}

fn token_source(s: &Settings, sid: String) -> kit_realtime::client::TokenSource {
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(std::time::Duration::from_secs(15)))
        .user_agent(concat!("Kit/", env!("CARGO_PKG_VERSION"), " kit_multiplayer"))
        .build()
        .into();
    let (env, backend) = (s.environment.clone(), s.tokens == "backend");
    let url = if backend { s.token_url.clone() } else { format!("{}/multiplayer/tokens/anonymous", s.api_base()) };
    Arc::new(move || {
        if url.is_empty() {
            anyhow::bail!("set the backend token URL in Project → Multiplayer");
        }
        let mut resp = agent.post(&url).send_json(serde_json::json!({ "environment": env, "sid": sid }))?;
        let v: serde_json::Value = resp.body_mut().read_json()?;
        v.get("token").and_then(|t| t.as_str()).map(str::to_owned).ok_or_else(|| anyhow::anyhow!("{}", v.get("error").and_then(|e| e.get("message")).and_then(|m| m.as_str()).unwrap_or("no token in the reply")))
    })
}

// ------------------------------------------------------------------ state

#[derive(Default)]
enum Phase {
    #[default]
    Unchecked,
    Off,
    On(Box<Realtime>),
}

/// One of this game's synced objects.
struct Local {
    cfg: RealtimeSync,
    kind: String,
    net: Option<u32>,
    pending: Option<u16>,
    /// Someone else owns it: we follow (its scripts are parked here).
    following: bool,
    parked_script: Option<kit_gamesys::Script>,
    /// Its body kind before following (restored when we take it over).
    parked_body: Option<kit_gamesys::BodyKind>,
    dirty: DirtyTracker,
    last_pos: Option<(Vec3, f64)>,
}

/// An object moved by the network: another player's copy, or a shared object we follow.
struct Remote {
    entity: Entity,
    interp: Interpolator,
    extrapolate: bool,
    /// We created it (another player's copy) and remove it when they leave.
    clone: bool,
}

#[derive(Default)]
pub struct Multiplayer {
    phase: Phase,
    start: Option<Instant>,
    session: Option<u32>,
    ready: bool,
    scene: String,
    locals: HashMap<Entity, Local>,
    remotes: HashMap<u32, Remote>,
    requests: HashMap<u16, Entity>,
    last_interest: f64,
    status: String,
}

impl Multiplayer {
    /// What a game or the editor can show ("Connected · 3 players", "Off: …").
    pub fn status(&self) -> &str {
        &self.status
    }
    fn now(&mut self) -> f64 {
        self.start.get_or_insert_with(Instant::now).elapsed().as_secs_f64()
    }
}

fn say(world: &World, text: impl Into<String>) {
    let text = text.into();
    log::info!("multiplayer: {text}");
    if let Some(mut c) = world.try_resource_mut::<Console>() {
        c.info(SOURCE, text);
    }
}

fn warn(world: &World, text: impl Into<String>) {
    let text = text.into();
    log::warn!("multiplayer: {text}");
    if let Some(mut c) = world.try_resource_mut::<Console>() {
        c.warn(SOURCE, text);
    }
}

fn current_scene(world: &World) -> Option<String> {
    world.try_resource::<kit_gamesys::scenes::LoadedScenes>().and_then(|l| l.current().filter(|p| !p.is_empty()).map(str::to_owned))
}

fn start(world: &mut World, mp: &mut Multiplayer) {
    let Some(res) = world.try_resource::<kit_resource::Resources>().map(|r| r.clone()) else {
        mp.phase = Phase::Off;
        return;
    };
    if !res.read_text("circuits.lock").is_ok_and(|t| enabled_in_lock(&t)) {
        mp.phase = Phase::Off;
        return;
    }
    let settings = Settings::parse(&res.read_text(kit_settings_file()).unwrap_or_default());
    if settings.environment.is_empty() {
        mp.status = "Off: set the environment id in Project → Multiplayer".into();
        warn(world, "Multiplayer is installed but has no environment. Set it in Project → Multiplayer (from your Kit account → Circuits → Multiplayer).");
        mp.phase = Phase::Off;
        return;
    }
    let cfg = Config { connect_url: settings.connect_url.clone(), environment: settings.environment.clone(), world: settings.world.clone(), party: None };
    let mut rt = Realtime::connect(cfg, token_source(&settings, session_id()));
    // Scene path: the loaded scene, or the project's main scene (editor Play).
    let scene = current_scene(world)
        .or_else(|| res.read_text("kit.project").ok().and_then(|t| toml::from_str::<toml::Table>(&t).ok()).and_then(|t| t.get("project")?.get("main_scene")?.as_str().map(str::to_owned)))
        .unwrap_or_else(|| "main.kscene".into());
    rt.enter_scene(&scene);
    mp.scene = scene;
    mp.status = "Connecting…".into();
    say(world, format!("connecting to world/{} ({})", settings.world, mp.scene));
    mp.phase = Phase::On(Box::new(rt));
}

fn kit_settings_file() -> &'static str {
    "circuits.settings.toml"
}

// ------------------------------------------------------------------ systems

fn receive(world: &mut World) {
    let Some(mut mp) = world.remove_resource::<Multiplayer>() else { return };
    if matches!(mp.phase, Phase::Unchecked) {
        start(world, &mut mp);
    }
    let now = mp.now();
    let events = match &mut mp.phase {
        Phase::On(rt) => rt.poll(),
        _ => Vec::new(),
    };
    for (ch, e) in events {
        if ch == Channel::Sector {
            sector_event(world, &mut mp, e, now);
        } else if let Event::Status(kit_realtime::client::Status::Closed { reason }) = e {
            mp.status = format!("Disconnected: {reason}");
            warn(world, format!("world connection closed: {reason}"));
        }
    }
    // Followers glide toward the network transform.
    for r in mp.remotes.values() {
        if let Some((pos, rot)) = r.interp.sample(now, r.extrapolate) {
            if let Some(mut t) = world.get_mut::<Transform>(r.entity) {
                t.translation = pos;
                t.rotation = rot;
                if let Some(s) = r.interp.latest().scale {
                    t.scale = s;
                }
            }
        }
    }
    world.insert_resource(mp);
}

fn sector_event(world: &mut World, mp: &mut Multiplayer, e: Event, now: f64) {
    match e {
        Event::Welcome { scope, session, .. } => {
            // New (or re-joined) sector: forget the old one's mapping.
            clear_remotes(world, mp);
            for l in mp.locals.values_mut() {
                l.net = None;
                l.pending = None;
            }
            mp.requests.clear();
            mp.session = Some(session);
            mp.ready = false;
            say(world, format!("joined {scope}"));
        }
        Event::Ready => {
            mp.ready = true;
            mp.status = "Connected".into();
        }
        Event::Entity { id, owner, kind, transform, .. } => on_entity(world, mp, id, owner, &kind, &transform, now),
        Event::Spawned { request, id } => {
            if let Some(e) = mp.requests.remove(&request) {
                if let Some(l) = mp.locals.get_mut(&e) {
                    l.pending = None;
                    // Ours unless the ENTITY that follows says someone else owns it.
                    if !l.following {
                        l.net = Some(id);
                    }
                }
            }
        }
        Event::Transform { id, transform } => {
            if let Some(r) = mp.remotes.get_mut(&id) {
                r.interp.push(now, &transform);
            }
        }
        Event::Gone { id } => {
            if let Some(r) = mp.remotes.remove(&id) {
                if r.clone {
                    kit_scene::despawn_recursive(world, r.entity);
                } else if let Some(l) = mp.locals.get_mut(&r.entity) {
                    // The shared object's owner left (or it left our interest): take it over.
                    unfollow(world, r.entity, l);
                }
            }
        }
        Event::Owner { id, owner } => {
            let mine = Some(owner) == mp.session;
            if mine {
                if let Some(r) = mp.remotes.remove(&id) {
                    if let Some(l) = mp.locals.get_mut(&r.entity) {
                        unfollow(world, r.entity, l);
                        l.net = Some(id);
                    }
                }
            }
        }
        Event::Presence { kind, session, .. } => {
            let what = match kind {
                Presence::Join => "joined",
                Presence::Leave => "left",
                Presence::Reconnect => "reconnected",
            };
            say(world, format!("player {session} {what}"));
        }
        Event::Error { message, .. } => warn(world, message),
        Event::Status(kit_realtime::client::Status::Closed { reason }) => {
            mp.status = format!("Disconnected: {reason}");
            warn(world, format!("sector connection closed: {reason}"));
        }
        Event::Status(kit_realtime::client::Status::Reconnecting { reason, in_ms, .. }) => {
            mp.status = "Reconnecting…".into();
            log::info!("multiplayer: reconnecting in {in_ms} ms ({reason})");
        }
        _ => {}
    }
}

fn on_entity(world: &mut World, mp: &mut Multiplayer, id: u32, owner: u32, kind: &str, t: &NetTransform, now: f64) {
    let mine = Some(owner) == mp.session;
    // One of ours (a reconnect, or the answer to our spawn).
    if let Some((&e, l)) = mp.locals.iter_mut().find(|(_, l)| l.kind == kind && (l.net == Some(id) || l.pending.is_some() || l.net.is_none())) {
        if mine {
            l.net = Some(id);
            return;
        }
        if kind.starts_with("scene:") {
            // A shared object someone else owns: follow it.
            l.net = Some(id);
            l.pending = None;
            if !l.following {
                follow(world, e, l);
            }
            let mut r = Remote { entity: e, interp: Interpolator::default(), extrapolate: l.cfg.extrapolate, clone: false };
            r.interp.push(now, t);
            mp.remotes.insert(id, r);
            return;
        }
    }
    if mine || mp.remotes.contains_key(&id) {
        return;
    }
    // Another player's copy of a one-per-player object.
    if let Some(name) = kind.strip_prefix("player:") {
        let Some(template) = mp.locals.iter().find(|(_, l)| l.kind == kind).map(|(e, _)| *e).or_else(|| kit_scene::find_by_name(world, name)) else {
            warn(world, format!("another player has `{name}`, but this scene has no object with that name"));
            return;
        };
        match kit_gamesys::clone_subtree(world, template) {
            Ok(copy) => {
                strip_copy(world, copy);
                if let Some(mut n) = world.get_mut::<kit_scene::Name>(copy) {
                    n.0 = format!("{name} (player {owner})");
                }
                let mut r = Remote { entity: copy, interp: Interpolator::default(), extrapolate: false, clone: true };
                r.interp.push(now, t);
                mp.remotes.insert(id, r);
            }
            Err(err) => warn(world, format!("couldn't create another player's {name}: {err:#}")),
        }
    }
}

/// Another player's copy: no scripts, cameras or listeners; physics follows the network.
fn strip_copy(world: &mut World, root: Entity) {
    for e in kit_scene::descendants_and_self(world, root) {
        world.remove::<kit_gamesys::Script>(e);
        world.remove::<kit_gamesys::Camera>(e);
        world.remove::<kit_gamesys::Diagram>(e);
        if let Some(mut x) = world.get_mut::<ExtraData>(e) {
            x.0.remove("realtime");
        }
        if let Some(mut b) = world.get_mut::<kit_gamesys::RigidBody>(e) {
            b.kind = kit_gamesys::BodyKind::Kinematic;
        }
    }
}

fn follow(world: &mut World, e: Entity, l: &mut Local) {
    l.following = true;
    l.parked_script = world.remove::<kit_gamesys::Script>(e);
    if let Some(mut b) = world.get_mut::<kit_gamesys::RigidBody>(e) {
        l.parked_body = Some(b.kind);
        b.kind = kit_gamesys::BodyKind::Kinematic;
    }
}

fn unfollow(world: &mut World, e: Entity, l: &mut Local) {
    l.following = false;
    l.net = None;
    l.dirty = DirtyTracker::default();
    if let Some(s) = l.parked_script.take() {
        world.insert(e, s);
    }
    if let (Some(kind), Some(mut b)) = (l.parked_body.take(), world.get_mut::<kit_gamesys::RigidBody>(e)) {
        b.kind = kind;
    }
}

fn clear_remotes(world: &mut World, mp: &mut Multiplayer) {
    for (_, r) in mp.remotes.drain() {
        if r.clone {
            kit_scene::despawn_recursive(world, r.entity);
        }
    }
}

fn send(world: &mut World) {
    let Some(mut mp) = world.remove_resource::<Multiplayer>() else { return };
    if !matches!(mp.phase, Phase::On(_)) || !mp.ready {
        if let Phase::On(rt) = &mut mp.phase {
            rt.flush();
        }
        world.insert_resource(mp);
        return;
    }
    let now = mp.now();
    // Pick up synced objects (new ones appear when scenes load or scripts add them).
    let clones: std::collections::HashSet<Entity> = mp.remotes.values().filter(|r| r.clone).map(|r| r.entity).collect();
    for e in world.query::<(ExtraData,)>() {
        if mp.locals.contains_key(&e) || clones.contains(&e) {
            continue;
        }
        let Some(cfg) = world.get::<ExtraData>(e).and_then(|x| RealtimeSync::from_extra(&x.0)) else { continue };
        let name = world.get::<kit_scene::Name>(e).map(|n| n.0.clone()).unwrap_or_default();
        let kind = format!("{}:{name}", if cfg.per_player { "player" } else { "scene" });
        mp.locals.insert(e, Local { cfg, kind, net: None, pending: None, following: false, parked_script: None, parked_body: None, dirty: DirtyTracker::default(), last_pos: None });
    }
    mp.locals.retain(|e, _| world.is_alive(*e));
    let Phase::On(rt) = &mut mp.phase else { unreachable!() };
    let mut interest = None;
    for (&e, l) in mp.locals.iter_mut() {
        if l.following || l.pending.is_some() {
            continue;
        }
        let Some(t) = world.get::<Transform>(e).map(|t| *t) else { continue };
        let vel = match l.last_pos {
            Some((p, at)) if now > at => (t.translation - p) / (now - at) as f32,
            _ => Vec3::ZERO,
        };
        l.last_pos = Some((t.translation, now));
        if l.cfg.per_player && interest.is_none() {
            interest = Some(t.translation);
        }
        match l.net {
            None => {
                let first = NetTransform { position: Some(t.translation), rotation: Some(t.rotation), scale: l.cfg.scale.then_some(t.scale), ..Default::default() };
                let flags = l.cfg.flags() | if l.cfg.per_player { 0 } else { ENTITY_UNIQUE };
                let req = rt.spawn(Channel::Sector, &l.kind, flags, &first);
                l.pending = Some(req);
                mp.requests.insert(req, e);
                l.dirty.update(&l.cfg, now, 20, t.translation, t.rotation, t.scale, vel);
            }
            Some(id) => {
                if let Some(nt) = l.dirty.update(&l.cfg, now, 20, t.translation, t.rotation, t.scale, vel) {
                    rt.transform(Channel::Sector, id, &nt);
                }
            }
        }
    }
    if let Some(p) = interest.filter(|_| now - mp.last_interest > 0.25) {
        rt.interest(p, 0.0);
        mp.last_interest = now;
    }
    rt.flush();
    world.insert_resource(mp);
}

/// Follow Scene.Load into the new scene's sector.
fn scenes(world: &mut World) {
    let loaded: Vec<String> = match world.try_resource::<kit_gamesys::scenes::SceneEvents>() {
        Some(ev) => ev.0.iter().filter_map(|e| match e {
            kit_gamesys::scenes::SceneEvent::Loaded { path, additive: false } => Some(path.clone()),
            _ => None,
        }).collect(),
        None => return,
    };
    let Some(path) = loaded.last().cloned() else { return };
    let Some(mut mp) = world.remove_resource::<Multiplayer>() else { return };
    if let Phase::On(rt) = &mut mp.phase {
        // The old scene's objects are gone; the new scene's are picked up once the sector is ready.
        mp.locals.clear();
        mp.remotes.clear();
        mp.requests.clear();
        mp.ready = false;
        rt.enter_scene(&path);
        mp.scene = path.clone();
        say(world, format!("moving to {path}"));
    }
    world.insert_resource(mp);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_project_settings_and_lock() {
        let s = Settings::parse("[\"kit.multiplayer\"]\nenvironment = \"env_abc\"\nworld = \"forest\"\n");
        assert_eq!(s.environment, "env_abc");
        assert_eq!(s.world, "forest");
        assert_eq!(s.tokens, "anonymous");
        assert_eq!(s.api_base(), "https://circuit.kitgameengine.win/api/v1");
        let local = Settings { connect_url: "ws://127.0.0.1:8787/api/v1/realtime/connect".into(), ..s };
        assert_eq!(local.api_base(), "http://127.0.0.1:8787/api/v1");
        assert!(enabled_in_lock("version = 1\n[[circuit]]\nid = \"kit.multiplayer\"\nversion = \"0.1.1\"\nsha256 = \"x\"\nsource = \"s\"\nenabled = true\n"));
        assert!(!enabled_in_lock("version = 1\n[[circuit]]\nid = \"kit.multiplayer\"\nversion = \"0.1.1\"\nsha256 = \"x\"\nsource = \"s\"\nenabled = false\n"));
        assert!(!enabled_in_lock(""));
        assert_ne!(session_id(), session_id());
    }
}
