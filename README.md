[![Kit — 2D / 3D game engine. A violet path winds across a floating game level.](docs/kit-banner.webp)](https://github.com/caraveo/kit)

# Kit

**Draw a path. Press play. See where it goes.**

Kit is a 2D and 3D game engine for making playable prototypes and building on them. Sketch movement directly in the viewport, wire up gameplay with **Diagrams**, set the mood with post-processing **Effects**, bring your own sprites, sounds and 3D models, and press **F5** to try the result in the same window. When you're ready to share, export a desktop build or a browser build.

[Download Kit](https://github.com/caraveo/kit/releases/latest) · [Make your first game](documentation/src/content/docs/getting-started/first-game.mdx) · [Browse the source](https://github.com/caraveo/kit)

![Kit’s native editor with a scene hierarchy, 3D viewport, and inspector](docs/editor.png)

## New in v0.0.2

- **Diagrams: visual logic, no code.** Press **7** to open the Diagram view under the viewport. Wire events like *On Start*, *On Interact*, *On Trigger* and *On Timer* to actions: move, tint, show or hide objects, change gravity, play sounds, run timers and branches. Diagrams run directly in the engine (in the editor, the desktop player and the browser) and light up live while you play. [Read more](documentation/src/content/docs/editor/diagrams.md)
- **Game Data.** Datasets, rows and constants that diagrams read by stable IDs, so renaming never breaks a wire.
- **Effects: post-processing volumes.** Add a box or sphere volume for bloom, color grading, fog, depth of field, vignette, sharpening and more. Each volume affects either the camera inside it or the objects inside it, and overlapping volumes blend. [Read more](documentation/src/content/docs/editor/post-processing.md)
- **Protected exports.** Sign your game, or sign and encrypt it, so players only run the build you published.
- **x scripting:** fields are private by default. `PUBLIC` shows a field in the inspector, and `EDIT` makes it editable per object. Diagrams can read `PUBLIC` fields and set `EDIT` fields.
- **Import anything Kit uses** (sounds, sprites, 3D models, scripts, scenes, diagrams) with **Assets → + → Import files…** or by dragging files onto Kit.
- **Editor:**
  - <kbd>1</kbd>–<kbd>7</kbd> toggle the viewport bar.
  - <kbd>W</kbd>/<kbd>S</kbd> zoom and <kbd>A</kbd>/<kbd>D</kbd> pan.
  - The console starts collapsed and shows a badge when there are new errors.
  - New app icon.
  - Plus many bug fixes. See the [changelog](CHANGELOG.md).

## What you can build with Kit

- **Make movement by drawing it.** Use [Canvas mode](documentation/src/content/docs/editor/canvas-mode.mdx) to sketch a patrol, moving platform, or obstacle course in the viewport. Hit play to see how the path interacts with colliders and rigid bodies.
- **Build scenes with your own art.** Work with sprites, lighting, cameras, and sculptable terrain. Import [GLB, glTF, FBX, OBJ, USDZ, and STL](documentation/src/content/docs/editor/scenes-and-assets.md#using-3d-models), with material colors, base-color textures, and baked animation clips when available. Imports other than plain OBJ use Blender 4.5+.
- **Give objects behavior, with or without code.** Wire events to actions in a [Diagram](documentation/src/content/docs/editor/diagrams.md), or write gameplay in Kit's [x scripting language](docs/x.md) in the built-in code editor. Both can work together. Add Rapier-powered physics, triggers, and [positional sound](documentation/src/content/docs/editor/sound.md) from the inspector.
- **Set the look with Effects.** Drop [post-processing volumes](documentation/src/content/docs/editor/post-processing.md) into a scene for bloom, color grading, fog, depth of field and more, on the camera or on just the objects inside.
- **Share a playable build.** [Export your game](documentation/src/content/docs/editor/export.md) for Linux, Windows, or macOS with the matching player, or export an HTML5 build for browsers with WebGPU. You can sign or encrypt exports.

## Try it in a minute

1. Get the package for your platform from [GitHub Releases](https://github.com/caraveo/kit/releases/latest), then open **Kit** and choose **Open example**.
2. Select an object in the scene and press **5** to enter Canvas mode. Drag in the viewport to sketch a path.
3. Press **7** to open the Diagram view, click **+ Add Diagram**, then right-click the canvas to add nodes and drag wires between them.
4. Press **F5** to play. Click the viewport to pass keyboard input to the game.
5. To bring in your own files, choose **Assets → + → Import files…**, or drag sounds, sprites and models onto Kit.

| Platform | Packages built by the release workflow |
| --- | --- |
| Linux x86_64 and ARM64 | `.deb` or portable `.tar.gz` |
| Windows x86_64 and ARM64 | `.zip`. Run `kit-editor.exe`. |
| macOS Apple Silicon and Intel | `.dmg`. Drag **Kit** into Applications. |

The desktop editor requires Vulkan 1.3. The macOS package bundles MoltenVK. See the [installation guide](documentation/src/content/docs/getting-started/install.mdx) for platform-specific steps.

## Script a player in x

Attach a script to an object, then use engine modules such as `Transform` and `Input` to make it respond to the player.

```x
TAKE Transform, Input

CREATE Player[
    EDIT FLOAT Speed: 3.0      # editable in the inspector
    PUBLIC INTEGER Coins: 0    # visible, read-only

    VOID CREATE Update(FLOAT dt)[
        FLOAT X: Input.Axis("Left", "Right")
        Transform.Move(X * Speed * dt, 0, 0)
    ]
]
```

[Learn x](docs/x.md) · [Browse scripting recipes](documentation/src/content/docs/scripting/recipes.md)

## Build from source

Kit is written in Rust. The desktop renderer uses Vulkan, browser builds use WebGPU, and the engine organizes scenes with an ECS and scene graph. The native editor and standalone player share the engine. Rust applications can also use Kit through the [Rust API](documentation/src/content/docs/engine/rust-api.md).

```sh
git clone https://github.com/caraveo/kit
cd kit
# On Debian or Ubuntu, install libvulkan-dev, libxkbcommon-dev and libwayland-dev.
cargo build --workspace --locked
cargo run --locked -p kit-editor -- examples/hello
cargo run --locked -p kit-player -- examples/hello
cargo test --workspace --locked
```

[Architecture](documentation/src/content/docs/engine/architecture.md) · [Build and packaging guide](documentation/src/content/docs/engine/build-from-source.md) · [Changelog](CHANGELOG.md)

Building something with Kit? [Open an issue](https://github.com/caraveo/kit/issues) to share what you're making, report a rough edge, or tell us what would help you finish it.

## License

Kit’s engine, editor, and runtime are free under the [MIT License](LICENSE).
You can use Kit to make and sell games without engine royalties. When you
distribute a game with the Kit runtime, include the Kit license notice.

The [Kit brand assets](BRANDING.md) are reserved. Separately sold educational
products come with their own terms.
