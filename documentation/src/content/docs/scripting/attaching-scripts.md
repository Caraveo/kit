---
title: Attaching scripts
description: How Kit runs a script on an object, and the lifecycle methods it calls.
---

Add a **Script** component to an object and pick an `.x` file. When the game plays, Kit creates **one instance of the script's class per object** and calls these methods if they exist:

| Method | When |
|---|---|
| `Init()` | Once, before the first update |
| `Update(FLOAT dt)` | Every frame. `dt` is the time since the last frame, in seconds |
| `Final()` | When the object is removed, the script changes, or play stops |
| `OnCollision(ANY other)` | This object's collider starts touching another one |
| `OnCollisionEnd(ANY other)` | ...stops touching it |
| `OnTrigger(ANY other)` | This object enters a **trigger** collider (or something enters its trigger) |
| `OnTriggerEnd(ANY other)` | ...leaves it |

`other` is the other object's id. Pass it to the `Scene` module: `Scene.Name(other)`, `Scene.Destroy(other)`.

## Which class?

Kit uses the class whose name matches the file (`player.x` → `Player`, ignoring case), or the first class in the file. A script without a class can define plain `Init` and `Update` functions instead:

```x
TAKE Transform
VOID CREATE Update(FLOAT dt)[
    Transform.Rotate(0, 90 * dt, 0)
]
```

## A complete example

```x
# scripts/coin.x: spins, and disappears when the player touches it.
TAKE Transform, Entity, Scene

CREATE Coin[
    FLOAT Spin: 180.0

    VOID CREATE Update(FLOAT dt)[
        Transform.Rotate(0, Spin * dt, 0)
    ]

    VOID CREATE OnTrigger(ANY other)[
        IF(Scene.Name(other) == "Player")[
            DISPLAY["Coin collected!"]
            Entity.Destroy()
        ]
    ]
]
```

For `OnTrigger` to fire, give the coin a **Collider** with **Trigger** checked, and give the player a collider. See [Colliders](/physics/colliders/).

Next: [Engine modules →](/scripting/engine-modules/)
