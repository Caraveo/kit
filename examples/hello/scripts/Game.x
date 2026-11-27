# Game: runs the Crystal Run demo.
#
# Fly the Ship through the asteroid field and grab as many crystals as you
# can before time runs out. Shows off, in one small script:
#   * coroutines (the countdown and the round clock use WAIT)
#   * timers (Time.Every spawns crystals)
#   * INSTANTIATE with a Fab (fabs/Crystal.kfab)
#   * features: crystals call Collect() here through GetFeature
#   * Save (your best score survives restarts)
#   * input actions you can rebind (Input.Bind)
#   * the on-screen interface (Interface.SetText / Show / Hide)
#   * Diagram.Emit: the Beacon's diagram flashes on every crystal
#   * Debug lines: press G for a guide line to the nearest crystal
#   * the Cinematics Circuit: the round waits for the Intro cinematic, with
#     the ship switched off (.Enabled) and the HUD hidden meanwhile
TAKE Scene, Time, Input, Save, Interface, Diagram, Debug, Math, Transform, Audio, Cinematic

CREATE Game[
    EDIT INTEGER RoundSeconds: 60
    EDIT FLOAT SpawnEvery: 0.8        # seconds between new crystals
    EDIT INTEGER MaxCrystals: 18
    EDIT FLOAT SpawnNear: 12.0        # closest a crystal appears to the ship
    EDIT FLOAT SpawnFar: 45.0         # farthest
    EDIT STRING ShipName: "Ship"

    PUBLIC INTEGER Score: 0
    PUBLIC INTEGER Best: 0
    PUBLIC INTEGER TimeLeft: 0
    PUBLIC BOOL Playing: FALSE

    ANY Ship: NULL
    BOOL Guide: FALSE
    INTEGER Round: 0

    VOID CREATE Init()[
        Ship: Scene.Find(ShipName)
        Best: Save.Get("crystal-run-best", 0)
        # Actions: rename or rebind them in one place.
        Input.Bind("Restart", "R", "Enter", "PadStart")
        Input.Bind("Guide", "G", "PadY")
        Time.Every(SpawnEvery, "SpawnCrystal")
        ShowScore()
        Interface.SetText("best", "BEST " + STRING(Best))
        Interface.Hide("summary")
        Interface.Hide("message")
        NewRound()
    ]

    VOID CREATE NewRound()[
        Round: Round + 1
        Score: 0
        TimeLeft: RoundSeconds
        ShowScore()
        Interface.Hide("summary")
        Time.Start("RunRound", Round)
    ]

    # A coroutine: countdown, then the round clock, then the results.
    VOID CREATE RunRound(INTEGER which)[
        Playing: FALSE
        # First round: let the intro cinematic play (if the Circuit is installed).
        WAIT 0
        IF(which == 1 AND Cinematic.Playing)[
            ShowHud(FALSE)
            ANY Controls: GetFeature(Ship, "Ship")
            IF(EXIST Controls)[
                Controls.Enabled: FALSE
            ]
            LOOP(Cinematic.Playing)[
                WAIT 0
            ]
            IF(EXIST Controls)[
                Controls.Enabled: TRUE
            ]
            ShowHud(TRUE)
        ]
        GRAB N IN [3, 2, 1][
            Interface.SetText("message", STRING(N))
            Interface.Show("message")
            WAIT 0.7
        ]
        Interface.SetText("message", "GO!")
        Playing: TRUE
        GRAB I IN RANGE(4)[
            SpawnCrystal()
        ]
        WAIT 0.6
        Interface.Hide("message")
        LOOP(TimeLeft > 0 AND which == Round)[
            Interface.SetText("time", STRING(TimeLeft))
            IF(TimeLeft <= 10)[
                Interface.SetColor("time", 1.0, 0.45, 0.4)
            ] ELSE [
                Interface.SetColor("time", 0.85, 0.95, 1.0)
            ]
            WAIT 1
            TimeLeft: TimeLeft - 1
        ]
        IF(which != Round)[
            GIVE
        ]
        Playing: FALSE
        Interface.SetText("time", "0")
        STRING Line: "You collected " + STRING(Score) + " crystals"
        IF(Score > Best)[
            Best: Score
            Save.Set("crystal-run-best", Best)
            Interface.SetText("best", "BEST " + STRING(Best))
            Line: Line + " - a new best!"
        ]
        Interface.SetText("summary-score", Line)
        Interface.Show("summary")
    ]

    VOID CREATE Update(FLOAT dt)[
        IF(NOT Playing AND TimeLeft == 0 AND Input.Pressed("Restart"))[
            NewRound()
        ]
        IF(Input.Pressed("Guide"))[
            Guide: NOT Guide
        ]
        IF(Guide AND EXIST Ship)[
            DrawGuide()
        ]
    ]

    # Crystals call this through GetFeature(Game, "Game").Collect(...).
    VOID CREATE Collect(INTEGER points, VECTOR where)[
        IF(NOT Playing)[
            GIVE
        ]
        Score: Score + points
        ShowScore()
        Diagram.Emit("Collected", Score)
        Debug.Sphere(where, 1.2, VECTOR(0.5, 1, 1), 0.4)
    ]

    VOID CREATE ShowHud(BOOL on)[
        GRAB Id IN ["score-card", "time-card", "best-card", "controls"][
            IF(on)[
                Interface.Show(Id)
            ] ELSE [
                Interface.Hide(Id)
            ]
        ]
    ]

    VOID CREATE ShowScore()[
        Interface.SetText("score", STRING(Score))
    ]

    VOID CREATE SpawnCrystal()[
        IF(NOT Playing OR NOT EXIST Ship)[
            GIVE
        ]
        IF(LENGTH(Scene.FindTagged("Crystal")) >= MaxCrystals)[
            GIVE
        ]
        # Somewhere around the ship, a bit more likely ahead of it.
        VECTOR Here: Scene.WorldPosition(Ship)
        VECTOR Dir: VECTOR(Math.RandomRange(-1.0, 1.0), Math.RandomRange(-0.5, 0.5), Math.RandomRange(-1.0, 1.0)).Normalize()
        VECTOR Ahead: Scene.Forward(Ship) * -1
        Dir: (Dir + Ahead * 0.6).Normalize()
        VECTOR Spot: Here + Dir * Math.RandomRange(SpawnNear, SpawnFar)
        INSTANTIATE("fabs/Crystal.kfab", Spot)
    ]

    VOID CREATE DrawGuide()[
        VECTOR Here: Scene.WorldPosition(Ship)
        ANY Nearest: NULL
        FLOAT Best2: 1000000.0
        GRAB C IN Scene.FindTagged("Crystal")[
            FLOAT D: Scene.WorldPosition(C).Distance(Here)
            IF(D < Best2)[
                Best2: D
                Nearest: C
            ]
        ]
        IF(EXIST Nearest)[
            Debug.Line(Here, Scene.WorldPosition(Nearest), VECTOR(0.4, 1, 0.8))
        ]
    ]
]
