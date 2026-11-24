# FreeCamera: fly a camera around the scene.
#
# Attach to any object with a Camera (or anything else, really) and press Play.
#
#   Look      hold the right mouse button and move the mouse
#             (or use the arrow keys)
#   Move      W A S D
#   Up/Down   E / Q   (Space / Ctrl also work)
#   Faster    hold Shift
#   Speed     scroll the mouse wheel to change the base speed
#
# Movement eases in and out so the camera glides instead of snapping.
TAKE Transform, Input, Math

CREATE FreeCamera[
    FLOAT Speed: 5.0           # units per second
    FLOAT FastMultiplier: 3.0  # while Shift is held
    FLOAT MinSpeed: 0.5
    FLOAT MaxSpeed: 100.0
    FLOAT LookSensitivity: 0.15  # degrees per pixel of mouse movement
    FLOAT KeyLookSpeed: 90.0     # degrees per second with the arrow keys
    FLOAT Smoothing: 10.0        # higher = snappier, lower = floatier
    BOOL InvertY: FALSE

    FLOAT Yaw: 0.0     # degrees around the world Y axis
    FLOAT Pitch: 0.0   # degrees up (+) or down (-)
    VECTOR Velocity: VECTOR(0, 0, 0)

    VOID CREATE Init()[
        # Start looking wherever the camera already points.
        VECTOR F: Transform.Forward
        Yaw: Math.Degrees(Math.Atan2(0 - F.X, 0 - F.Z))
        Pitch: Math.Degrees(Math.Asin(Math.Clamp(F.Y, -1, 1)))
        DISPLAY["FreeCamera: right-drag to look, WASD to move, E/Q up/down, Shift faster, scroll for speed."]
    ]

    VOID CREATE Update(FLOAT dt)[
        Look(dt)
        Fly(dt)
    ]

    VOID CREATE Look(FLOAT dt)[
        IF(Input.Down("MouseRight"))[
            VECTOR M: Input.MouseDelta
            FLOAT Vertical: M.Y
            IF(InvertY)[
                Vertical: 0 - Vertical
            ]
            Yaw: Yaw - M.X * LookSensitivity
            Pitch: Pitch - Vertical * LookSensitivity
        ]
        Yaw: Yaw + Input.Axis("Right", "Left") * KeyLookSpeed * dt
        Pitch: Pitch + Input.Axis("Down", "Up") * KeyLookSpeed * dt
        # Stop just short of straight up/down so the view never flips.
        Pitch: Math.Clamp(Pitch, -89, 89)

        FLOAT Y: Math.Radians(Yaw)
        FLOAT P: Math.Radians(Pitch)
        VECTOR Ahead: VECTOR(0 - Math.Sin(Y) * Math.Cos(P), Math.Sin(P), 0 - Math.Cos(Y) * Math.Cos(P))
        Transform.LookAt(Transform.Position + Ahead)
    ]

    VOID CREATE Fly(FLOAT dt)[
        # Scroll changes the cruising speed.
        FLOAT Wheel: Input.Scroll
        IF(Wheel != 0)[
            Speed: Math.Clamp(Speed * Math.Pow(1.15, Wheel), MinSpeed, MaxSpeed)
        ]

        FLOAT Forward: Input.Axis("S", "W")
        FLOAT Side: Input.Axis("A", "D")
        FLOAT Lift: Math.Clamp(Input.Axis("Q", "E") + Input.Axis("Ctrl", "Space"), -1, 1)

        # Walk relative to where the camera faces; lift is always straight up.
        VECTOR Wish: Transform.Forward * Forward + Transform.Right * Side + VECTOR(0, Lift, 0)
        FLOAT Length: Math.Sqrt(Wish.X * Wish.X + Wish.Y * Wish.Y + Wish.Z * Wish.Z)
        IF(Length > 1)[
            Wish: Wish * (1 / Length)  # diagonals aren't faster
        ]

        FLOAT Current: Speed
        IF(Input.Down("Shift"))[
            Current: Speed * FastMultiplier
        ]

        # Ease toward the target velocity (frame-rate independent).
        FLOAT Blend: 1 - Math.Pow(2.718281828, 0 - Smoothing * dt)
        Velocity: Velocity + (Wish * Current - Velocity) * Blend
        Transform.Move(Velocity * dt)
    ]
]
