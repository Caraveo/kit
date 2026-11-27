# Multiplayer 0.1.1 (beta)

- **Realtime Sync component:** + Add component → Circuits → Realtime Sync.
  Choose what an object shares: position, rotation, scale, velocity,
  animation, rate, smoothing, scope, lifetime and takeover.
- **Project → Multiplayer…:** this project's environment, world, player token
  mode and Realtime address, plus a button to open your Multiplayer dashboard.
- Docs: networked physics is best effort. Others follow the owner's
  transform, and rigid bodies aren't synced (networked-physics.md).

## 0.1.0

The Realtime SDK: binary protocol v1, worlds and sectors with automatic
instances, streamed transforms with distance and crowd culling, shared state
with revisions, seed + diff worlds with world and chunk hashes, and
per-component network defaults.
