# Cinematics

Camera shots for intros, cutscenes and trailers, made in the editor.

Install it from **Project → Circuit Manager → Cinematics**.

## Make one

1. **+ Add object → Empty** and name it, e.g. *Intro*. **+ Add component →
   Circuits → Cinematic**. Tick **Play on start** if it should open the level.
2. Inside it, add a **Camera** for each shot, and frame each one the way you
   want. Give each **+ Add component → Circuits → Cinematic Shot**.
3. The shots play top to bottom, in Hierarchy order. For each one, set:
   - **Duration**.
   - **Transition in:** a *cut*, or a *blend* that glides from the previous
     view. The first shot can blend in from the game's camera.
   - **Look at:** keeps an object centered.
   - **Zoom to:** a field of view to reach by the end of the shot.
   - **Caption:** text at the bottom of the screen.
   - **Shake:** handheld wobble.
4. **To move a camera during its shot,** select it, press **5** for Canvas
   mode and sketch the path. It follows the path only while its shot plays.

While a cinematic plays, there are letterbox bars and a fade in. It fades to
black at the end, then fades back to the game's own camera. Players can skip
it with Esc, Space, Enter or Start. You can turn each of these off on the
Cinematic.

## From scripts

```x
TAKE Cinematic, Time

Cinematic.Play("Intro")
LOOP(Cinematic.Playing)[ WAIT 0 ]   # inside a coroutine: wait for it to end
Cinematic.Skip()                    # or Stop() to cut out at once
DISPLAY[Cinematic.Current, Cinematic.Shot, Cinematic.Time]
```

`Cinematic.Playing` is `FALSE` when the Circuit isn't installed, so your game
still runs without it.

## From diagrams

- **To start one:** an **Emit Event** node named `PlayCinematic`, with the
  cinematic's name as its text. Emit `StopCinematic` to stop it.
- **To react:** **On Custom Event** nodes for `CinematicStarted` and
  `CinematicFinished`, with the name as their payload.

## How it runs

The runtime (`runtime/`, the `kit_cinematics` crate) ships inside Kit's
players on every platform, including the web. It does nothing unless the
project has `kit.cinematics` installed and enabled in `circuits.lock`. Shots
are stored on objects as `"cinematic"` and `"cinematic_shot"` scene data, so
removing the Circuit leaves your scenes intact. Reinstalling it brings the
cinematics back.
