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

Properties can be read without parentheses (`Transform.Position`, `Time.Delta`)
and written with `:`. For example, `Transform.Position: VECTOR(0, 1, 0)` calls
`SetPosition`.

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
