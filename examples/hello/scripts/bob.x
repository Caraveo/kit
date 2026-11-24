# Bob: a 2D sprite that floats and fades.
TAKE Transform, Sprite, Time, Math

FLOAT Base: 0.0

VOID CREATE Init()[
    Base: Transform.Position.Y
]

VOID CREATE Update(FLOAT dt)[
    FLOAT T: Time.Elapsed
    Transform.Position: VECTOR(Transform.Position.X, Base + Math.Sin(T * 1.5) * 0.15, Transform.Position.Z)
    Sprite.SetColor(1, 1, 1, 0.75 + 0.25 * Math.Sin(T * 3.0))
]
