# Changelog

## v0.0.4: Rex and Rigg

(Release notes in progress.)

## v0.0.3 — Kit logo and editor polish

- Updated the Kit app icon on Ubuntu, macOS, and Windows. The new artwork also appears in the editor header and dock, the welcome screen, and web export loading screens.
- Editor views have clean headers without close buttons.
- Website download buttons open the latest Kit release.

![Kit v0.0.3 editor showing the updated logo and view headers](https://raw.githubusercontent.com/caraveo/kit/v0.0.3/docs/releases/v0.0.3.png)

## v0.0.2: Diagrams and Effects

### Major features
- **Diagrams (visual logic):**
  - Blueprint-style graphs that run directly on the engine, with no code generated.
  - Press **7** for the Diagram view: add nodes from a searchable picker, drag wires between typed pins, select a wire and press Del to remove it, set values inline, and undo or redo.
  - **Nodes:**
    - Events: On Start, On Interact, On Trigger Enter/Exit, On Timer, custom events.
    - Objects: position, rotation, scale, Move Over Time, visibility.
    - Appearance and physics: tint, gravity scale, scene gravity, impulse, velocity.
    - Data and logic: datasets, constants, variables, Compare, Branch, Sequence, Delay, math, logic, conversions.
    - Integration: sounds, events, script fields.
  - **Checks:** each problem is flagged on its node and pin before running.
  - **Runtime:** deterministic frame order, a per-frame node budget against runaway loops, and live highlighting and pin values while playing.
  - Runs in the editor, the desktop player and the web player (`kit_diagram`).
- **Game Data:** datasets with typed fields and rows, plus constants, saved in `game.kdata` and referenced by stable IDs (`kit_gamedata`).
- **Effects (post-processing volumes):**
  - Box or sphere volumes with bloom, color grading, exposure, fog, depth of field, vignette, sharpening and more.
  - Each volume affects the camera inside it or the objects inside it. Overlapping volumes blend by priority and soft edges.
  - Identical on Vulkan and WebGPU, with a Volumes preview (**6**) and the `examples/volumes` project (`kit_post`).
- **Protected exports:**
  - Standard, Signed, or Signed + Encrypted.
  - Uses Ed25519 signatures and XChaCha20-Poly1305 encryption, and a pinned publisher key in each player.
  - A changed or re-signed game refuses to start (`kit_export_security`).

### x scripting
- Fields are private by default. `PUBLIC` shows a field in the inspector (read-only) and `EDIT` makes it editable per object; edited values are saved in the scene and applied before `Init`.
- Diagrams read `PUBLIC` fields and set `EDIT` fields.

### Editor
- **Shortcuts:**
  - <kbd>1</kbd>–<kbd>7</kbd> toggle the viewport bar: wireframe, lighting, grid, colliders, canvas, volumes, diagram.
  - <kbd>W</kbd>/<kbd>S</kbd> zoom, and <kbd>A</kbd>/<kbd>D</kbd> pan (<kbd>Q</kbd>/<kbd>E</kbd> down/up, <kbd>Shift</kbd> faster).
- **Import files…** and drag-and-drop for every kind of file Kit uses: sounds, sprites, 3D models, scripts, scenes and diagrams. Sprites and sounds dropped on the viewport become objects.
- **Export:**
  - Exports go to `~/KitBuilds/<project>/<platform>` by default.
  - Exports are written atomically.
  - A stale player is detected and rebuilt.
- **Inspector and console:**
  - The console starts collapsed, with a badge for new errors.
  - The inspector scroll bar appears after a short delay and no longer covers buttons.
- New app icon.

### Bug fixes
- The asset list no longer shows copies from exported games inside the project (Scenes showed 6 instead of 2).
- Long model names no longer overflow the inspector and object list.
- Fixed hierarchy drag-to-reparent, which could swallow other drags.
- Light mode now uses a light viewport background and no longer shows a stray dark line between panels.
- Exports no longer include the export folder itself.
- Mac exports made on Linux now include MoltenVK.


## v0.0.1 — first release

The first public build of Kit: a modular 2D/3D game engine in Rust with a
Vulkan renderer, an editor, and the **x** scripting language.

### Engine modules
- **kit_ecs**: generational entities, sparse-set component storage, resources, tuple queries.
- **kit_core**: `App`, the `Module` trait, ordered stages, play/edit run modes, time, console.
- **kit_scene**: scene graph with transforms, parent/child hierarchy and transform propagation.
- **kit_input**: keyboard and mouse state with pressed/released edges and axes.
- **kit_resource**: `kit.project` files, resource paths, text and image loading.
- **kit_gamesys**: Camera (perspective or 2D orthographic), Model (built-in meshes), Sprite, Light and Script components, plus `.kscene` scene files.
- **kit_graphics**: Vulkan 1.3 (dynamic rendering), swapchain, GPU memory, and WGSL shaders compiled at runtime.
- **kit_render**: lit 3D models, alpha-blended 2D sprites, 4× MSAA, textures with hot reload, the editor grid and gradient backdrop, and picking.
- **kit_x**: the x language, with a lexer, parser and interpreter. It supports classes, `RIDE` inheritance, methods, strict types, `GRAB`/`LOOP`/`IF`, multiple returns, `TAKE` for modules and other files, and a built-in `Math` module.
- **kit_script**: x scripts on entities with `Init`/`Update`/`Final` and collision hooks, script hot reload, and the `Transform`, `Input`, `Time`, `Entity`, `Scene`, `Sprite`, `Model`, `Physics`, `Path`, `Terrain`, `Audio` and `Log` modules.
- **kit_audio**: Audio Source components and script sounds (WAV, MP3, Ogg Vorbis, FLAC, AAC). 3D sounds fade with distance (Natural, Linear or Smooth falloff) and pan left/right. On Linux it uses the system's sound with no extra packages.
- **kit_collider**: collision shapes (box, sphere, capsule, cylinder), contact and trigger events, ray casts. Built on Rapier.
- **kit_physics**: rigid bodies (dynamic, kinematic, static), gravity, mass, damping, forces and impulses, 2D plane lock, CCD.
- **kit_path**: follows paths sketched in Canvas mode, best effort, respecting colliders and physics.
- **Terrain** (in kit_gamesys/kit_render/kit_collider): height grids with generation, sculpting, slope/height coloring and heightfield colliders.
- **kit_engine**: the windowed runtime.
- **Imported 3D models**: OBJ, glTF/GLB, FBX, USDZ and STL (converted with Blender), with materials, textures and animation clips.

### Tools
- **kit-editor**: terrain sculpting, camera/light gizmos with a live camera preview, glide-to-frame on hierarchy select, Canvas mode for sketching movement paths, collider outlines, Collider/Rigid Body/Path inspectors, hierarchy with drag-and-drop reparenting, inspector, Vulkan viewport (orbit, pan, zoom, 2D/3D, wireframe, lighting, grid), click to select, undo/redo, play mode with live script fields, console, x code editor with syntax highlighting, asset list, new/open project, and an export command that writes a runnable game folder.
- **kit-editor**, newer features: move arrows and rotation rings, sound drag-and-drop (Assets → Sounds), 3D model import, light and dark themes, and the **Export wizard**.
- **Export wizard and Hooks**: each platform is a hook (`hooks/<platform>`). Kit exports Linux, Windows, Mac and **Web** builds. The web build runs in the browser on WebGPU, with the same engine, shaders and mixer as desktop. Licensed platforms (Switch, PlayStation, Xbox, Steam) show **Request access**, and installed hooks can add exporters.
- **kit-player**: runs a project or an exported game. Every Kit package includes the Linux, Windows, Mac and web players, so any editor can export for any of them.
- An example project, **Hello Kit**.

### Downloads
| System | File |
|---|---|
| Mac (Apple Silicon) | `kit-0.0.1-macos-arm64.dmg` |
| Mac (Intel) | `kit-0.0.1-macos-x86_64.dmg` |
| Windows 10/11 (x64) | `kit-0.0.1-windows-x86_64.zip` |
| Ubuntu / Debian (x64) | `kit_0.0.1_amd64.deb` (`sudo apt install ./kit_0.0.1_amd64.deb`) |
| Linux (x64, any distribution) | `kit-0.0.1-linux-x86_64.tar.gz` |

Kit needs a GPU with Vulkan 1.3 (on Macs, MoltenVK is included). The Mac apps aren't notarized, so open them the first time with right-click → **Open**.
