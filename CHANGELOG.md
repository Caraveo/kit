# Changelog

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
- **kit_script**: x scripts on entities with `Init`/`Update`/`Final`, script hot reload, and the `Transform`, `Input`, `Time`, `Entity`, `Scene`, `Sprite`, `Model` and `Log` modules.
- **kit_collider**: collision shapes (box, sphere, capsule, cylinder), contact and trigger events, ray casts. Built on Rapier.
- **kit_physics**: rigid bodies (dynamic, kinematic, static), gravity, mass, damping, forces and impulses, 2D plane lock, CCD.
- **kit_path**: follows paths sketched in Canvas mode, best effort, respecting colliders and physics.
- **Terrain** (in kit_gamesys/kit_render/kit_collider): height grids with generation, sculpting, slope/height coloring and heightfield colliders.
- **kit_engine**: the windowed runtime.

### Tools
- **kit-editor**: terrain sculpting, camera/light gizmos with a live camera preview, glide-to-frame on hierarchy select, Canvas mode for sketching movement paths, collider outlines, Collider/Rigid Body/Path inspectors, hierarchy with drag-and-drop reparenting, inspector, Vulkan viewport (orbit, pan, zoom, 2D/3D, wireframe, lighting, grid), click to select, undo/redo, play mode with live script fields, console, x code editor with syntax highlighting, asset list, new/open project, and an export command that writes a runnable game folder.
- **kit-player**: runs a project or an exported game.
- An example project, **Hello Kit**.

### Platforms
Linux (x86_64, aarch64: `.tar.gz`, `.deb`), Windows (x86_64, arm64: `.zip`), macOS (Apple Silicon, Intel: `.dmg` with MoltenVK bundled).
