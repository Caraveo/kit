# Player: move with WASD or the arrow keys, Space to change color.
TAKE Transform, Input, Model, Math, Scene

CREATE Actor[
    FLOAT Speed: 3.0
    INTEGER Moves: 0

    VOID CREATE Step(FLOAT x, FLOAT z, FLOAT dt)[
        IF(x != 0 OR z != 0)[
            Transform.Move(x * Speed * dt, 0, z * Speed * dt)
            Moves: Moves + 1
        ]
    ]
]

# Player RIDEs (extends) Actor and adds input handling.
CREATE Player RIDE Actor[
    INDEX Colors: [VECTOR(0.76, 0.66, 1.0), VECTOR(1.0, 0.6, 0.8), VECTOR(0.6, 0.95, 0.8), VECTOR(1.0, 0.85, 0.5)]
    INTEGER Current: 0

    VOID CREATE Init()[
        DISPLAY["Hello from x! WASD / arrows to move, Space to recolor."]
    ]

    VOID CREATE Update(FLOAT dt)[
        FLOAT X: Input.Axis("A", "D") + Input.Axis("Left", "Right")
        FLOAT Z: Input.Axis("W", "S") + Input.Axis("Up", "Down")
        Step(Math.Clamp(X, -1, 1), Math.Clamp(Z, -1, 1), dt)

        IF(Input.Pressed("Space"))[
            Current: (Current + 1) % Colors.Length()
            Model.SetColor(Colors[Current])
            DISPLAY["Color", Current, "after", Moves, "moving frames"]
        ]
    ]

    # Called by the physics module when this collider touches another.
    VOID CREATE OnCollision(ANY other)[
        DISPLAY["Bumped into", Scene.Name(other)]
    ]
]
