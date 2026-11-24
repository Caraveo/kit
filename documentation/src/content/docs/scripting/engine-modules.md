---
title: Engine modules
description: What a script can reach with TAKE. Transform, Input, Time, Physics, Path and more.
---

Scripts talk to the engine through **modules**. Bring them in with `TAKE` at the top of the file:

```x
TAKE Transform, Input, Time
```

Using a module without `TAKE` gives a clear error telling you which line to add.

Members can be **called** (`Transform.Move(1, 0, 0)`), **read like properties** without parentheses (`Transform.Position`, `Time.Delta`), and **set with `:`** (`Transform.Position: VECTOR(0, 1, 0)`, which calls `SetPosition`). Vectors can be written as `VECTOR(x, y, z)` or as three numbers.

## Transform
Acts on the script's own object.

| Member | |
|---|---|
| `Position` / `SetPosition(v)` | Local position |
| `Move(v)` | Add to the position |
| `Rotation` / `SetRotation(v)` | Rotation in degrees (X, Y, Z) |
| `Rotate(v)` | Rotate by degrees |
| `Scale` / `SetScale(v or number)` | Scale |
| `Forward`, `Right`, `Up` | Direction vectors |
| `LookAt(v)` | Turn to face a point |
| `WorldPosition` | Position in the world (after parents) |

## Input
Key names are case-insensitive: `"A"`, `"1"`, `"Space"`, `"Enter"`, `"Escape"`, `"Shift"`, `"Ctrl"`, `"Left"`, `"Right"`, `"Up"`, `"Down"`, `"F5"`, `"MouseLeft"`, `"MouseRight"`.

| Member | |
|---|---|
| `Down("A")` | Held this frame |
| `Pressed("Space")` | Went down this frame |
| `Released("Space")` | Went up this frame |
| `Axis("Left", "Right")` | −1, 0 or +1 |
| `Mouse`, `MouseDelta`, `Scroll` | Mouse position (pixels), movement, wheel |

## Time
`Delta` (seconds since the last frame), `Elapsed` (seconds since play started), `Frame`, `Scale`.

## Entity
The script's own object: `Id`, `Name`, `SetName("…")`, `Destroy()`, `Parent`, `Children`.

## Scene
Any object, by id:

| Member | |
|---|---|
| `Find("Name")` | Id of the first object with that name, or `NULL` |
| `Exists(id)`, `Name(id)`, `Destroy(id)` | |
| `Position(id)`, `SetPosition(id, v)`, `Move(id, v)`, … | Every Transform member, with the id first |

## Model / Sprite
`Model.SetColor(r, g, b[, a])`, `Model.SetVisible(BOOL)`.
`Sprite.SetColor(r, g, b[, a])`, `Sprite.Color`, `Sprite.SetSize(w, h)`, `Sprite.SetFlipX(BOOL)`, `Sprite.SetFlipY(BOOL)`, `Sprite.SetVisible(BOOL)`.

## Physics
See [Scripting physics](/physics/scripting-physics/): `Velocity`, `SetVelocity`, `ApplyImpulse`, `AddForce`, `Raycast`, `Contacts`, `Touching`, `Gravity`, `SetGravity`.

## Path
For objects with a [sketched path](/editor/canvas-mode/): `Play()`, `Pause()`, `Playing`, `Speed`, `SetSpeed(n)`, `Points`.

## Terrain
`HeightAt(x, z)`: the ground height at a world position, or `NULL` off the terrain. See [Terrain](/editor/terrain/).

## Audio

`Play()`, `Stop()`, `Playing`, `Volume` / `SetVolume(v)`, `Pitch` / `SetPitch(p)`, `SetLoop(b)`, `Clip` / `SetClip(path)`, `PlaySound(path, volume, pitch)`, `StopAll()`. See [Sound](/editor/sound/).

## Log
`Log.Info(...)`, `Log.Warn(...)`, `Log.Error(...)`: like `DISPLAY`, with a level.

## Math
Built into x (no engine needed): `Math.PI`, `Sin`, `Cos`, `Tan`, `Atan2`, `Sqrt`, `Pow`, `Abs`, `Floor`, `Ceil`, `Round`, `Min`, `Max`, `Clamp`, `Lerp`, `Random()`, `RandomRange(a, b)`, `Radians`, `Degrees`.
