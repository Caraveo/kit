---
title: Scripting physics
description: Velocities, impulses, forces, ray casts and contacts from x.
---

`TAKE Physics` in your script. Members act on the script's own object:

| Member | |
|---|---|
| `Velocity` / `SetVelocity(v)` | Linear velocity (units/s) |
| `AngularVelocity` / `SetAngularVelocity(v)` | Spin (radians/s per axis) |
| `ApplyImpulse(v)` | An instant kick (jumps, explosions) |
| `AddForce(v)` | A push during the next physics step; call it every frame for a continuous force |
| `Contacts` | An `INDEX` of ids of objects touching this one |
| `Touching(id)` | `TRUE` if touching that object |
| `Gravity` / `SetGravity(v)` | World gravity |
| `Raycast(origin, direction[, max])` | The first thing hit, or `NULL` |

## Ray casts

```x
ANY Hit: Physics.Raycast(Transform.WorldPosition, VECTOR(0, -1, 0), 10)
IF(EXIST Hit)[
    DISPLAY["Ground below:", Scene.Name(Hit.Entity), "at", Hit.Distance]
]
```

A hit is an `OBJECT` with `Entity` (id), `Distance`, `Point` (`VECTOR`) and `Normal` (`VECTOR`). The ray ignores the script's own object and trigger colliders.

## Collision events

Define these methods in your script's class:

```x
VOID CREATE OnCollision(ANY other)[ ]      # started touching
VOID CREATE OnCollisionEnd(ANY other)[ ]   # stopped touching
VOID CREATE OnTrigger(ANY other)[ ]        # entered a trigger
VOID CREATE OnTriggerEnd(ANY other)[ ]     # left a trigger
```

Events arrive on the frame after the physics step that produced them.
