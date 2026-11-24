---
title: Classes & methods
description: Group data and behavior with CREATE, inherit with RIDE.
---

A **class** groups fields (data) and methods (functions):

```x
CREATE Enemy[
    STRING Name: "Slime"
    INTEGER Health: 10

    VOID CREATE Hit(INTEGER amount)[
        Health: Health - amount
        IF(Health <= 0)[
            DISPLAY[Name, "defeated"]
        ]
    ]
]
```

Inside a method, you use fields by name (`Health`). Use `THIS` when you need the object itself.

## Showing fields in the editor: PUBLIC and EDIT

Fields are **private** by default: the editor doesn't show them. Put a word in front to change that:

| Written as | In the inspector |
|---|---|
| `FLOAT Yaw: 0.0` | Hidden (private) |
| `PUBLIC INTEGER Health: 10` | Shown with its value, **not editable** |
| `EDIT FLOAT Speed: 5.0` | Shown **and editable**: public, plus you can set it per object |

```x
CREATE Player[
    EDIT FLOAT Speed: 3.0        # tune it in the inspector
    EDIT VECTOR Spawn: VECTOR(0, 1, 0)
    PUBLIC INTEGER Coins: 0      # watch it while playing
    FLOAT Cooldown: 0.0          # internal: hidden
]
```

- A value you set in the inspector is saved with the object in the scene and applied **before `Init` runs**. The script's own value (`3.0` above) is the default; **↺** goes back to it.
- While playing, PUBLIC and EDIT fields show live values. Changing an EDIT field then affects the running game, but is undone when you stop.
- `EDIT` works on `STRING`, `INTEGER`, `FLOAT`, `BOOL` and `VECTOR` fields. Other types can be `PUBLIC`.
- `PUBLIC` and `EDIT` only go in front of class fields, not methods or local variables.
- These words control what the **editor** shows. Your scripts can still read each other's fields in code.

## Making instances

```x
NEW Boss Enemy()          # declares Boss
Boss.Hit(3)
DISPLAY[Boss.Health]      # 7

Enemy Other: NEW Enemy()  # same thing, as a typed variable
```

### Constructors

A method named `New` runs when an instance is created, with the arguments you pass:

```x
CREATE Enemy[
    INTEGER Health: 10
    VOID CREATE New(INTEGER hp)[
        Health: hp
    ]
]
NEW Boss Enemy(100)
```

## Inheritance with RIDE

`RIDE` makes one class build on another. The child gets the parent's fields and methods, and can replace any of them:

```x
CREATE Actor[
    FLOAT Speed: 2.0
    VOID CREATE Describe()[ DISPLAY["An actor"] ]
]

CREATE Player RIDE Actor[
    FLOAT Speed: 4.0                      # overrides the default
    VOID CREATE Describe()[ DISPLAY["The player, speed", Speed] ]
]
```

## Using other files

`TAKE "path.x"` brings in the classes and functions of another script:

```x
TAKE "lib/helpers.x"
```

Next: [Attaching scripts to objects →](/scripting/attaching-scripts/)
