---
title: Features, timers and more
description: GetFeature, AddFeature, BeActive, timers, coroutines, rotations, physics queries, animation, cameras, saving, debugging and Fabs.
---

## Features: reaching other objects (GetFeature)

Every object is made of **features**: its scripts, and engine features such as
`Model`, `Sprite`, `Physics`, `Collider`, `Audio`, `Light`, `Camera`, `Path`,
`Particles`, `Diagram` and `Transform`. `GetFeature` hands you one of them, on this object
or another, and you use it like a module or a class instance:

```x
TAKE Scene
ANY Foe: Scene.Find("Goblin")

Enemy E: GetFeature(Foe, "Enemy")   # its Enemy script (class or file name)
E.TakeDamage(10)                    # call its methods
E.Health: E.Health - 5              # read and set its fields

ANY M: GetFeature(Foe, "Model")     # an engine feature on another object
M.SetColor(1, 0, 0)
ANY Body: GetFeature("Physics")     # one argument = this object
```

| Function | |
|---|---|
| `GetFeature([object,] "Name")` | The feature, or `NULL` when the object doesn't have it |
| `HasFeature([object,] "Name")` | `TRUE` / `FALSE` |
| `AddFeature([object,] "Name"[, extra])` | Adds it and gives it back. `extra` is a shape (`"Sphere"`), a `.kmodel` path, a sound, a texture or `"Kinematic"`, depending on the feature. `AddFeature(obj, "scripts/Enemy.x")` adds a script. |
| `RemoveFeature([object,] "Name")` | Removes it |
| `BeActive([object,] TRUE/FALSE)` | Switch a whole object (and everything inside it) on or off. Off: not drawn, no collisions, no scripts. |
| `IsActive([object])` | Is it on (and all its parents)? |

`object` is an id, an object's name, or another feature. Every feature has
`.Enabled` (`M.Enabled: FALSE` hides a model, stops a script's updates, takes
a collider or body out of physics, silences audio).

## More lifecycle methods

| Method | When |
|---|---|
| `Init()` | Once. Every script's `Init` runs before any `Update`, so features on other objects are ready. |
| `Update(FLOAT dt)` | Every frame |
| `FixedUpdate(FLOAT step)` | 60 times a second, steadily (use it for physics forces) |
| `LateUpdate(FLOAT dt)` | After every Update, physics and movement; world positions are up to date (cameras that follow go here) |
| `OnCollision(ANY other[, OBJECT hit])` | Started touching. `hit` is `{ Point, Normal, Strength }`. |
| `OnClick([OBJECT hit])` | Clicked with the left mouse button (needs a Collider). `hit` is `{ Point, Normal, Distance }`. |
| `Final()` | When the object or script goes away, or play stops |

## Timers and coroutines

```x
TAKE Time
Time.After(2, "Explode")            # call a method once, 2 seconds from now
ANY Blink: Time.Every(0.5, "Flash") # call it every half second
Time.Cancel(Blink)

Time.Start("Countdown", 3)          # run a method that can WAIT
VOID CREATE Countdown(INTEGER n)[
    LOOP(n > 0)[
        DISPLAY[n]
        n: n - 1
        WAIT 1                      # pause this method for a second
    ]
    DISPLAY["Go!"]
]
```

`WAIT seconds` works anywhere in a method started with `Time.Start`, inside
`IF`, `LOOP` and `GRAB` too (`WAIT 0` waits a frame). `Time.Start` gives back
an id for `Time.Stop(id)`. Extra arguments after the method name are passed to it.

## Rotations and smooth movement

Rotations are VECTORs of degrees, like `Transform.Rotation`. The `Rotation`
module does the careful math underneath, so blends take the shortest way round
and nothing gets stuck pointing straight up.

| `Rotation.` | |
|---|---|
| `Blend(a, b, t)` | Part way from one rotation to another (0 = a, 1 = b) |
| `TurnToward(from, to, maxDegrees)` | Turn toward a rotation by at most some degrees |
| `LookToward(direction[, up])` | The rotation that faces a direction |
| `AroundAxis(axis, degrees)` | A turn around an axis |
| `Between(fromDir, toDir)` | The turn from one direction to another |
| `Combine(first, second)` | Rotate by one, then the other |
| `Reverse(r)` | The rotation that undoes `r` |
| `Apply(r, vector)` | Turn a vector by a rotation |
| `Angle(a, b)` | Degrees between two rotations |
| `Forward(r)`, `Up(r)`, `Right(r)` | Which way a rotation faces |

New `Math` helpers (numbers or VECTORs): `Blend(a, b, t)` (also called `Lerp`),
`MoveToward(current, target, maxStep)`, `Smooth(current, target, speed, dt)`
(eases the same at any frame rate), `Remap(x, fromLow, fromHigh, toLow, toHigh)`,
`Angle(v1, v2)`, `Seed(n)` (repeatable `Random`), `Noise(x[, y[, z]])` (smooth
0–1 noise) and `NoiseSeed(n)`.

New `Transform` members: `ToWorld(point)` and `ToLocal(point)`,
`ToWorldDirection(v)` and `ToLocalDirection(v)`, `SetWorldPosition(v)`,
`WorldRotation` / `SetWorldRotation(v)`, `LookAt(target, up)` and
`TurnToward(target, maxDegrees)`.

