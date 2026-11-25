---
title: Rigg (character rigs)
description: Pose and animate a model with simple shapes inside it. No bones.
---

**Rigg** rigs a model with **volumes**: spheres for joints, capsules for limbs and boxes for the torso, placed inside the mesh. Move, turn or scale a volume and the mesh around it follows smoothly. There are no bones to set up and no weights to paint.

## Rig a model

1. Select a model and press <kbd>9</kbd> (or **Rig** in the viewport bar). The model turns see-through.
2. Click **+ Rig *model***, then **Humanoid preset**. Kit adds about 30 volumes fitted to the model's size, under a **Rig Volumes** object in the hierarchy. You can also add volumes one at a time with **+ Volume**.
3. With **Fit** selected, move and resize volumes until they sit inside the right parts of the body. In Fit the mesh doesn't bend; where you leave the volumes becomes the rest pose.
4. Switch to **Pose** and move or rotate volumes: the mesh now bends with them.

Volumes are ordinary objects, so you can select, move, rename, duplicate and delete them, and undo works as usual. Click a volume in the viewport to pick it, even inside the mesh.

## Tune a volume

Select a volume to see its **Rig Volume** settings in the Inspector:

| Setting | What it does |
|---|---|
| **Shape** | Sphere, Capsule or Box, with a diagram of the dimensions |
| **Radius / Length / Size** | The shape's size |
| **Influence** | How strongly it pulls the mesh (0–1) |
| **Falloff** | How far past its surface it still pulls, for softer or sharper bends |
| **Connected To** | Its parent and child volumes |

**Mirror L↔R** copies an `L_` volume's shape and pose to its `R_` twin, or the other way round.

Turn on **Weights** in the Rig toolbar to color the mesh by influence. With a volume selected, its area glows orange; otherwise each volume shows in its own color.

On the model, the **Rig** section has **Deform mesh** (turn the rig off without deleting it) and **Preserve volume**. Preserve volume keeps limbs from thinning where they bend or twist, and is on by default.

## Poses and animation

- **Poses:** type a name under **Poses** in the Rig section and click **Save pose**. **Apply** brings it back. **Reset pose** in the toolbar puts every volume back at rest.
- **Animation:** click **Animation** in the Rig toolbar to open the timeline.
  - Type a clip name (Idle, Walk, Jump…) and press **+**.
  - Pose the volumes and click **◆ Key** to record the pose at the current time. Turn on **Auto-key** to record a key whenever you move a volume.
  - Drag along the ruler to scrub, and use ▶ to preview. Clips can loop.
- **In the game:** set **Play on start** in the Rig section to the clip the character should play.

Volumes can also follow paths drawn in [Canvas mode](/editor/canvas-mode/): the character bends as they move.

## What gets saved

The rig, its volumes, rest pose, poses and clips are stored in the scene, so they're included in every export, including web and protected builds.

Models imported from GLB, FBX and the other formats keep their true rest pose, so you can rig an animated model too. Its own baked animation is replaced by the rig while the rig is on.

## Current limits

- Scripts and Diagrams can't start or switch clips yet; only **Play on start** plays one.
- Deformation runs on the CPU. On the web, a character whose pose is changing re-sends its mesh each frame, which is fine for typical game characters but heavy for very dense meshes.
- Volumes don't collide; add colliders separately.
