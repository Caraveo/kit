# Kit Realtime protocol v1

The binary protocol between game clients and Realtime scopes. The reference
implementations are `sdk/src/protocol.rs` (Rust) and kit-ai's
`src/realtime/protocol.ts` (Worker). Golden hash values are tested on both
sides.

## Connecting

```
GET /api/v1/realtime/connect?environment=<env>&scope=<scope>[&instance=auto][&party=<id>]
Upgrade: websocket
Sec-WebSocket-Protocol: kit-realtime.v1, <connection token>
```

- The token goes in the subprotocol header, never in the URL, so it stays out
  of logs. The server answers with `kit-realtime.v1`.
- `scope`: lowercase path segments, e.g. `world/main`,
  `world/main/sector/scenes/level2`, `match/42`. The token's scope patterns
  must allow it.
- `instance=auto`: the scope is a **sector**, and the service picks an instance
  with room (`…/i/1`, `…/i/2`). With `party=<id>`, a party lands together while
  there's room. WELCOME carries the real scope name.
- One connection per scope. A game normally holds two: its world and its
  current sector. While changing scenes it briefly holds a third, the next
  sector, which it joins before leaving the old one.
- A new connection with the same session id (`sid` claim) replaces the old one.
  That's how reconnects keep entity ownership.

## Frames

Every WebSocket message is a **binary frame containing one or more messages**
back to back. Each message is a `u8` type followed by its fields. Everything
is little-endian.

| Notation | Meaning |
|---|---|
| `str8` / `str16` | u8 / u16 byte length, then UTF-8 |
| `bytes16` | u16 length, then bytes |
| `value` | u8 tag, then: 0 null, 1 false, 2 true, 3 f64, 4 i32, 5 str16, 6 bytes16 |
| `transform` | u8 mask, then the present fields in order (below) |
| `world` | u8 has-seed (0/1), then `str8 id, f64 seed, str8 generator, str8 generatorVersion, u32 chunkSize, str8 compatibilityHash` |

### Transform block

| Bit | Field | Encoding | Bytes |
|---|---|---|---|
| 1 | position | 3 × f32 | 12 |
| 2 | rotation | smallest-three quaternion: 2-bit index + 3 × 10 bits | 4 |
| 4 | scale | 3 × f32 | 12 |
| 8 | velocity | 3 × i16, 1/100 m/s | 6 |
| 16 | angular velocity | 3 × i16, 1/100 rad/s | 6 |

Only changed fields are sent. A moving player (position and rotation) costs 1 +
16 bytes, plus 5 for the message header, so 22 bytes.

## Client → scope

| Type | Name | Fields | Needs permission |
|---|---|---|---|
| 0x01 | SPAWN | u16 request, u8 flags (1 persistent, 2 transferable, 4 unique: if an entity of this kind exists, SPAWNED returns it and its ENTITY follows), str8 kind, transform | spawn |
| 0x02 | DESPAWN | u32 entity | owner |
| 0x03 | TRANSFORM | u32 entity, transform | publish, owner |
| 0x04 | STATE_SET | u32 expect revision (0xFFFFFFFF = any), str8 key, value (null deletes) | state |
| 0x05 | EVENT | str8 name, bytes16 payload (≤ 4 KB) | events |
| 0x06 | INTEREST | f32 x, y, z, f32 radius (0 = scope default) | — |
| 0x07 | PING | u32 client time (ms) | — |
| 0x08 | CLAIM | u32 entity (owner gone, or transferable) | spawn |
| 0x09 | WORLD_MOD | i16 cx, i16 cz, u32 expect revision (0 = only if unset), str8 key, value | world |
| 0x0A | CHUNKS | u16 n, n × (i16 cx, i16 cz, u64 cached chunk hash); replaces the watched set | subscribe |
| 0x0B | ANIM | u32 entity, u16 state, u16 normalized time (×65535), u8 speed (×16) | publish, owner |
| 0x0C | WORLD_DESC | u32 expect world revision, then the seed fields (always present) | world |

## Scope → client

