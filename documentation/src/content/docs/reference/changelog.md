---
title: Changelog
description: What's new in each version.
---

## v0.0.2: Diagrams and Effects

- **Diagrams:** visual logic with nodes and wires. Press <kbd>7</kbd>. It runs in the editor, the players and the browser, and lights up live while you play. See [Diagrams](/editor/diagrams/).
- **Game Data:** datasets and constants that diagrams read by stable IDs.
- **Effects:** post-processing volumes for the camera or for the objects inside them. See [Post-processing](/editor/post-processing/).
- **Protected exports:** signed, or signed and encrypted.
- **x:** `PUBLIC` and `EDIT` fields (private by default).
- **Editor:**
  - <kbd>1</kbd>–<kbd>7</kbd> toggle the viewport bar.
  - <kbd>W</kbd>/<kbd>S</kbd> zoom and <kbd>A</kbd>/<kbd>D</kbd> pan.
  - **Import files…** for everything Kit uses.
  - The console starts collapsed.
  - New icon.
  - Many bug fixes.

## v0.0.1

The first release.

- **Engine:** ECS, scene graph, Vulkan 1.3 renderer (lit 3D models, sprites, MSAA, hot-reloaded textures), input, projects and scenes.
- **x scripting:** classes, `RIDE` inheritance, strict types, `TAKE` modules, hot reload, lifecycle and collision hooks.
- **Physics:** colliders, triggers, rigid bodies, ray casts and world settings (Rapier).
- **Terrain:** generated hills, sculpt brushes (raise, lower, smooth, flatten), automatic height and slope coloring, and a heightfield collider.
- **Canvas mode:** draw 3D paths on camera-facing sheets that extend stroke by stroke, with draggable points, smooth curves and ground mode. Physics objects fly the path, then land.
- **Editor:**
  - Camera and light gizmos, a camera preview, and framing on select.
  - Objects, inspector and grouped assets.
  - Undo/redo, play mode, console and an x code editor.
  - Project settings and export.
- **Platforms:** Linux, Windows and macOS packages.