## Physics, colliders and layers

| | |
|---|---|
| `Physics.FindNear(center, radius[, layers])` | Every object whose collider overlaps a sphere |
| `Physics.SweepSphere(start, direction, radius[, max[, layers]])` | A fat ray: the first thing a moving ball would touch → `{ Entity, Distance, Point, Normal }` |
| `Physics.Raycast(origin, direction, max, layers)` | A ray that only sees some layers |
| `Physics.Kind` / `SetKind("Dynamic" / "Kinematic" / "Static")` | Change how a body moves while playing |
| `Physics.Mass` / `SetMass`, `GravityScale` / `SetGravityScale` | |
| `Collider.Layer` / `SetLayer(0-31)` | Which layer a collider is on |
| `Collider.Trigger` / `SetTrigger`, `Bounciness` / `SetBounciness`, `Friction` / `SetFriction` | |

`layers` is one layer number or a list, e.g. `[0, 3]`.

## Animation, sound and light

| | |
|---|---|
| `Model.Play("Run")` | Start a clip from the beginning (`""` = the first clip) |
| `Model.Pause()`, `Resume()`, `Playing` | |
| `Model.AnimationSpeed` / `SetAnimationSpeed(x)` | Changes speed smoothly, without jumping |
| `Model.AnimationTime` / `SetAnimationTime(s)`, `Animation`, `Loop` / `SetLoop` | |
| `Model.Color`, `Model.Visible` | |
| `Audio.Volume` / `SetVolume`, `Pitch` / `SetPitch`, `SetLoop`, `Clip` / `SetClip` | Change a playing sound live (an engine's pitch rising with speed) |
| `Light.Color` / `SetColor`, `Brightness` / `SetBrightness` | |

## Cameras, mouse, gamepads and touch

| | |
|---|---|
| `Camera.MakeActive()` | Show the game through this camera |
| `Camera.FieldOfView` / `SetFieldOfView`, `Size` / `SetSize` (orthographic) | |
| `Camera.ScreenToRay(Input.Mouse)` | `{ Origin, Direction }` through a screen point (use with `Physics.Raycast`) |
| `Camera.WorldToScreen(point)` | Pixel position of a world point (Z = distance in front) |
| `Input.LockMouse(TRUE)` | Capture the mouse for mouse-look (hidden, unlimited `MouseDelta`); Esc in the editor frees it |
| `Input.ShowMouse(FALSE)`, `MouseLocked`, `ScreenSize` | |
| `Input.Bind("Jump", "Space", "PadA")` | Name an action; `Input.Down("Jump")` then works for any of its keys. Call again to rebind. |
| `Input.Unbind("Jump")`, `Bound("Jump")`, `AnyPressed` | `AnyPressed` gives the key pressed this frame (for "press a key" screens) |
| `Input.Stick("Left")`, `Input.Trigger("Right")` | Gamepad sticks (VECTOR) and triggers (0–1); buttons are keys like `"PadA"` |
| `Input.Touches`, `TouchCount` | Fingers on a touch screen |

## Tags, saving and debugging

| | |
|---|---|
| `Entity.AddTag("Enemy")`, `RemoveTag`, `HasTag`, `Tags` | Labels (also editable in the Inspector, saved with the scene) |
| `Scene.FindTagged("Enemy")`, `Scene.FindAll("Name")` | Lists of ids |
| `Save.Set("best", 120)`, `Save.Get("best", 0)`, `Has`, `Delete`, `Clear`, `Keys` | Values kept between plays and launches (numbers, text, VECTORs, lists, objects) |
| `Debug.Line(from, to[, color[, seconds]])`, `Debug.Ray(origin, dir, …)`, `Debug.Sphere(center, radius, …)` | Drawn over the viewport while playing |
| `Debug.Clock()` | Seconds from a steady clock, to time your own code |

## On-screen interface from scripts

Change the scene camera's interface (`.xui`) by element id:
`Interface.SetText("score", 12)`, `Interface.Show("menu")`, `Interface.Hide("menu")`,
`Interface.SetColor("time", 1, 0.4, 0.4)` and `Interface.SetBackground(id, r, g, b[, a])`.
Changes last until play stops.

## Cinematics (Circuit)

With the Cinematics Circuit installed: `Cinematic.Play("Intro")`, `Stop()`,
`Skip()`, `Playing`, `Current`, `Shot`, `Time`. `Playing` is `FALSE` without
the Circuit, so games still run. See `circuit/cinematics/README.md`.

## Diagrams and scripts

A diagram's **Run Script** node calls a function on an object's script and
gets its result back; an error comes back to the node instead of stopping the
script. Scripts send events the other way with `Diagram.Emit("Name"[, value])`,
which a diagram's **On Custom Event** node receives.

## Fabs: reusable objects

Right-click an object in the Hierarchy → **Save as Fab** to save it (with
everything inside it) as `fabs/<Name>.kfab`. Place one from **Assets → Fabs**
(double-click), or spawn it from a script: `INSTANTIATE("Enemy", position)` or
`INVOKE("fabs/Enemy.kfab")`. A bare name means an object in the scene first,
then the Fab of that name.
