//! Kit Multiplayer Circuit: Realtime SDK.
//!
//! Multiplayer without servers: each game project's world is a Realtime
//! scope, divided into sectors (one per scene, instanced when full). Clients
//! render and simulate; they stream what changed (transforms, animation
//! state, shared state, world modifications) and the scopes relay it to the
//! clients that need it. See `../README.md` and `../PROTOCOL.md`.
//!
//! - [`protocol`]: the binary wire format (matches the Worker byte for byte).
//! - [`client`]: one non-blocking connection per scope.
//! - [`event`]: decoded scope messages.
//! - [`sync`]: per-component network policies, the Realtime Sync component,
//!   dirty tracking and interpolation.
//! - [`world`]: seed + diff world cache with chunk hashes.
//! - [`Realtime`]: world + current sector, with sector transitions.

pub mod client;
pub mod event;
pub mod protocol;
pub mod sync;
pub mod world;

use client::{Connection, Status, Target, TokenSource};
use event::Event;
use protocol::{NetTransform, Value, Writer, c};

/// Where a game's Realtime lives.
#[derive(Clone, Debug)]
pub struct Config {
    /// `wss://circuit.kitgameengine.win/api/v1/realtime/connect`, or a local relay.
    pub connect_url: String,
    pub environment: String,
    /// The world's id: scopes are `world/<id>` and `world/<id>/sector/<scene>`.
    pub world: String,
    pub party: Option<String>,
}

/// The sector scope for a scene file ("scenes/level2.kscene" → "world/main/sector/scenes/level2").
pub fn sector_scope(world: &str, scene_path: &str) -> String {
    let stem = scene_path.trim_start_matches("./").trim_end_matches(".kscene").replace('\\', "/");
    let clean: String =
        stem.chars()
            .map(|c| {
                if c.is_ascii_alphanumeric() || c == '/' || c == '-' || c == '_' { c.to_ascii_lowercase() } else { '-' }
            })
            .collect();
    format!("world/{world}/sector/{clean}")
}

/// Which connection a message or event belongs to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Channel {
    World,
    Sector,
}

/// A game's connection to its world and current sector.
pub struct Realtime {
    cfg: Config,
    tokens: TokenSource,
    world: Connection,
    sector: Option<(String, Connection)>,
    /// The next sector, connected while the scene loads; swapped in once ready.
    entering: Option<(String, Connection, bool)>,
    frames: [Writer; 2],
    next_request: u16,
}

impl Realtime {
    #[cfg(not(target_arch = "wasm32"))]
    pub fn connect(cfg: Config, tokens: TokenSource) -> Self {
        let world = Connection::open(
            Target {
                base: cfg.connect_url.clone(),
                environment: cfg.environment.clone(),
                scope: format!("world/{}", cfg.world),
                auto_instance: false,
                party: None,
            },
            tokens.clone(),
        );
        Self {
            cfg,
            tokens,
            world,
            sector: None,
            entering: None,
            frames: [Writer::new(), Writer::new()],
            next_request: 1,
        }
    }

