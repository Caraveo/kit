# Ship: a free-flying space ship with a chase camera.
#
# Attach to the ship and press Play. Add a Camera named "Camera" to the scene
# (on its own or inside the ship) and the ship flies it as a chase camera.
#
#   Thrust          W / S
#   Strafe          A / D
#   Up / Down       Space / Ctrl
#   Pitch / Yaw     hold the right mouse button and move the mouse
#                   (or the arrow keys)
#   Roll            Q / E
#   Boost           hold Shift
#   Brake           hold X   (stops drifting and spinning)
#   Flight assist   Z        (on: the ship steadies itself, like an arcade ship;
#                             off: it coasts like a real spacecraft)
#   Camera zoom     scroll the mouse wheel
#
# Everything moves around the ship's own axes, so it can loop, roll and fly
# upside down freely, and the camera banks with it without ever flipping.
TAKE Transform, Input, Math, Scene, Entity

CREATE Ship[
    # --- Flight --------------------------------------------------------------
    EDIT FLOAT MaxSpeed: 20.0          # units per second, forward
    EDIT FLOAT StrafeSpeed: 10.0       # sideways and up/down
    EDIT FLOAT Acceleration: 25.0      # units per second, each second
    EDIT FLOAT BoostMultiplier: 2.5    # speed and acceleration while Shift is held
    EDIT FLOAT BrakeStrength: 40.0     # how hard X slows you down
    EDIT BOOL FlightAssist: TRUE       # start with the dampers on

    # --- Turning -------------------------------------------------------------
    EDIT FLOAT PitchRate: 90.0         # degrees per second at full stick
    EDIT FLOAT YawRate: 70.0
    EDIT FLOAT RollRate: 140.0
    EDIT FLOAT TurnResponse: 6.0       # higher = snappier turns, lower = heavier ship
    EDIT FLOAT MouseSensitivity: 0.12  # degrees per pixel of mouse movement
    EDIT BOOL MouseAlwaysSteers: FALSE # steer without holding the right button
    EDIT BOOL InvertY: FALSE
    EDIT BOOL NoseIsPlusZ: TRUE        # the model's nose points along +Z (untick if it flies backwards)

    # --- Chase camera ----------------------------------------------------------
    EDIT STRING CameraName: "Camera"
    EDIT FLOAT CameraDistance: 6.0     # behind the ship
    EDIT FLOAT CameraHeight: 1.6       # above the ship
    EDIT FLOAT CameraLookAhead: 8.0    # aims this far in front of the ship
    EDIT FLOAT CameraFollow: 5.0       # higher = tighter, lower = more swing
    EDIT FLOAT SpeedPullback: 0.5      # backs off this much more at top speed

    PUBLIC FLOAT Speed: 0.0            # current speed (shown while playing)

    VECTOR Velocity: VECTOR(0, 0, 0)
    VECTOR Spin: VECTOR(0, 0, 0)       # pitch, yaw, roll in degrees per second
    FLOAT Nose: 1.0
    ANY Cam: NULL
    BOOL CamIsChild: FALSE             # the camera sits inside the ship in the Hierarchy
    VECTOR CamOffset: VECTOR(0, 0, 0)
    VECTOR CamUp: VECTOR(0, 1, 0)

    VOID CREATE Init()[
        IF(NoseIsPlusZ)[
            Nose: -1.0
        ]
        Cam: Scene.Find(CameraName)
        GRAB Child IN Entity.Children[
            IF(EXIST Cam AND Child == Cam)[
                CamIsChild: TRUE
            ]
        ]
        CamOffset: CameraTarget(0)
        CamUp: Transform.Up
        IF(EXIST Cam)[
            DISPLAY["Ship: WASD thrust, Space/Ctrl up/down, right-drag or arrows to steer, Q/E roll, Shift boost, X brake, Z flight assist."]
        ] ELSE [
            DISPLAY["Ship: add a Camera named", CameraName, "to get a chase camera."]
        ]
    ]

    VOID CREATE Update(FLOAT dt)[
        IF(dt <= 0)[
            GIVE
        ]
        IF(Input.Pressed("Z"))[
            FlightAssist: NOT FlightAssist
            IF(FlightAssist)[
                DISPLAY["Flight assist ON"]
            ] ELSE [
                DISPLAY["Flight assist OFF: you'll keep drifting until you thrust the other way (or hold X)"]
            ]
        ]
        Turn(dt)
        Fly(dt)
        Chase(dt)
    ]

    # 1 - e^(-rate * dt): eases toward a target at the same pace at any frame rate.
    FLOAT CREATE Blend(FLOAT Rate, FLOAT dt)[
        GIVE 1 - Math.Pow(Math.E, 0 - Rate * dt)
    ]

    # Moves Current toward Target by at most MaxStep (for acceleration limits).
    VECTOR CREATE Approach(VECTOR Current, VECTOR Target, FLOAT MaxStep)[
        VECTOR Gap: Target - Current
        FLOAT Length: Gap.Length()
        IF(Length <= MaxStep OR Length == 0)[
            GIVE Target
        ]
        GIVE Current + Gap * (MaxStep / Length)
    ]

    VOID CREATE Turn(FLOAT dt)[
        # Stick input from -1 to 1 on each axis.
        FLOAT Pitch: Input.Axis("Down", "Up")
        FLOAT Yaw: Input.Axis("Right", "Left")
        FLOAT Roll: Input.Axis("E", "Q")

        # The mouse adds on top, scaled so a quick flick turns fast.
        IF(MouseAlwaysSteers OR Input.Down("MouseRight"))[
            VECTOR M: Input.MouseDelta
            FLOAT Vertical: M.Y
            IF(InvertY)[
                Vertical: 0 - Vertical
            ]
            Pitch: Pitch - Vertical * MouseSensitivity / (PitchRate * dt)
            Yaw: Yaw - M.X * MouseSensitivity / (YawRate * dt)
        ]
        Pitch: Math.Clamp(Pitch, -2, 2)
        Yaw: Math.Clamp(Yaw, -2, 2)

        VECTOR Wanted: VECTOR(Pitch * PitchRate, Yaw * YawRate, Roll * RollRate)
        IF(Input.Down("X"))[
            Wanted: VECTOR(0, 0, 0)
        ]
        # Angular inertia: the ship eases into and out of turns.
        Spin: Spin + (Wanted - Spin) * Blend(TurnResponse, dt)
        # Turning left/right is the same whichever way the model faces; pitch and
        # roll follow the nose.
        Transform.Rotate(Spin.X * Nose * dt, Spin.Y * dt, Spin.Z * Nose * dt)
    ]

    VOID CREATE Fly(FLOAT dt)[
        VECTOR F: Transform.Forward * Nose
        VECTOR R: Transform.Right * Nose
        VECTOR U: Transform.Up

        FLOAT Thrust: Input.Axis("S", "W")
        FLOAT Side: Input.Axis("A", "D")
        FLOAT Lift: Input.Axis("Ctrl", "Space")

        FLOAT Boost: 1.0
        IF(Input.Down("Shift"))[
            Boost: BoostMultiplier
        ]
        FLOAT Top: MaxSpeed * Boost

        IF(Input.Down("X"))[
            Velocity: Approach(Velocity, VECTOR(0, 0, 0), BrakeStrength * dt)
        ] ELSE IF(FlightAssist)[
            # The dampers steer velocity toward what the stick asks for, so the
            # ship follows its nose and settles when you let go.
            VECTOR Wanted: F * (Thrust * Top) + R * (Side * StrafeSpeed * Boost) + U * (Lift * StrafeSpeed * Boost)
            Velocity: Approach(Velocity, Wanted, Acceleration * Boost * dt)
        ] ELSE [
            # Newtonian: thrust adds velocity and nothing takes it away.
            VECTOR Push: F * Thrust + R * Side + U * Lift
            IF(Push.Length() > 1)[
                Push: Push.Normalize()
            ]
            Velocity: Velocity + Push * (Acceleration * Boost * dt)
            IF(Velocity.Length() > Top)[
                # Over the limit (e.g. after letting go of boost): bleed off gently.
                Velocity: Approach(Velocity, Velocity.Normalize() * Top, Acceleration * dt)
            ]
        ]

        Speed: Velocity.Length()
        Transform.Move(Velocity * dt)
    ]

    # Where the camera wants to sit, relative to the ship.
    VECTOR CREATE CameraTarget(FLOAT SpeedFraction)[
        FLOAT Distance: CameraDistance * (1 + SpeedPullback * SpeedFraction)
        GIVE Transform.Forward * (0 - Nose * Distance) + Transform.Up * CameraHeight
    ]

    VOID CREATE Chase(FLOAT dt)[
        IF(NOT EXIST Cam)[
            GIVE
        ]
        FLOAT Wheel: Input.Scroll
        IF(Wheel != 0)[
            CameraDistance: Math.Clamp(CameraDistance * Math.Pow(1.15, 0 - Wheel), 1.5, 60)
        ]

        # The offset swings around the ship (instead of the camera trailing in
        # world space), so it stays steady even at high speed.
        FLOAT SpeedFraction: Math.Clamp(Speed / (MaxSpeed * BoostMultiplier), 0, 1)
        FLOAT K: Blend(CameraFollow, dt)
        CamOffset: CamOffset + (CameraTarget(SpeedFraction) - CamOffset) * K
        CamUp: (CamUp + (Transform.Up - CamUp) * K).Normalize()

        VECTOR P: Transform.WorldPosition
        VECTOR Eye: P + CamOffset
        VECTOR Look: (P + Transform.Forward * (Nose * CameraLookAhead) - Eye).Normalize()
        IF(CamIsChild)[
            # A child's position and rotation are relative to the ship.
            VECTOR S: Transform.Scale
            VECTOR L: ToShip(CamOffset)
            Scene.SetPosition(Cam, VECTOR(L.X / S.X, L.Y / S.Y, L.Z / S.Z))
            Scene.SetRotation(Cam, Orientation(ToShip(Look), ToShip(CamUp)))
        ] ELSE [
            Scene.SetPosition(Cam, Eye)
            Scene.SetRotation(Cam, Orientation(Look, CamUp))
        ]
    ]

    # A world direction expressed along the ship's own X, Y and Z axes.
    VECTOR CREATE ToShip(VECTOR V)[
        GIVE VECTOR(V.Dot(Transform.Right), V.Dot(Transform.Up), 0 - V.Dot(Transform.Forward))
    ]

    # Euler angles (degrees, Kit's X-Y-Z order) that point -Z along Look with
    # the given up direction. Works at any angle, including straight up/down.
    VECTOR CREATE Orientation(VECTOR Look, VECTOR UpHint)[
        VECTOR Right: Look.Cross(UpHint)
        IF(Right.Length() < 0.0001)[
            Right: Look.Cross(VECTOR(0, 0, 1))  # up is parallel to look: pick any side
        ]
        Right: Right.Normalize()
        VECTOR Up: Right.Cross(Look)
        VECTOR Back: Look * -1
        FLOAT B: Math.Asin(Math.Clamp(Back.X, -1, 1))
        FLOAT A: 0.0
        FLOAT C: 0.0
        IF(Math.Abs(Back.X) < 0.9999)[
            A: Math.Atan2(0 - Back.Y, Back.Z)
            C: Math.Atan2(0 - Up.X, Right.X)
        ] ELSE [
            A: Math.Atan2(Up.Z, Up.Y)
        ]
        GIVE VECTOR(Math.Degrees(A), Math.Degrees(B), Math.Degrees(C))
    ]
]
