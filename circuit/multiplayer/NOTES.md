# Multiplayer 0.1.0 (beta)

First release of the Realtime SDK.

- Realtime protocol v1: binary, batched, compact (a moving entity's update is
  18–24 bytes).
- Scopes: a world plus sectors, one per scene. A full sector opens a new
  instance automatically, and parties stay together.
- Transforms: dirty tracking, a 32-bit quaternion, distance level of detail,
  interpolation and optional extrapolation.
- Shared state with revisions, conditional writes and authority policies.
- Seed + diff worlds: an initial seed plus a world hash that changes with
  every modification, and per-chunk hashes so rejoining skips unchanged chunks.
- Per-component network defaults: rendering, terrain and physics shapes stay
  local unless you opt in.

Known limits: the native client covers Linux, macOS and Windows. The web
player and the editor's Realtime Sync inspector are coming next.
