//! Decoding scope → client frames into events for the game thread.

use crate::protocol::{NetAnim, NetTransform, ProtocolError, Reader, Value, WorldDescriptor, s};

#[derive(Clone, Debug, PartialEq)]
pub enum Event {
    /// Connected (or reconnected) to a scope; everything that follows until
    /// [`Event::Ready`] is the scope's snapshot.
    Welcome { scope: String, session: u32, state_revision: u32, world_revision: u32, tick_hz: u16, interest_radius: f32, world: Option<WorldDescriptor>, world_hash: u64 },
    Ready,
    Entity { id: u32, owner: u32, flags: u8, kind: String, transform: NetTransform },
    Gone { id: u32 },
    Transform { id: u32, transform: NetTransform },
    Anim { id: u32, anim: NetAnim },
    Owner { id: u32, owner: u32 },
    Spawned { request: u16, id: u32 },
    State { revision: u32, key: String, value: Value, by: u32 },
    Rejected { current_revision: u32, key: String },
    Event { from: u32, name: String, payload: Vec<u8> },
    Presence { kind: Presence, session: u32, player: String },
    Pong { sent_ms: u32 },
    Error { code: u16, message: String },
    WorldMod { revision: u32, cx: i16, cz: i16, key: String, value: Value, by: u32, world_hash: u64 },
    WorldSeed { revision: u32, world: Option<WorldDescriptor>, world_hash: u64 },
    /// Answer to a chunk subscription: `count` modifications follow (0 when the cached hash matched).
    Chunk { cx: i16, cz: i16, hash: u64, count: u32 },
    /// Connection state changes from the client itself (not the scope).
    Status(crate::client::Status),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Presence {
    Join,
    Leave,
    Reconnect,
}

pub fn decode(frame: &[u8], out: &mut Vec<Event>) -> Result<(), ProtocolError> {
    let mut r = Reader::new(frame);
    while !r.done() {
        let t = r.u8()?;
        out.push(match t {
            s::WELCOME => {
                let version = r.u8()?;
                if version != crate::protocol::VERSION {
                    return Err(ProtocolError(format!("scope speaks protocol {version}")));
                }
                Event::Welcome {
                    scope: r.str8()?,
                    session: r.u32()?,
                    state_revision: r.u32()?,
                    world_revision: r.u32()?,
                    tick_hz: r.u16()?,
                    interest_radius: r.f32()?,
                    world: WorldDescriptor::read(&mut r)?,
                    world_hash: r.u64()?,
                }
            }
            s::READY => Event::Ready,
            s::ENTITY => Event::Entity { id: r.u32()?, owner: r.u32()?, flags: r.u8()?, kind: r.str8()?, transform: r.transform()? },
            s::GONE => Event::Gone { id: r.u32()? },
            s::TRANSFORM => Event::Transform { id: r.u32()?, transform: r.transform()? },
            s::ANIM => Event::Anim { id: r.u32()?, anim: NetAnim::decode(&mut r)? },
            s::OWNER => Event::Owner { id: r.u32()?, owner: r.u32()? },
            s::SPAWNED => Event::Spawned { request: r.u16()?, id: r.u32()? },
            s::STATE => Event::State { revision: r.u32()?, key: r.str8()?, value: r.value()?, by: r.u32()? },
            s::STATE_REJECT => Event::Rejected { current_revision: r.u32()?, key: r.str8()? },
            s::EVENT => Event::Event { from: r.u32()?, name: r.str8()?, payload: r.bytes16()? },
            s::PRESENCE => {
                let kind = match r.u8()? {
                    1 => Presence::Join,
                    2 => Presence::Leave,
                    _ => Presence::Reconnect,
                };
                Event::Presence { kind, session: r.u32()?, player: r.str8()? }
            }
            s::PONG => Event::Pong { sent_ms: r.u32()? },
            s::ERROR => Event::Error { code: r.u16()?, message: r.str16()? },
            s::WORLD_MOD => Event::WorldMod { revision: r.u32()?, cx: r.i16()?, cz: r.i16()?, key: r.str8()?, value: r.value()?, by: r.u32()?, world_hash: r.u64()? },
            s::WORLD_DESC => Event::WorldSeed { revision: r.u32()?, world: WorldDescriptor::read(&mut r)?, world_hash: r.u64()? },
            s::CHUNK => Event::Chunk { cx: r.i16()?, cz: r.i16()?, hash: r.u64()?, count: r.u32()? },
            other => return Err(ProtocolError(format!("unknown message type {other:#x}"))),
        });
    }
    Ok(())
}