| Type | Name | Fields |
|---|---|---|
| 0x81 | WELCOME | u8 version, str8 scope, u32 session, u32 state revision, u32 world revision, u16 tick Hz, f32 interest radius, world, u64 world hash |
| 0x82 | ENTITY | u32 entity, u32 owner session (0 = none), u8 flags, str8 kind, transform (full) |
| 0x83 | GONE | u32 entity (despawned, or out of interest) |
| 0x84 | TRANSFORM | u32 entity, transform |
| 0x85 | STATE | u32 revision, str8 key, value, u32 by session (0 = developer) |
| 0x86 | STATE_REJECT | u32 current revision, str8 key (`w:<cx>,<cz>:<key>` for world mods, `$world` for the seed) |
| 0x87 | EVENT | u32 from session, str8 name, bytes16 payload |
| 0x88 | PRESENCE | u8 kind (1 join, 2 leave, 3 reconnect), u32 session, str8 player (opaque, may be empty) |
| 0x89 | PONG | u32 client time |
| 0x8A | ERROR | u16 code, str16 message |
| 0x8B | SPAWNED | u16 request, u32 entity |
| 0x8C | WORLD_MOD | u32 world revision, i16 cx, i16 cz, str8 key, value, u32 by, u64 world hash |
| 0x8D | OWNER | u32 entity, u32 owner session (0 = orphaned) |
| 0x8E | ANIM | u32 entity, 5 animation bytes |
| 0x8F | READY | (end of the join snapshot) |
| 0x90 | WORLD_DESC | u32 world revision, world, u64 world hash |
| 0x91 | CHUNK | i16 cx, i16 cz, u64 chunk hash, u32 count (modifications that follow; 0 when the cached hash matched) |

Error codes: 1 malformed, 2 forbidden, 3 limit, 4 unknown entity, 5 not owner,
6 too large, 7 rate, 8 scope full, 9 replaced by a newer connection.

## Joining and recovery

1. WELCOME gives the scope's state revision, world revision, seed and world hash.
2. The snapshot follows: every STATE key, every ENTITY in interest (and its
   ANIM), and a PRESENCE for each connected session.
3. READY ends the snapshot. Everything after it is live.

WebSockets are reliable and ordered, so a connection never misses a revision.
After a reconnect, the new snapshot replaces what the client had. A client
keeps the entities whose owner session is still its own: same `sid`, same
session number, within the reconnect grace.

## Seed + diff worlds

- The **seed** (WELCOME / WORLD_DESC) is what every client generates the scene
  or world from.
- **Modifications** are keyed per chunk (`tree:42` → `"removed"`). Writes with
  expect `0` succeed only once, which is how "collect this resource once" works.
- The **world hash** is the scope's "current seed": `seedHash + Σ modHash (mod
  2^64)` over every modification in effect, where
  `modHash = fnv1a64(i16 cx, i16 cz, key bytes, 0x00, value encoding)` and
  `seedHash = fnv1a64(str8 id, f64 seed, str8 generator, str8 generatorVersion,
  u32 chunkSize)`. It changes with every modification and depends only on
  content, so a client can recompute it from what it has.
- A **chunk hash** is the sum of its modifications' hashes. CHUNKS sends the hash
  the client has cached. When it matches, the scope answers with one 17-byte
  CHUNK and sends nothing else.
- A matching seed is not proof of identical generation: generators must be
  deterministic across platforms (integer hashing, no FMA-sensitive float
  paths, a fixed iteration order). Use `compatibilityHash` to compare a
  reference chunk.

## Streaming and interest

- The scope sends at most `tickHz` updates per entity per second (default 20,
  max 30). An entity that was quiet for a whole tick goes out immediately
  (leading edge). Faster updates coalesce: only the newest value is sent.
- With INTEREST set, an entity within `radius` streams at full rate, within 2×
  every `midEvery` ticks, within 4× every `farEvery` ticks, and is hidden
  beyond that (GONE). Hysteresis: it shows inside 4r and hides beyond 4.4r.
- Queues are bounded per client, at most one pending update per entity, so a
  slow client gets the latest state, never a backlog.
