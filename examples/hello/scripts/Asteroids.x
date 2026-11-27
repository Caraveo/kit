# Asteroids: an endless, procedural asteroid field around the ship.
#
# Attach to any object (an empty one works best) and press Play. The field is
# built from a seed, so the same rocks are always in the same places: fly away
# and come back and they're still there. Only the rocks near the ship exist;
# as you fly, the field streams in ahead of you and the rocks behind you are
# reused.
#
# The look of the rocks comes from template objects in your scene, so you
# design a rock once and the field copies it. Make a Sphere (or import a rock
# model), color it, name it "Asteroid", and put that name in Templates. List
# several, separated by commas ("Asteroid, Asteroid 2"), for variety. The
# templates are hidden while playing. Without a template, plain spheres are used.
#
# The object this script is on is the field's center, and the rocks are created
# inside it (they're listed under it in the Hierarchy while playing). With
# BeltThickness set, the rocks form a flat belt through that point instead of
# filling all of space.
TAKE Transform, Math, Scene, Time

CREATE Asteroids[
    # --- Field ---------------------------------------------------------------
    EDIT INTEGER Seed: 1337            # change for a different field
    EDIT STRING Templates: "Asteroid"  # objects to copy, separated by commas
    EDIT INTEGER MaxRocks: 260         # rocks alive at once
    EDIT FLOAT CellSize: 24.0          # the field is built in cubes this size
    EDIT INTEGER ViewCells: 3          # how many cubes out from the ship
    EDIT FLOAT Density: 1.0            # how packed the field is (rocks per cube, roughly)
    EDIT FLOAT ClusterSize: 90.0       # size of the dense and empty patches
    EDIT FLOAT BeltThickness: 0.0      # 0 = rocks everywhere; else a flat belt this thick
    EDIT FLOAT SafeRadius: 14.0        # keeps rocks away from where the ship starts

    # --- Rocks ---------------------------------------------------------------
    EDIT FLOAT MinSize: 0.5
    EDIT FLOAT MaxSize: 7.0            # most rocks are small; a few are huge
    EDIT FLOAT Lumpiness: 0.35         # 0 = round, higher = squashed and stretched
    EDIT FLOAT TumbleSpeed: 30.0       # degrees per second (big rocks turn slower)
    EDIT FLOAT Drift: 1.2              # how far rocks wander from their spot

    # --- Ship ----------------------------------------------------------------
    EDIT STRING ShipName: "Ship"
    EDIT BOOL Collide: TRUE            # the ship can't fly through rocks
    EDIT FLOAT ShipRadius: 0.6
    EDIT FLOAT RockRadius: 0.5         # template radius at scale 1 (0.5 for a Sphere)

    PUBLIC INTEGER Visible: 0          # rocks in view right now

    INDEX Sources: []    # template ids
    INDEX Bases: []      # each template's own scale
    INDEX Rocks: []      # pool of spawned rock ids
    INDEX Kinds: []      # which template each rock copies
    INDEX Home: []       # where each active rock sits
    INDEX Shape: []      # its scale
    INDEX Turn: []       # tumble, degrees per second on each axis
    INDEX Phase: []      # starting angles
    INDEX Radius: []
    INDEX Offsets: []    # nearby cubes, nearest first
    INTEGER Used: 0
    ANY Ship: NULL
    VECTOR Center: VECTOR(0, 0, 0)
    VECTOR FieldScale: VECTOR(1, 1, 1)
    VECTOR Start: VECTOR(0, 0, 0)
    VECTOR Cell: VECTOR(0.5, 0.5, 0.5)  # not a real cube, so the first frame builds

    VOID CREATE Init()[
        Center: Transform.WorldPosition
        FieldScale: Transform.Scale
        Ship: Scene.Find(ShipName)
        Start: Center
        IF(EXIST Ship)[
            Start: Scene.WorldPosition(Ship)
        ]
        FindTemplates()
        BuildOffsets()
        SpawnPool()
    ]

    VOID CREATE Update(FLOAT dt)[
        VECTOR Here: Center
        IF(EXIST Ship)[
            Here: Scene.WorldPosition(Ship)
        ]
        VECTOR Now: VECTOR(Math.Floor(Here.X / CellSize), Math.Floor(Here.Y / CellSize), Math.Floor(Here.Z / CellSize))
        IF(Now.X != Cell.X OR Now.Y != Cell.Y OR Now.Z != Cell.Z)[
            Cell: Now
            Build()
        ]
        Animate(Here)
    ]

    # ------------------------------------------------------------------ setup

    VOID CREATE FindTemplates()[
        GRAB Name IN Templates.Split(",")[
            STRING N: Name.Trim()
            IF(N.Length() > 0)[
                ANY Id: Scene.Find(N)
                IF(EXIST Id)[
                    Sources.Add(Id)
                    Bases.Add(Scene.Scale(Id))
                ] ELSE [
                    DISPLAY["Asteroids: no object named", N, "- using a plain sphere instead"]
                ]
            ]
        ]
    ]

    # Every cube within ViewCells of the ship, nearest first, so when the pool
    # runs out it's the farthest rocks that go missing.
    VOID CREATE BuildOffsets()[
        GRAB Ring IN RANGE(ViewCells + 1)[
            GRAB X IN RANGE(0 - Ring, Ring + 1)[
                GRAB Y IN RANGE(0 - Ring, Ring + 1)[
                    GRAB Z IN RANGE(0 - Ring, Ring + 1)[
                        IF(Math.Max(Math.Abs(X), Math.Max(Math.Abs(Y), Math.Abs(Z))) == Ring)[
                            Offsets.Add(VECTOR(X, Y, Z))
                        ]
                    ]
                ]
            ]
        ]
    ]

    VOID CREATE SpawnPool()[
        GRAB I IN RANGE(MaxRocks)[
            ANY Rock: NULL
            INTEGER Kind: 0
            IF(Sources.Length() > 0)[
                Kind: I % Sources.Length()
                Rock: INVOKE(Sources[Kind])
            ] ELSE [
                Rock: INVOKE("Sphere")
                Kind: -1
            ]
            Park(Rock)
            Rocks.Add(Rock)
            Kinds.Add(Kind)
        ]
        # Hide the templates themselves (Stop puts them back).
        GRAB Id IN Sources[
            Park(Id)
        ]
    ]

    VOID CREATE Park(ANY Rock)[
        Scene.SetPosition(Rock, VECTOR(0, -100000, 0))
        Scene.SetScale(Rock, 0.0001)
    ]

    # ------------------------------------------------------------- generation

    # A repeatable random number from 0 to 1 for any point and slot.
    FLOAT CREATE Hash(VECTOR P, FLOAT Slot)[
        FLOAT V: Math.Sin(P.X * 127.1 + P.Y * 311.7 + P.Z * 74.7 + Slot * 19.19 + Seed * 0.7311) * 43758.5453
        GIVE V - Math.Floor(V)
    ]

    FLOAT CREATE Smooth(FLOAT T)[
        GIVE T * T * (3 - 2 * T)
    ]

    # Smooth 3D noise from 0 to 1: makes dense clusters and empty gaps.
    FLOAT CREATE Noise(VECTOR P)[
        VECTOR C: VECTOR(Math.Floor(P.X), Math.Floor(P.Y), Math.Floor(P.Z))
        FLOAT FX: Smooth(P.X - C.X)
        FLOAT FY: Smooth(P.Y - C.Y)
        FLOAT FZ: Smooth(P.Z - C.Z)
        FLOAT A: Math.Lerp(Hash(C, 99), Hash(C + VECTOR(1, 0, 0), 99), FX)
        FLOAT B: Math.Lerp(Hash(C + VECTOR(0, 1, 0), 99), Hash(C + VECTOR(1, 1, 0), 99), FX)
        FLOAT D: Math.Lerp(Hash(C + VECTOR(0, 0, 1), 99), Hash(C + VECTOR(1, 0, 1), 99), FX)
        FLOAT E: Math.Lerp(Hash(C + VECTOR(0, 1, 1), 99), Hash(C + VECTOR(1, 1, 1), 99), FX)
        GIVE Math.Lerp(Math.Lerp(A, B, FY), Math.Lerp(D, E, FY), FZ)
    ]

    # Lay out the rocks for every cube around the ship's cube.
    VOID CREATE Build()[
        Home: []
        Shape: []
        Turn: []
        Phase: []
        Radius: []
        Used: 0
        GRAB O IN Offsets[
            IF(Used >= MaxRocks)[
                EXIT
            ]
            VECTOR C: Cell + O
            VECTOR Mid: (C + VECTOR(0.5, 0.5, 0.5)) * CellSize
            # Skip whole cubes outside the belt.
            IF(BeltThickness > 0 AND Math.Abs(Mid.Y - Center.Y) > BeltThickness * 0.5 + CellSize)[
                SKIP
            ]
            FLOAT Patchy: Smooth(Noise(Mid * (1 / ClusterSize)))
            FLOAT Count: Math.Floor(Density * 2.2 * Patchy * Patchy + Hash(C, 0))
            GRAB K IN RANGE(INTEGER(Math.Min(Count, 6)))[
                IF(Used >= MaxRocks)[
                    EXIT
                ]
                Place(C, K)
            ]
        ]
        # Park whatever the pool didn't need this time.
        GRAB I IN RANGE(Used, Rocks.Length())[
            Park(Rocks[I])
        ]
    ]

    VOID CREATE Place(VECTOR C, INTEGER K)[
        FLOAT S: K * 10 + 1
        VECTOR P: (C + VECTOR(Hash(C, S), Hash(C, S + 1), Hash(C, S + 2))) * CellSize
        IF(BeltThickness > 0)[
            P: VECTOR(P.X, Center.Y + (Hash(C, S + 3) - 0.5) * BeltThickness, P.Z)
        ]
        IF(P.Distance(Start) < SafeRadius)[
            GIVE
        ]
        # Cubing the random number makes most rocks small and a few huge.
        FLOAT R: Hash(C, S + 4)
        FLOAT Size: MinSize + (MaxSize - MinSize) * R * R * R
        VECTOR Lumps: VECTOR(1 + (Hash(C, S + 5) - 0.5) * 2 * Lumpiness, 1 + (Hash(C, S + 6) - 0.5) * 2 * Lumpiness, 1 + (Hash(C, S + 7) - 0.5) * 2 * Lumpiness)
        FLOAT Slow: TumbleSpeed / Math.Sqrt(Size)
        Home.Add(P)
        Shape.Add(Lumps * Size)
        Turn.Add(VECTOR((Hash(C, S + 8) - 0.5) * 2 * Slow, (Hash(C, S + 9) - 0.5) * 2 * Slow, (Hash(C, S + 10) - 0.5) * 2 * Slow))
        Phase.Add(VECTOR(Hash(C, S + 11) * 360, Hash(C, S + 12) * 360, Hash(C, S + 13) * 360))
        Radius.Add(RockRadius * Size * Math.Min(Lumps.X, Math.Min(Lumps.Y, Lumps.Z)))
        Used: Used + 1
    ]

    # ---------------------------------------------------------------- per frame

    # A world position as seen from this object (its rocks live inside it).
    VECTOR CREATE ToField(VECTOR P)[
        VECTOR D: P - Center
        GIVE VECTOR(D.Dot(Transform.Right) / FieldScale.X, D.Dot(Transform.Up) / FieldScale.Y, (0 - D.Dot(Transform.Forward)) / FieldScale.Z)
    ]

    VOID CREATE Animate(VECTOR Here)[
        FLOAT T: Time.Elapsed
        FLOAT Edge: ViewCells * CellSize
        Visible: 0
        GRAB I IN RANGE(Used)[
            VECTOR Ph: Phase[I]
            VECTOR P: Home[I] + VECTOR(Math.Sin(T * 0.13 + Ph.X), Math.Sin(T * 0.11 + Ph.Y), Math.Sin(T * 0.09 + Ph.Z)) * Drift
            # Rocks grow in at the edge of the field instead of popping in.
            FLOAT Fade: Math.Clamp((Edge - P.Distance(Here)) / (CellSize * 0.75), 0, 1)
            Fade: Smooth(Fade)
            ANY Rock: Rocks[I]
            IF(Fade <= 0)[
                Scene.SetScale(Rock, 0.0001)
                SKIP
            ]
            Visible: Visible + 1
            VECTOR Size: Shape[I] * Fade
            INTEGER Kind: Kinds[I]
            IF(Kind >= 0)[
                VECTOR B: Bases[Kind]
                Size: VECTOR(Size.X * B.X, Size.Y * B.Y, Size.Z * B.Z)
            ]
            # The rocks are inside this object, so they're placed relative to it.
            Scene.SetPosition(Rock, ToField(P))
            Scene.SetRotation(Rock, Ph + Turn[I] * T)
            Scene.SetScale(Rock, VECTOR(Size.X / FieldScale.X, Size.Y / FieldScale.Y, Size.Z / FieldScale.Z))

            IF(Collide AND EXIST Ship)[
                FLOAT Reach: Radius[I] * Fade + ShipRadius
                VECTOR Away: Here - P
                FLOAT D: Away.Length()
                IF(D < Reach AND D > 0)[
                    # Slide the ship out to the rock's surface.
                    Here: P + Away * (Reach / D)
                    Scene.SetPosition(Ship, Here)
                ]
            ]
        ]
    ]
]
