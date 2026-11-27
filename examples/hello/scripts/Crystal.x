# Crystal: a collectible for the Crystal Run demo.
#
# Lives in fabs/Crystal.kfab; Game.x spawns copies with INSTANTIATE. It grows
# in, spins and bobs, and when the Ship flies through it, it tells the Game
# (through GetFeature) and shrinks away in a coroutine.
TAKE Transform, Time, Scene, Entity, Math

CREATE Crystal[
    EDIT FLOAT SpinSpeed: 120.0     # degrees per second
    EDIT FLOAT Bob: 0.35            # how far it floats up and down
    EDIT FLOAT Lifetime: 25.0       # seconds before it fades if nobody takes it
    EDIT INTEGER Points: 1

    BOOL Taken: FALSE
    FLOAT Seed: 0.0
    VECTOR Home: VECTOR(0, 0, 0)
    FLOAT Size: 1.0

    VOID CREATE Init()[
        Entity.AddTag("Crystal")
        Seed: Math.RandomRange(0.0, 100.0)
        Home: Transform.Position
        Size: 0.0
        Transform.SetScale(0.001)
        Time.Start("Appear")
        Time.After(Lifetime, "Expire")
    ]

    VOID CREATE Update(FLOAT dt)[
        Transform.Rotate(0, SpinSpeed * dt, 0)
        # Smooth noise makes each crystal drift a little differently.
        FLOAT T: Time.Elapsed * 0.6 + Seed
        Transform.Position: Home + VECTOR(0, (Math.Noise(T) - 0.5) * 2 * Bob, 0)
    ]

    # A coroutine: grow from nothing over a quarter of a second.
    VOID CREATE Appear()[
        LOOP(Size < 1 AND NOT Taken)[
            Size: Math.Min(1, Size + 0.08)
            Transform.SetScale(Size)
            WAIT 0
        ]
    ]

    # The Ship's collider overlaps this trigger.
    VOID CREATE OnTrigger(ANY other)[
        IF(Taken OR Scene.Name(other) != "Ship")[
            GIVE
        ]
        ANY Boss: Scene.Find("Game")
        IF(EXIST Boss)[
            Game G: GetFeature(Boss, "Game")
            IF(NOT G.Playing)[
                GIVE
            ]
            G.Collect(Points, Transform.Position)
        ]
        Taken: TRUE
        Time.Start("Vanish")
    ]

    VOID CREATE Expire()[
        IF(NOT Taken)[
            Taken: TRUE
            Time.Start("Vanish")
        ]
    ]

    # Shrink away, then remove the object.
    VOID CREATE Vanish()[
        LOOP(Size > 0.05)[
            Size: Size * 0.75
            Transform.SetScale(Size)
            WAIT 0
        ]
        Entity.Destroy()
    ]
]
