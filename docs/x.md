# The x language in Kit

x (eXtensible / X++, <https://github.com/Caraveo/x>) is a C-like hybrid. It has
ALL-CAPS types and keywords, `[ ]` blocks, `:` for assignment, classes with
methods, and module access through `TAKE`.

```x
# Comments start with #
TAKE Transform, Input, Time, Math     # engine modules
TAKE "lib/helpers.x"                  # another script file

CREATE Player[                        # a class
    FLOAT Speed: 3.0                  # fields: TYPE Name: value
    INDEX Items: ["key", "map"]

    VOID CREATE Init()[               # called once
        DISPLAY["Ready with", Items.Length(), "items"]
    ]

    VOID CREATE Update(FLOAT dt)[     # called every frame
        FLOAT X: Input.Axis("Left", "Right")
        Transform.Move(X * Speed * dt, 0, 0)
        IF(Input.Pressed("Space"))[
            Jump()
        ] ELSE IF(Transform.Position.Y > 5)[
            Transform.Position: VECTOR(0, 0, 0)
        ]
    ]

    VOID CREATE Jump()[
        Transform.Move(0, 1, 0)
    ]
]
```

## Types

| Type | Example |
|---|---|
| `STRING` | `STRING Name: "Kit"` |
| `INTEGER` | `INTEGER Lives: 3` |
| `FLOAT` | `FLOAT Speed: 2.5` (integers widen automatically) |
| `BOOL` | `BOOL Ready: TRUE` |
| `INDEX` (list) | `INDEX Cats: ["Pixie!", "Diva"]` |
| `OBJECT` | `OBJECT Save: { "Level": 2, Name: "a" }` |
| `VECTOR` | `VECTOR P: VECTOR(1, 2, 3)`, with `P.X`, `P.Y`, `P.Z` |
| `ANY` | any value |
| class name | `Enemy Target: NULL` |

Types are strict. Assigning a `STRING` to an `INTEGER` is an error, and
converting is explicit: `INTEGER("5")`, `STRING(3)`, `FLOAT(x)`.

## Statements

| | |
|---|---|
| Declare | `FLOAT Speed: 1.0` |
| Assign | `Speed: Speed + 1` (x assigns with `:`, compares with `==`) |
| If | `IF(a > b)[ ... ] ELSE IF(c)[ ... ] ELSE [ ... ]` |
| Loop | `LOOP(cond)[ ... ]`, with `EXIT` (break) and `SKIP` (continue) |
| For each | `GRAB Item IN Items[ ... ]` (lists, strings, object keys, `RANGE(n)`) |
| Return | `GIVE value` or `GIVE a, b` (multiple values come back as an `INDEX`) |
| Print | `DISPLAY["Score:", Score]` |
| New instance | `NEW Boss Enemy(100)` or `Enemy Boss: NEW Enemy(100)` |
| Spawn an object | `INVOKE("Sphere")` (inside this object) or `INSTANTIATE("Sphere")` (top level); see below |
| Call | `Jump()` or `CALL Jump()` |
| Inheritance | `CREATE Boss RIDE Enemy[ ... ]` |
| Constructor | a method named `New` |

Operators: `+ - * / %`, `== != < > <= >=`, `AND OR NOT` (or `&& || !`), and
`EXIST x` (TRUE when not `NULL`). Strings concatenate with `+`.

## Built-ins

* `RANGE(n)`, `RANGE(a, b)`, `LENGTH(x)`, `TYPEOF(x)`
* `INDEX`: `Add`, `Insert`, `Remove`, `Pop`, `Contains`, `Find`, `Join`, `Reverse`, `Copy`, `Length`
* `STRING`: `Length`, `Upper`, `Lower`, `Trim`, `Contains`, `StartsWith`, `Split`, `Replace`
* `OBJECT`: `Keys`, `Has`, `Remove`, `Length`
* `VECTOR`: `Length`, `Normalize`, `Dot`, `Cross`, `Distance`
* `Math` (`TAKE Math`): `PI`, `TAU`, `E`, `Sin`, `Cos`, `Tan`, `Asin`, `Acos`, `Atan`, `Atan2`, `Sqrt`, `Pow`, `Abs`, `Sign`, `Floor`, `Ceil`, `Round`, `Min`, `Max`, `Clamp`, `Lerp`, `Random`, `RandomRange`, `Radians`, `Degrees`

## Engine modules

A script runs on the entity it is attached to. These modules act on that entity:

| Module | Members |
|---|---|
| `Transform` | `Position`, `SetPosition`, `Move`, `Rotation` (degrees), `SetRotation`, `Rotate`, `Scale`, `SetScale`, `Forward`, `Right`, `Up`, `LookAt`, `WorldPosition` |
| `Input` | `Down("A")`, `Pressed`, `Released`, `Axis("Left", "Right")`, `Mouse`, `MouseDelta`, `Scroll` |
| `Time` | `Delta`, `Elapsed`, `Frame`, `Scale` |
| `Entity` | `Id`, `Name`, `SetName`, `Destroy`, `Parent`, `Children` |
| `Scene` | `Find("Name")`, `Exists(id)`, `Destroy(id)`, `Name(id)`, and any Transform member with an id first: `Scene.SetPosition(id, v)` |
| `Sprite` | `Color`, `SetColor(r, g, b[, a])`, `SetSize`, `SetFlipX`, `SetFlipY`, `SetVisible` |
| `Model` | `SetColor`, `SetVisible` |
| `Log` | `Info`, `Warn`, `Error` |
| `Physics` | `Velocity`, `SetVelocity`, `AngularVelocity`, `SetAngularVelocity`, `ApplyImpulse`, `AddForce`, `Gravity`, `SetGravity`, `Contacts` (entity ids), `Touching(id)`, `Raycast(origin, dir[, max])` → `{ Entity, Distance, Point, Normal }` or `NULL` |
| `Path` | `Play`, `Pause`, `Playing`, `Speed`, `SetSpeed`, `Points` (for paths sketched in Canvas mode) |
| `Collider`, `Light`, `Camera` | See below |
| `Audio` | `Play`, `Stop`, `Playing`, `PlaySound(clip[, volume, pitch])`, `StopAll`, `Volume`/`SetVolume`, `Pitch`/`SetPitch`, `SetLoop`, `Clip`/`SetClip` |
| `Diagram` | `Emit("Event"[, value])` |
| `Save`, `Debug` | See below |
| `Rotation`, `Math` | Built into x (no engine needed) |

Properties can be read without parentheses (`Transform.Position`, `Time.Delta`)
and written with `:`. For example, `Transform.Position: VECTOR(0, 1, 0)` calls
`SetPosition`.

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

## Script lifecycle

Kit uses the class whose name matches the file (`player.x` → `Player`), or the
first class in the file. A script without a class can define top-level `Init`
and `Update` functions instead. Kit calls `Init()` once, `Update(FLOAT dt)`
every frame, and `Final()` when the entity or script goes away.

With colliders, Kit also calls these methods when they are defined. `other` is
an entity id, so `Scene.Name(other)` gives its name:

```x
VOID CREATE OnCollision(ANY other)[ DISPLAY["Hit", Scene.Name(other)] ]
VOID CREATE OnCollisionEnd(ANY other)[ ]
VOID CREATE OnTrigger(ANY other)[ ]       # entered a trigger collider
VOID CREATE OnTriggerEnd(ANY other)[ ]
``` Saving a script
while the game is playing reloads it.
