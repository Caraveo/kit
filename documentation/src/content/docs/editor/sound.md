---
title: Sound
description: Add sounds by dragging files into Kit, then give objects Audio Sources with proximity.
---

## Adding sounds

Drag sound files from your file manager into the Kit window. Kit copies them into your project's `sounds/` folder and lists them under **Assets → Sounds**.

Kit plays these open formats: **WAV, MP3, Ogg Vorbis (.ogg), FLAC and AAC (.m4a / .aac)**. Files Kit can't play are refused with a message when you drop them.

- **Listen:** click a sound in Assets. Click it again to stop.
- **Add to the scene:** double-click it, or drag it onto the viewport. Kit creates a sound object (shown as ♪) where you drop it.
- **Give an object a sound:** drag the sound onto that object in the Objects list. It gets an **Audio Source**, or its existing one switches to the new sound.

Dropping a file from your file manager straight onto the viewport does both at once: it imports the file and places it.

## The Audio Source component

Sounds can be their own objects or a component on any object: a radio on a table, footsteps on the player. Add one with **+ Add component → ♪ Audio Source**.

| Setting | What it does |
|---|---|
| **Sound** | The file to play. ▶ previews it in the editor. |
| **Volume** | 0 is silent, 1 is as recorded, up to 2. |
| **Pitch** | Playback speed: 2× is an octave higher and twice as fast. |
| **Loop** | Start again at the end (music, ambience, engines). |
| **Play on start** | Play when the game starts. Otherwise, start it from a script. |
| **3D sound (proximity)** | Louder near the listener and panned left/right. Off = the same everywhere (music, interface sounds). |
| **Min dist / Max** | Full volume within Min, silent beyond Max. |
| **Falloff** | How it fades between them: **Natural** (like real sound), **Linear**, or **Smooth** (stays loud, then drops). |

The listener is the **active camera**. With a 3D sound selected, the viewport shows its Min distance (solid) and Max distance (faint) as spheres, with rings on the ground. The inspector draws the volume curve.

Objects placed from Assets start as 3D sounds. An Audio Source added to a camera starts as a 2D one.

## Sounds in scripts

```
TAKE Audio

CREATE Door[
    VOID CREATE OnTrigger(ANY other)[
        Audio.Play()                         # this object's Audio Source
        Audio.PlaySound("sounds/creak.wav")  # a one-shot, from this object
    ]
]
```

| Member | |
|---|---|
| `Audio.Play()` / `Audio.Stop()` | Start (or restart) / stop this object's Audio Source. |
| `Audio.Playing` | `TRUE` while it plays. |
| `Audio.Volume`, `Audio.SetVolume(v)` | Read or change the volume. |
| `Audio.Pitch`, `Audio.SetPitch(p)` | Read or change the pitch. |
| `Audio.SetLoop(TRUE)` | Turn looping on or off. |
| `Audio.Clip`, `Audio.SetClip("sounds/x.wav")` | Read or change the sound file. |
| `Audio.PlaySound("sounds/x.wav", volume, pitch)` | Play a file once (volume and pitch are optional). If this object has a 3D Audio Source, it plays from here with that proximity. |
| `Audio.StopAll()` | Silence everything. |

## On every platform

The same mixer runs on desktop and in web builds, so sounds behave identically. On Linux, Kit uses the system's sound (PipeWire, PulseAudio or ALSA) with no extra packages. In web builds, browsers only allow sound after the player clicks or presses a key, so sound starts then.

If a computer has no sound device, the game runs silently and the editor says so.
