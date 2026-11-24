---
title: Core concepts
description: Projects, scenes, objects, components, modules and scripts.
---

| Concept | What it is |
|---|---|
| **Project** | A folder with a `kit.project` file. Everything in your game lives inside it. |
| **Scene** | A `.kscene` file: a list of objects. The project's *main scene* loads when the game starts. |
| **Object** (entity) | Something in a scene. On its own it has a name and a position; components give it everything else. |
| **Component** | A piece of data and behavior on an object: **Model**, **Sprite**, **Camera**, **Light**, **Script**, **Collider**, **Physics**, **Path**, **Terrain**. |
| **Hierarchy** | Objects can be children of other objects; children move with their parent. |
| **Script** | An `.x` file attached to an object. Kit calls its `Init` and `Update` methods. |
| **Module** | A part of the engine (rendering, input, physics…). Modules are listed under [Architecture](/engine/architecture/). |

## 2D and 3D

Kit treats 2D and 3D the same way. A **Sprite** is a flat picture in the world, and a **Model** is a 3D mesh. The camera decides how the game looks:

- A **Perspective** camera gives a 3D game.
- An **Orthographic (2D)** camera looks straight at the XY plane and gives a 2D game.

You can mix the two in one scene. In the editor, toggle the viewport between **3D** and **2D** with <kbd>T</kbd>.

## Edit mode and play mode

In **edit mode** nothing moves on its own: scripts, physics and paths are paused. Press <kbd>F5</kbd> for **play mode** to run the game inside the editor. When you stop, Kit restores the scene exactly as it was, so you can't break your level by testing it.
