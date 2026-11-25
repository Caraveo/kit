# Networked physics: best-effort, smoothed to the transform

Multiplayer does **not** synchronize rigid bodies. Physics is a best-effort
sync: each object is simulated by one client (its owner), and everyone else
smoothly follows the owner's streamed transform.

## How it works

- **Owner:** runs real physics for the objects it owns (its player, the ball
  it last kicked, the crate it pushed) and streams their transforms like any
  other synced entity. Velocity can be included (Realtime Sync → velocity) to
  smooth better and extrapolate a little.
- **Everyone else:** the object's rigid body is **kinematic**. Remote clients
  don't simulate it. Each frame they move it toward the latest streamed
  transform (interpolation, with optional short extrapolation from velocity),
  so it glides instead of snapping. Their own physics still collides with
  it, because a kinematic body pushes dynamic ones.
- **Nothing else travels:** no forces, contacts, impulses, solver state,
  masses or colliders. Collider shapes and body settings are in the scene file
  every client already has.

So a rigid body costs the same as any moving entity: its transform, about 22
bytes, only while it moves.

## Ownership is the trick

- Whoever is interacting with an object should own it: kicking a ball,
  grabbing a crate. Mark such objects **transferable** and `CLAIM` them when
  touched, so physics runs where the input is and feels immediate there.
- When the owner leaves, persistent objects are orphaned and the next client
  to touch them claims them. Temporary ones leave with their owner.

## What "best effort" means (expect this)

- Two clients may briefly disagree. During a collision between objects with
  different owners, each side sees the other's object where it was a few
  updates ago. Smoothing hides small differences, while big ones (a fast
  collision) can look slightly off for a moment, then converge.
- No rollback, no lockstep, no determinism guarantees. Physics engines aren't
  bit-identical across machines anyway.
- Not cheat-proof: the owner's client decides where its objects go. Games that
  need trusted physics (competitive ball games, fighting) need their own
  authority, such as a trusted simulation client (see ai-npc.md for the
  "service client" idea), not Realtime alone.

## Smoothing details (SDK)

- `sync::Interpolator` renders remote objects about 100 ms in the past, between
  the two samples around that time. Rotation uses slerp.
- When updates are late, it extrapolates from the last velocity for up to
  250 ms, only if the entity syncs velocity and the component enables
  extrapolation.
- If the error is large after a teleport or respawn, snap instead of gliding
  (planned: a snap threshold on the Realtime Sync component).
- Planned: a spring-style correction mode for kinematic followers, so physical
  contact feels softer than pure interpolation.
