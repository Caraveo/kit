# Spinner: turns the object and bobs it up and down.
TAKE Transform, Time, Math

CREATE Spinner[
    FLOAT Speed: 45.0
    FLOAT Height: 0.25
    VECTOR Start: VECTOR()

    VOID CREATE Init()[
        Start: Transform.Position
    ]

    VOID CREATE Update(FLOAT dt)[
        Transform.Rotate(0, Speed * dt, 0)
        FLOAT Y: Math.Sin(Time.Elapsed * 2.0) * Height
        Transform.Position: Start + VECTOR(0, Y, 0)
    ]
]
