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

## Spawning objects: INVOKE and INSTANTIATE

`INVOKE` and `INSTANTIATE` create an object in the scene right away and give
back its id, so you can move it or destroy it on the next line. No `TAKE` is
needed. They're the same except for where the new object goes:

* `INVOKE` puts it **inside the object whose script called it**, as a child in
  the Hierarchy. Its position, rotation and scale are relative to that object,
  and it moves with it.
* `INSTANTIATE` puts it **at the top level**, so its position is in world space.

```x
ANY Rock: INVOKE("Sphere", VECTOR(0, 5, 0))                # 5 above this object, inside it
INSTANTIATE("models/ship/Ship.kmodel", VECTOR(0, 0, -10))  # at a world position
ANY Enemy: INSTANTIATE("Enemy Template")                   # copy an object in the scene
INVOKE(Enemy, VECTOR(4, 0, 0), VECTOR(0, 90, 0), 2)        # copy by id, rotated and scaled
```

Both take `(what[, position[, rotation[, scale]]])`:

* `what` is a shape (`"Cube"`, `"Sphere"`, `"Cylinder"`, `"Torus"`, `"Plane"`,
  `"Quad"`), a `.kmodel` file, or an object in the scene, given by name or id.
  A copy brings its children, components and scripts along, and its scripts
  start running on their own. A good pattern is a hidden template object that
  you copy.
* `rotation` is in degrees, and `scale` is a number or a `VECTOR`.
* Objects created while playing go away when you press Stop.

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
