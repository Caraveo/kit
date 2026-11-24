---
title: Objects & components
description: Add objects and give them components in the inspector.
---

## Adding objects

Use **＋ New → Object**, the **+** above the object list, or right-click → **Add child**:

| Object | What you get |
|---|---|
| Empty | Just a transform. Good for grouping. |
| Cube, Sphere, Plane, Cylinder, Torus, Quad | A **Model** with that mesh. A new Plane is sized as a 10 × 10 floor. |
| Sprite | A flat picture (2D). |
| Camera / Camera (2D) | A perspective or orthographic camera. |
| Light | A directional light (sun). |

New objects appear near the center of the view. Duplicate with <kbd>Ctrl</kbd> + <kbd>D</kbd> and delete with <kbd>Del</kbd>.

## Components

Each component is a section in the inspector. Remove one with its **✖** button. Add more with **＋ Add component**.

### Transform
**Position**, **Rotation** (degrees) and **Scale**. Every object has one. Children inherit their parent's transform.

### Model
A 3D mesh: **Mesh**, **Color**, **Texture** (any PNG/JPG in the project), **Unlit** (ignore lighting) and **Visible**. A color with transparency (alpha below 100%) makes the model see-through.

### Sprite
A picture in the XY plane: **Size** in world units, **Color** (tint and transparency), **Texture**, **Flip X/Y** and **Visible**.

### Camera
- **Perspective**: field of view, near and far clip planes.
- **2D / Ortho**: the visible height in world units.

Also **Clear** (background color) and **Active**. The game uses the first active camera in the object list.

### Light
A directional light shining along the object's forward (−Z) axis: **Color** and **Intensity**. Without any light, Kit uses a default sun.

### Script
Attaches an `.x` script. Pick the **File**, click **Edit** to open it, or click **New script** to create one named after the object. While playing, this section shows the script's live field values. See [Attaching scripts](/scripting/attaching-scripts/).

### Collider, Physics
See [Colliders](/physics/colliders/) and [Physics bodies](/physics/physics-bodies/).

### Path
A movement path drawn in [Canvas mode](/editor/canvas-mode/).

## Colors

Colors in the inspector are what you see on screen (sRGB). The hex code next to the swatch is handy for copying colors between objects.
