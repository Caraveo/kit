---
title: Export your game
description: Use the Export wizard to make a copy of your game anyone can run, without the editor.
---

Click **Export** in the header to open the Export wizard.

1. **Pick a platform.** Each card says whether Kit can build it right now:
   - **Ready**: Kit has a matching game player for that platform.
   - **Builds player**: you're running Kit from source and the player is out of date, so Kit rebuilds it first.
   - **Needs player**: Kit needs the game player for that platform (see below).
   - **Coming later / Needs SDK / Guide**: not something Kit can build yet. Click the card to read why.
2. **Choose the output folder.** By default it's `export/<platform>/` inside your project. Kit never copies this folder into the game.
3. Click **Export for …**. When it's done, use **Open folder**, or **Run game** (**Play in browser** for HTML5).

Kit saves the scene before exporting, and replaces the previous export's `game/` folder, so files you've deleted don't linger.

## What you get

```
My_Game/                 (Windows and Linux)
├─ My_Game               ← the game (My_Game.exe on Windows)
├─ game/                 ← a copy of your project: scenes, scripts, models, textures…
└─ (runtime libraries)

My_Game.app/             (macOS)
└─ Contents/
   ├─ MacOS/My_Game
   ├─ Resources/game/
   └─ Frameworks/        ← MoltenVK
```

```
My_Game/                 (HTML5)
├─ index.html            ← the page
├─ kit.js                ← the WebGPU renderer
├─ kit_web.wasm          ← the game engine, as WebAssembly
├─ game.pack             ← your project in one file
└─ README.txt
```

Zip it and share it. Players run **My_Game**: it opens a window at the size set in [Project settings](/projects/project-settings/) and loads the main scene.

## Game players and other platforms

An exported game is Kit's **kit-player** program for the target platform, renamed, plus your project. The player must come from the **same Kit version** as the editor, or the game won't match what you see in the editor. Kit checks this by reading a build ID stamped into both programs, and won't export with a mismatched player.

- **This computer's platform** uses the `kit-player` next to the editor.
- **Other desktop platforms** need that platform's player in `players/<platform>/` next to the editor, for example `players/windows/kit-player.exe` or `players/macos/kit-player`. Take it from the Kit release for that platform, or export from Kit running on that system.
- **Steam** builds are ordinary desktop builds. Export for Windows, macOS or Linux, then upload the folder with Steamworks (SteamPipe).
- **HTML5** uses Kit's web player (`players/web/kit_web.wasm`, included with Kit). See [Web builds](#web-builds).
- **Android and iOS** are planned. They need touch input and app packaging.
- **Xbox, PlayStation and Nintendo Switch** require the platform holder's licensed SDK and developer program, so Kit can't ship them.

## Web builds

The HTML5 export runs your whole game in the browser: scenes, x scripts, physics, paths, terrain and imported models all work as they do on desktop. The engine runs as WebAssembly, and the page draws each frame with **WebGPU**, using the same shaders as the desktop renderer.

- **Players need a browser with WebGPU**: a recent Chrome, Edge, Firefox or Safari. Other browsers see a message saying so.
- **Browsers only run it from a web server**, not by double-clicking `index.html`. In Kit, **Play in browser** starts a small local server for you. To test it some other way, run `python3 -m http.server` in the folder.
- **Publishing:** upload the folder to any static host (GitHub Pages, Netlify, your own site), or zip it and upload it to **itch.io** as an HTML game.
- **Controls:** keyboard, mouse buttons, mouse movement and the wheel reach your scripts exactly as on desktop. Right-click doesn't open the browser menu, and arrow keys and Space don't scroll the page.
- **Size:** the download is your project plus about 3.5 MB of engine. Large imported models make it bigger.

If you run Kit from source, the first web export builds the web player with cargo. That needs Rust's WebAssembly target, which you install once with `rustup target add wasm32-unknown-unknown`.

## Exporting from the command line

```sh
kit-editor path/to/my-game --export path/to/output [--platform linux|windows|macos|html5]
```

## Running a project without exporting

```sh
kit-player path/to/my-game
```
