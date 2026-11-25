# Launches the character up and spins them in a backflip.
TAKE Transform, Physics, Input, Time

CREATE BackflipController[
    EDIT FLOAT JumpForce: 5.0      # Upward strength
    EDIT FLOAT SpinSpeed: 720.0    # Degrees per second
    
    BOOL IsFlipping: FALSE
    FLOAT FlipTimer: 0.0
    EDIT FLOAT FlipDuration: 0.8   # How long the spin lasts

    VOID CREATE Update(FLOAT dt)[
        # Trigger the flip when Space is pressed
        IF(Input.Pressed("Space") AND NOT IsFlipping)[
            IsFlipping: TRUE
            FlipTimer: 0.0
            
            # Launch the character up
            Physics.ApplyImpulse(VECTOR(0, JumpForce, 0))
            
            # Add a little forward momentum so they don't just flip in place
            Physics.ApplyImpulse(Transform.Forward * 2.0)
        ]

        IF(IsFlipping)[
            FlipTimer: FlipTimer + dt
            
            IF(FlipTimer < FlipDuration)[
                # Rotate the character on the X axis for a backflip
                Transform.Rotate(-SpinSpeed * dt, 0, 0)
            ] ELSE [
                IsFlipping: FALSE
            ]
        ]
    ]
]
