---
title: Recipes
description: Copy-paste scripts for common game behaviors.
---

## Move with the keyboard

```x
TAKE Transform, Input
CREATE Mover[
    FLOAT Speed: 4.0
    VOID CREATE Update(FLOAT dt)[
        Transform.Move(Input.Axis("A", "D") * Speed * dt, 0, Input.Axis("W", "S") * Speed * dt)
    ]
]
```

## Jump (physics)

Give the object **Physics** (Dynamic, *Lock rotation* on) and a collider.

```x
TAKE Input, Physics, Transform
CREATE Jumper[
    FLOAT Force: 5.0
    VOID CREATE Update(FLOAT dt)[
        # On the ground if a short ray down hits something.
        ANY Hit: Physics.Raycast(Transform.WorldPosition, VECTOR(0, -1, 0), 0.6)
        IF(Input.Pressed("Space") AND EXIST Hit)[
            Physics.ApplyImpulse(0, Force, 0)
        ]
    ]
]
```

## Follow another object

```x
TAKE Transform, Scene
CREATE Follow[
    STRING Target: "Player"
    FLOAT Speed: 2.0
    VOID CREATE Update(FLOAT dt)[
        ANY T: Scene.Find(Target)
        IF(EXIST T)[
            VECTOR To: Scene.WorldPosition(T) - Transform.WorldPosition
            IF(To.Length() > 1.5)[
                Transform.Move(To.Normalize() * Speed * dt)
            ]
        ]
    ]
]
```

## Spin and bob

```x
TAKE Transform, Time, Math
CREATE Spinner[
    VECTOR Start: VECTOR()
    VOID CREATE Init()[ Start: Transform.Position ]
    VOID CREATE Update(FLOAT dt)[
        Transform.Rotate(0, 45 * dt, 0)
        Transform.Position: Start + VECTOR(0, Math.Sin(Time.Elapsed * 2) * 0.25, 0)
    ]
]
```

## Respawn when falling off the world

```x
TAKE Transform
CREATE Respawn[
    VECTOR Start: VECTOR()
    VOID CREATE Init()[ Start: Transform.Position ]
    VOID CREATE Update(FLOAT dt)[
        IF(Transform.Position.Y < -10)[
            Transform.Position: Start
        ]
    ]
]
```

## Trigger zone

Give a box a **Collider** with **Trigger** checked:

```x
TAKE Scene
CREATE Goal[
    VOID CREATE OnTrigger(ANY other)[
        IF(Scene.Name(other) == "Player")[
            DISPLAY["You win!"]
        ]
    ]
]
```

## Start a sketched path on a key

Draw a path in [Canvas mode](/editor/canvas-mode/) and turn off **Play on start**:

```x
TAKE Input, Path
CREATE Elevator[
    VOID CREATE Update(FLOAT dt)[
        IF(Input.Pressed("E"))[ Path.Play() ]
    ]
]
```
