---
title: Project structure
description: What's inside a Kit project folder.
---

```
my-game/
├─ kit.project          # settings (TOML)
├─ main.kscene          # the main scene
├─ scenes/              # more scenes
├─ scripts/             # x scripts
├─ sprites/             # images
└─ …                    # any other folders you like
```

All paths inside scenes and scripts are **relative to the project folder** and use `/`, for example `scripts/player.x` and `sprites/hero.png`. A path can't point outside the project.

Projects are plain files, so they work well with **git**. Commit everything except build output.

Open a project from **Project → Open project…**, from the recent list, or from the command line: `kit-editor path/to/my-game`.
