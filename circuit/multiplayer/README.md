# Multiplayer Circuit (Realtime)

Multiplayer for Kit games without game servers. Install it from **Project →
Circuit Manager**, mark the entities that should be shared, and players see
each other.

```
Developer
  └─ Game project            (a Realtime project; its id goes in kit.project)
       └─ Environment        (development, production, …)
            └─ World         world/<id>: global state, time of day, the world seed
                 └─ Sectors  world/<id>/sector/<scene path>: one per scene,
                             instanced automatically when full
```

- **Clients render and simulate.** Realtime relays only what other players
  need to reproduce what they see: transforms of synced entities, animation
  state, shared game state, world changes and events.
- **The owner client is authoritative** for the entities it spawned. Shared
  state uses revisions and conditional writes, and the scope host can be the
  only writer. Realtime is a synchronization service, not a trusted
  simulation: games that need cheat-proof combat or physics need their own
  trusted logic. AI NPCs as a service are planned separately (see
  [ai-npc.md](ai-npc.md)).
- **No servers to run.** Scopes are created on first use and released when
  idle. Developers never configure infrastructure.
- **Players don't have accounts.** Connections use short-lived tokens from
  your game's backend (with a backend key) or, if you allow it, anonymous
  tokens. REX accounts are for developers only.

## What gets networked

Nothing is networked unless an entity has **Realtime Sync**. Even then, only
what changes travels:

| Component | Default | Why |
|---|---|---|
| Transform | streamed (position and rotation; scale opt-in) | the thing other players see move |
| Rig / animation | on change (state, time, speed: 5 bytes) | skeletons are evaluated locally |
| Model, sprite, light, camera, post effects | local | rendering is local; assets are on every client |
| Terrain | local; **sculpting opt-in** as world modifications | heights are in the scene or regenerated from the seed |
| Collider, rigid body | local (velocity opt-in for smoothing) | the owner simulates; others follow its transform |
| Path follower | local | deterministic from the scene and time |
| Audio | events | play/stop travel, not audio |
| Scripts | local; networked fields as state | logic runs on the owner |

Ambient life (birds, bugs) and personal NPCs (a storyteller talking to you)
stay local too. See [ai-npc.md](ai-npc.md). Physics is best effort: the owner
simulates, and everyone else smoothly follows its transform. See
[networked-physics.md](networked-physics.md).

In the editor: **+ Add component → Circuits → Realtime Sync** on the objects to
share, and **Project → Multiplayer…** for this project's environment and world.

## Scenes are sectors

`Scene.Load("scenes/level2")`, now part of the engine, switches scenes. With
Multiplayer, the client joins that scene's sector (`instance=auto`) while the
new scene loads, and leaves the old sector once the new one is ready. No gap,
no duplicate entities. A full sector opens a new instance, and parties stay
together.

## Seed + diff worlds

A world or scene has an **initial seed** (what every client generates from) and a
**world hash**, which acts as the updated seed: it changes with every
modification. Late joiners get the seed and current hash, regenerate locally,
and fetch only the modifications for the chunks they watch. If they cached a
chunk before, they send its hash and the scope skips it when it's still
current. See [PROTOCOL.md](PROTOCOL.md).

## SDK

`sdk/` is the Rust SDK (`kit_realtime`):

```rust
let mut rt = Realtime::connect(Config { connect_url, environment, world: "main".into(), party: None }, tokens);
rt.enter_scene("scenes/level2.kscene");            // join that scene's sector
let req = rt.spawn(Channel::Sector, "player", 0, &first_transform);
// every frame:
if let Some(t) = dirty.update(&sync, now, 20, pos, rot, scale, vel) { rt.transform(Channel::Sector, id, &t); }
rt.set_state(Channel::World, "weather", &Value::Str("rain".into()), ANY_REVISION);
rt.modify(Channel::Sector, cx, cz, "tree:42", &Value::Str("removed".into()), 0);
rt.flush();                                          // one frame per scope per game frame
for (channel, event) in rt.poll() { /* spawn, move, despawn, state, world… */ }
```

Networking runs on its own thread; the game and render threads never block.
Queues are bounded, and drops reconnect with backoff using a fresh token.

`cargo run -p kit_realtime --example probe` runs two clients against a scope
and prints measured numbers.

## Status (0.1.0 beta)

Implemented and tested: the protocol, scopes, sector instancing, tokens,
transforms, state, world seed + diff, interest management, presence,
reconnect, the dashboard, and the native client (Linux, macOS, Windows).

Not yet: the editor's Realtime Sync inspector and automatic ECS binding (today
games call the SDK), the web player transport, partitioning one sector across
several objects, and hosted billing.
