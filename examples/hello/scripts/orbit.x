# Orbit: circles around the parent's origin.
TAKE Transform, Time, Math

CREATE Orbit[
    EDIT FLOAT Radius: 1.6
    EDIT FLOAT Speed: 1.4

    VOID CREATE Update(FLOAT dt)[
        FLOAT A: Time.Elapsed * Speed
        Transform.Position: VECTOR(Math.Cos(A) * Radius, 0.3 * Math.Sin(A * 2.0), Math.Sin(A) * Radius)
    ]
]
