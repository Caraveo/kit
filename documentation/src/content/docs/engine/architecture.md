---
title: Architecture
description: Kit's modules and how a frame runs.
---

Kit is **modular**: each subsystem is its own Rust crate and plugs into the app as a **module**. The layout follows [Defold](https://github.com/defold/defold).

| Module | Crate | Role |
|---|---|---|
| ecs | `kit_ecs` | Entities, sparse-set component storage, resources, queries |
| core | `kit_core` | `App`, the `Module` trait, stages, play/edit modes, time, console |
| scene | `kit_scene` | Transforms, parent/child hierarchy, transform propagation |
| input | `kit_input` | Keyboard and mouse state |
| resource | `kit_resource` | Project files, paths, images |
| gamesys | `kit_gamesys` | Component types and `.kscene` files |
| graphics | `kit_graphics` | Vulkan 1.3: device, swapchain, memory, pipelines, WGSL → SPIR-V |
| render | `kit_render` | Draws models, sprites, grid and backdrop; picking |
| x | `kit_x` | The x language: lexer, parser, interpreter |
| script | `kit_script` | Runs x scripts on entities; engine modules for scripts |
| path | `kit_path` | Follows Canvas-mode paths |
| collider | `kit_collider` | Collision shapes, events, ray casts (Rapier) |
| physics | `kit_physics` | Rigid-body dynamics on top of collider |
| engine | `kit_engine` | The windowed runtime and default module set |
| kit | `kit` | One crate re-exporting everything |

## A frame

Systems run in fixed **stages**:

1. **First**: engine bookkeeping.
2. **PreUpdate**
3. **Update**: x scripts, then path following, then collision/physics (fixed 60 Hz steps).
4. **PostUpdate**: transform propagation.
5. **Last**: input edges reset.

Then the renderer draws the active camera's view. Systems marked *play-only* (scripts, paths, physics) are skipped in the editor's edit mode.

## Modules

```rust
pub trait Module {
    fn name(&self) -> &'static str;
    fn dependencies(&self) -> &'static [&'static str] { &[] }
    fn build(&self, app: &mut App);
}
```

`build` registers resources and systems. Modules are added in dependency order; `App::add_module` checks the order and refuses missing dependencies.
