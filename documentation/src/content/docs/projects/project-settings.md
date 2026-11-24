---
title: Project settings
description: Title, main scene, window, and physics.
---

**Project → Project settings…** edits `kit.project`:

| Section | Setting | |
|---|---|---|
| Project | **Title** | The game's name. Export uses it for the folder and program name. |
| | **Version** | Your game's version. |
| | **Main scene** | The scene the game starts with. |
| Window | **Size** | Window size when the game starts. |
| | **VSync**, **Fullscreen** | Press <kbd>F11</kbd> in a running game to toggle fullscreen. |
| Physics | see [World physics settings](/physics/settings/) | |

A complete `kit.project`:

```toml
[project]
title = "Hello Kit"
version = "0.0.1"
main_scene = "main.kscene"

[display]
width = 1280
height = 720
vsync = true
fullscreen = false

[physics]
enabled = true
gravity = [0.0, -9.81, 0.0]
rate = 60
```
