---
title: Physics bodies
description: Make objects fall, bounce, get pushed and be moved by forces.
---

Add **＋ Add component → ⚛ Physics** to make an object part of the simulation. This also adds a fitted collider if it doesn't have one. An object with only a collider shows a one-click **⚛ Add Physics** button.

## Type

| Type | Behavior | Use it for |
|---|---|---|
| **Dynamic** | Moved by the simulation: gravity, collisions, forces. | Crates, balls, ragdoll-style objects, physics-driven players |
| **Kinematic** | Moved by your scripts; pushes dynamic bodies but nothing pushes it. | Moving platforms, doors, elevators |
| **Static** | Never moves. | Floors, walls |

## Settings (Dynamic)

| Setting | |
|---|---|
| **Mass** | In kilograms. Heavier objects are harder to push. |
| **Gravity ×** | Gravity multiplier: 0 floats, 2 falls twice as fast, −1 falls up. |
| **Damping: move / spin** | Slows movement and rotation over time (like air resistance). |
| **Lock rotation** | Keeps the object upright. Use for characters. |
| **2D (XY plane)** | Keeps the object on the XY plane, for 2D games. |
| **Fast (CCD)** | Prevents fast objects from passing through thin walls. |

While playing, the section shows the body's live **Velocity**. **World settings…** opens the [physics settings](/physics/settings/).

## Moving physics objects from scripts

Don't set a dynamic body's position every frame; that fights the simulation. Use velocity, impulses or forces instead (see [Scripting physics](/physics/scripting-physics/)). Teleporting with `Transform.Position: …` is fine for respawning.
