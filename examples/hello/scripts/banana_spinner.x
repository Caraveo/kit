# Rotates the object around its Y axis.
TAKE Transform

CREATE BananaSpinner[
    EDIT FLOAT RotationSpeed: 90.0    # degrees per second

    VOID CREATE Update(FLOAT dt)[
        Transform.Rotate(0, RotationSpeed * dt, 0)
    ]
]
