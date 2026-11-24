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
