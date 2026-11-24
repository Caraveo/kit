---
title: x basics
description: Variables, types, conditions, loops and functions in x.
---

**x** (eXtensible, or X++) is Kit's scripting language. It reads like C with a few twists:

- **Types and keywords are ALL CAPS:** `FLOAT`, `IF`, `CREATE`.
- **Blocks use square brackets** `[ ]`.
- **You assign with a colon:** `Score: Score + 1`. Comparison is `==`.
- **Comments start with `#`.**
- **There are no semicolons.** One statement per line.

## Variables

Declare a variable with its type, a name and a value:

```x
STRING Name: "Kit"
INTEGER Lives: 3
FLOAT Speed: 2.5
BOOL Ready: TRUE
INDEX Items: ["key", "map"]          # a list
OBJECT Save: { "Level": 2, Name: "a" }
VECTOR Spot: VECTOR(1, 2, 3)         # Spot.X, Spot.Y, Spot.Z
```

Change it later with `:`:

```x
Lives: Lives - 1
Spot.Y: 5
Items.Add("lamp")
```

Types are **strict**: putting a `STRING` into an `INTEGER` is an error. Integers widen to `FLOAT` automatically. Convert explicitly with `INTEGER("5")`, `STRING(3)` or `FLOAT(x)`.

## Conditions

```x
IF(Lives <= 0)[
    DISPLAY["Game over"]
] ELSE IF(Lives == 1)[
    DISPLAY["Last life!"]
] ELSE [
    DISPLAY["Lives:", Lives]
]
```

Combine conditions with `AND`, `OR` and `NOT` (or `&&`, `||`, `!`). `EXIST x` is `TRUE` when `x` is not `NULL`.

## Loops

```x
GRAB Item IN Items[          # for each
    DISPLAY[Item]
]

GRAB I IN RANGE(10)[         # 0..9
    IF(I % 2 == 0)[ SKIP ]   # continue
    IF(I > 7)[ EXIT ]        # break
]

LOOP(Lives > 0)[             # while
    Lives: Lives - 1
]
```

## Functions

```x
FLOAT CREATE Double(FLOAT value)[
    GIVE value * 2           # GIVE returns a value
]

VOID CREATE Greet(STRING who)[
    DISPLAY["Hello,", who]
]

Greet("world")
DISPLAY[Double(21)]
```

A function can return several values, which come back as an `INDEX`:

```x
STRING, INTEGER CREATE Info()[
    GIVE "Kit", 1
]
INDEX R: Info()     # R[0] = "Kit", R[1] = 1
```

## Printing

`DISPLAY[...]` prints values separated by spaces to the editor console (and the terminal for exported games). Strings join with `+`: `"Score: " + Score`.

Next: [Classes & methods →](/scripting/classes/)
