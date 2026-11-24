---
title: Post-processing volumes
description: Change how parts of your world look (color, exposure, glow, focus, ambient occlusion, anti-aliasing) with boxes, spheres and global volumes.
---

A **Post-Processing Volume** is an invisible object that changes how the picture looks. Place one at a cave entrance and the screen darkens and cools as the camera walks in. Fill a room with glow, or blur the background through a telescope.

## Adding one

1. **+ → Empty** to create an object.
2. **+ Add component → ✨ Post-Processing Volume**.
3. Choose a **Shape**:
   - **Box**: the object's transform sets its size, position and rotation (a unit cube, scaled).
   - **Sphere**: its diameter is the object's largest scale.
   - **Global**: applies everywhere. Use it for scene-wide defaults that local volumes build on.

Volumes are invisible in the game. In the editor you see their outline (solid) and blend region (dashed). Purple volumes affect the camera, amber ones affect objects. Parenting works: a volume on a child object follows its parent.

## Settings

| Setting | |
|---|---|
| **Affect** | **Camera inside**: the whole picture changes while the camera is inside or near the volume. **Objects inside**: only what's inside the volume changes, seen from anywhere (worked out per pixel from depth). |
| **Priority** | Where volumes overlap, higher priority wins. |
| **Blend** | World units outside the shape over which the effect fades in. |
| **Weight** | How strongly the volume applies at full influence (0 to 1). |

Each effect has **Inherit / On / Off**:

- **Inherit** leaves the effect to other volumes (a lower priority one, or the global default).
- **On** uses this volume's values.
- **Off** switches the effect off here, even if a global volume turned it on.

So a volume can change only Glow and keep everyone else's Color and Exposure.

## Effects

| Effect | Settings |
|---|---|
| **Color** | Saturation, contrast, temperature (cool ↔ warm). |
| **Exposure** | EV (stops; +1 doubles brightness) and tone mapping: **Filmic** (soft highlights) or **None**. |
| **Glow** | HDR bloom: threshold, intensity, radius. Blurred at reduced resolution and added before tone mapping. |
| **Focus** | Depth of field: focus distance and range (world units along the view direction), strength. Works with perspective and orthographic cameras. |
| **Ambient** | Screen-space ambient occlusion: radius (world units), strength, quality (8 / 16 / 32 samples). |
| **Image** | FXAA and sharpening. MSAA stays as it is. |

The order is fixed: ambient occlusion → depth of field → glow → exposure and tone mapping → color → FXAA and sharpening. Colors are linear HDR until tone mapping, and the result is converted to the screen's color space once. Editor gizmos and UI are drawn on top, unaffected.

## How volumes combine

For each camera, at its position:

1. Start from **neutral**: no visible change.
2. Apply **Global** volumes, then local ones, from lowest to highest priority. The highest priority applies last and wins inside overlaps. Equal priorities: the **smaller** volume wins, then the one **later in the Objects list**.
3. A volume's influence is full inside its shape and fades smoothly to nothing over its Blend distance, times its Weight.
4. Numbers blend continuously. Choices (tone mapping, AO quality, FXAA on/off) switch to the volume's setting once its influence passes **one half**.

Each camera is resolved separately: the editor camera, a selected camera's preview, and the game camera in Play mode.

**Objects inside** volumes follow the same rules per pixel, on top of the camera's settings. Up to 8 are evaluated (lowest priority dropped first), and AO quality comes from the camera's settings.

## Where you see it

- **Play mode**, exported games (desktop and web) and the **camera preview** (select a camera) always apply volumes.
- The main viewport previews them from the editor camera. Toggle with **✨ Volumes** in the bottom bar, or <kbd>6</kbd>.

## Good to know

- A scene with no active volume draws exactly as before: Kit skips post-processing entirely.
- Focus, Ambient and Objects-inside volumes read the scene's depth. Transparent things (sprites, see-through models) don't write depth, so these effects see what's behind them.
- With MSAA on, depth is multisampled and Kit reads its first sample. Very fine edges may show a 1-pixel difference in depth-based effects.
- The editor grid and backdrop are part of the 3D view, so volumes affect them in the main viewport.
- Not yet: stylized effects, auto exposure, temporal anti-aliasing, motion blur.

See `examples/volumes` for a scene with a neutral global volume, a Cave and a Glow Room that overlap, a rotated Telescope (focus) and a grey pillar (Objects inside).