    /// Start moving to a scene's sector. The current sector keeps streaming
    /// until the new one is ready (see [`Realtime::poll`]), so there's no gap
    /// and no duplicate entities.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn enter_scene(&mut self, scene_path: &str) {
        let scope = sector_scope(&self.cfg.world, scene_path);
        if self.sector.as_ref().is_some_and(|(s, _)| s.starts_with(&scope)) {
            return;
        }
        let conn = Connection::open(
            Target {
                base: self.cfg.connect_url.clone(),
                environment: self.cfg.environment.clone(),
                scope: scope.clone(),
                auto_instance: true,
                party: self.cfg.party.clone(),
            },
            self.tokens.clone(),
        );
        self.entering = Some((scope, conn, false));
    }

    fn frame(&mut self, ch: Channel) -> &mut Writer {
        &mut self.frames[ch as usize]
    }

    pub fn spawn(&mut self, ch: Channel, kind: &str, flags: u8, t: &NetTransform) -> u16 {
        let req = self.next_request;
        self.next_request = self.next_request.wrapping_add(1).max(1);
        self.frame(ch).u8(c::SPAWN).u16(req).u8(flags).str8(kind).transform(t);
        req
    }
    pub fn despawn(&mut self, ch: Channel, id: u32) {
        self.frame(ch).u8(c::DESPAWN).u32(id);
    }
    pub fn transform(&mut self, ch: Channel, id: u32, t: &NetTransform) {
        self.frame(ch).u8(c::TRANSFORM).u32(id).transform(t);
    }
    pub fn anim(&mut self, ch: Channel, id: u32, a: &protocol::NetAnim) {
        let w = self.frame(ch);
        w.u8(c::ANIM).u32(id);
        a.encode(w);
    }
    /// Change shared state. `expect` = the revision you last saw (or [`protocol::ANY_REVISION`]).
    pub fn set_state(&mut self, ch: Channel, key: &str, value: &Value, expect: u32) {
        self.frame(ch).u8(c::STATE_SET).u32(expect).str8(key).value(value);
    }
    pub fn emit(&mut self, ch: Channel, name: &str, payload: &[u8]) {
        self.frame(ch).u8(c::EVENT).str8(name).bytes16(payload);
    }
    /// Where this player is looking from (drives spatial interest).
    pub fn interest(&mut self, pos: glam::Vec3, radius: f32) {
        self.frame(Channel::Sector).u8(c::INTEREST).f32(pos.x).f32(pos.y).f32(pos.z).f32(radius);
    }
    pub fn claim(&mut self, ch: Channel, id: u32) {
        self.frame(ch).u8(c::CLAIM).u32(id);
    }
    /// Change the world (collect, place, sculpt). `expect` 0 = only if nobody changed it yet.
    pub fn modify(&mut self, ch: Channel, cx: i16, cz: i16, key: &str, value: &Value, expect: u32) {
        self.frame(ch).u8(c::WORLD_MOD).i16(cx).i16(cz).u32(expect).str8(key).value(value);
    }
    /// Watch these chunks, telling the scope which hashes are already cached.
    pub fn watch_chunks(&mut self, ch: Channel, chunks: &[((i16, i16), u64)]) {
        let w = self.frame(ch);
        w.u8(c::CHUNKS).u16(chunks.len().min(256) as u16);
        for ((cx, cz), h) in chunks.iter().take(256) {
            w.i16(*cx).i16(*cz).u64(*h);
        }
    }
    /// Set this scope's world seed (once; or replace with `expect` = current world revision).
    pub fn set_seed(&mut self, ch: Channel, seed: &protocol::WorldDescriptor, expect: u32) {
        self.frame(ch)
            .u8(c::WORLD_DESC)
            .u32(expect)
            .str8(&seed.id)
            .f64(seed.seed)
            .str8(&seed.generator)
            .str8(&seed.generator_version)
            .u32(seed.chunk_size)
            .str8(&seed.compatibility_hash);
    }
    pub fn ping(&mut self, ch: Channel, now_ms: u32) {
        self.frame(ch).u8(c::PING).u32(now_ms);
    }

    /// Send this frame's batched messages (call once per game frame).
    pub fn flush(&mut self) {
        let [w, s] = &mut self.frames;
        self.world.send(w);
        match &mut self.sector {
            Some((_, conn)) => {
                conn.send(s);
            }
            None => s.buf.clear(),
        }
    }

    /// Events since the last call, tagged with their channel. Completes a
    /// sector transition once the new sector's snapshot has arrived.
    pub fn poll(&mut self) -> Vec<(Channel, Event)> {
        let mut out: Vec<(Channel, Event)> = self.world.poll().into_iter().map(|e| (Channel::World, e)).collect();
        if let Some((_, conn)) = &mut self.sector {
            out.extend(conn.poll().into_iter().map(|e| (Channel::Sector, e)));
        }
        let mut swap = false;
        if let Some((_, conn, ready)) = &mut self.entering {
            for e in conn.poll() {
                if e == Event::Ready {
                    *ready = true;
                }
                if matches!(&e, Event::Status(Status::Closed { .. })) {
                    out.push((Channel::Sector, e));
                    continue;
                }
                out.push((Channel::Sector, e));
            }
            swap = *ready;
        }
        if swap {
            let (scope, conn, _) = self.entering.take().unwrap();
            // Dropping the old connection closes it; its entities leave with Gone/Leave on the scope side.
            self.sector = Some((scope, conn));
        }
        out
    }

    pub fn sector(&self) -> Option<&str> {
        self.sector.as_ref().map(|(s, _)| s.as_str())
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn sector_scopes_follow_scene_paths() {
        assert_eq!(super::sector_scope("main", "scenes/level2.kscene"), "world/main/sector/scenes/level2");
        assert_eq!(super::sector_scope("main", "main.kscene"), "world/main/sector/main");
        assert_eq!(super::sector_scope("main", "Scenes/My Level.kscene"), "world/main/sector/scenes/my-level");
    }
}
