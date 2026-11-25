# AI NPCs (future Circuit)

Status: **design notes for later**. Nothing here is implemented yet. It
will ship as its own Circuit (`kit.ai-npc`), separate from Multiplayer, and
can be installed with or without it.

## The rule: network only what other players must agree on

Most NPCs in most games don't need the network at all. Pick the cheapest tier
that works:

| Tier         | Examples                                                                                                            | Where it runs                                       | Network               |
| ------------ | ------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------- | --------------------- |
| **Ambient**  | bugs, birds, butterflies, fish, fireflies, crowds in the distance                                                   | each client, locally                                | none                  |
| **Personal** | a storyteller who talks to *you*, a companion pet, a tutorial guide, a shopkeeper's small talk                      | each client, locally (AI calls go from that client) | none                  |
| **Shared**   | a guard everyone can fight, a boss, a merchant whose stock runs out, a quest giver whose state changes for everyone | an NPC client on Kit's servers ("NPC as a service") | streams like a player |

### Ambient AI: local, unsynced

Bugs, birds, butterflies and the like are things you can interact with (scare a
flock, catch a butterfly) but nobody else has to see exactly the same one.

- Run them entirely on the client, like particles with behavior.
- If players should *roughly* agree (a flock over the lake for everyone),
  spawn them from the scene's world seed plus a time bucket, e.g.
  `hash(world seed, sector, floor(time / 60 s))`. Every client then gets the same
  flock without sending a byte, because Realtime's seed + diff model already
  gives every client the same seed.
- An interaction that matters (you caught the golden butterfly, worth a
  quest flag) is sent as a normal Realtime event or state change, not as the
  creature's movement.

### Personal AI: local, unsynced

NPCs that tell you stories, chat, guide you, or react only to you.

- Each player's conversation is their own. Nothing to sync.
- Runs in the client; dialog generation calls Kit's AI service (Rex
  infrastructure) or the developer's own backend with the *developer's*
  credentials held server-side, never inside the game.
- If a conversation changes the shared world ("the storyteller gave you the
  key"), only that outcome goes through Realtime (state or world
  modification), with the usual permission checks.

### Shared AI: NPC as a service

For NPCs every player must see identically (mostly competitive or MMO-style
games):

- Each shared NPC, or a group of them in a sector, runs as an **independent
  client on Kit's servers**, with its own code and AI.
- It joins scopes with a connection token like any player, owns its entities,
  and streams transforms, animation and state through the same scopes. Its
  token marks it as an NPC (an `npc` claim the scope announces in presence)
  and may grant extra permissions (e.g. state authority in its sector).
- Scaling follows sectors: an NPC service per active sector instance, started
  when the first player enters and stopped when the instance empties.
- Because most games' NPCs are player-based (personal), this tier stays
  opt-in and per NPC.


DO NOT USE NPC to fool Players into believing they are not BOTS. Basically be smart.
## Open questions for when this is built

- NPC runtime: JavaScript/TypeScript vs. compiling x scripts to run on
  Kit's servers.
- Behavior authoring in the Kit editor: Diagram graphs, x scripts, or prompts.
- Cost controls: AI calls per NPC per minute, shared NPCs per environment.
- Moderation and safety filtering for generated dialog.
