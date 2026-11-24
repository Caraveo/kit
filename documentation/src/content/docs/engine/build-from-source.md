---
title: Build from source
description: Compile Kit, run tests, and package releases.
---

You need [Rust](https://rustup.rs) (stable) and Vulkan development files.

```sh
# Linux
sudo apt install libvulkan-dev libxkbcommon-dev libwayland-dev

git clone https://github.com/Caraveo/kit
cd kit
cargo run -p kit-editor --release             # the editor
cargo run -p kit-player -- examples/hello     # the player
cargo test --workspace                        # all tests
```

Set `KIT_VALIDATION=1` to turn on the Vulkan validation layers (install `vulkan-validationlayers` on Linux).

## Packaging

| Platform | Command (after `cargo build --release`) |
|---|---|
| Linux | `packaging/linux/package.sh` → `.tar.gz` + `.deb` |
| macOS | `packaging/macos/package.sh` → `Kit.app` in a `.dmg` (needs `brew install molten-vk`) |
| Windows | `pwsh packaging/windows/package.ps1` → `.zip` |

Pushing a tag like `v0.0.2` runs `.github/workflows/release.yml`, which builds all six platform packages and publishes a GitHub release.

## This documentation

```sh
cd documentation
npm install
npm run dev      # http://localhost:4321
npm run build    # static site in documentation/dist
```
