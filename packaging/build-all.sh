#!/usr/bin/env bash
# Build every Kit package this computer can make, into dist/.
#
#   Linux:  Linux x86_64 (.tar.gz + .deb) and Windows x86_64 (.zip, needs mingw-w64)
#   macOS:  Mac Intel and Apple Silicon (.dmg each; needs Xcode command line tools
#           and a universal MoltenVK, e.g. MOLTENVK=~/VulkanSDK/<v>/macOS/lib/libMoltenVK.dylib)
#
# Every package also carries the game players built here (dist/players/<os>/),
# so its editor can export for those systems too. To add another system's
# players (e.g. the Mac ones on Linux), copy its dist/players/<os> folder into
# dist/players before running this.
#
# usage: packaging/build-all.sh
set -euo pipefail
cd "$(dirname "$0")/.."
VERSION="$(grep -m1 '^version' Cargo.toml | cut -d'"' -f2)"
mkdir -p dist/players
step() { printf '\n\033[1m== %s\033[0m\n' "$*"; }
target() { command -v rustup >/dev/null && rustup target add "$1" >/dev/null 2>&1 || true; }

step "Web player (WebAssembly)"
target wasm32-unknown-unknown
cargo build --release --locked --target wasm32-unknown-unknown -p kit_web

case "$(uname -s)" in
Linux)
  step "Linux x86_64"
  cargo build --release --locked -p kit-editor -p kit-player
  install -Dm755 target/release/kit-player dist/players/linux/kit-player

  WINDOWS=0
  if command -v x86_64-w64-mingw32-gcc >/dev/null; then
    step "Windows x86_64 (mingw-w64)"
    target x86_64-pc-windows-gnu
    cargo build --release --locked --target x86_64-pc-windows-gnu -p kit-editor -p kit-player
    install -Dm755 target/x86_64-pc-windows-gnu/release/kit-player.exe dist/players/windows/kit-player.exe
    WINDOWS=1
  else
    echo "skipping Windows: install mingw-w64 (sudo apt install mingw-w64)"
  fi

  step "Packages"
  packaging/linux/package.sh
  [ "$WINDOWS" = 1 ] && packaging/windows/package.sh x86_64-pc-windows-gnu
  ;;
Darwin)
  : "${MOLTENVK:=$(brew --prefix molten-vk 2>/dev/null)/lib/libMoltenVK.dylib}"
  export MOLTENVK
  [ -f "$MOLTENVK" ] || { echo "MoltenVK not found: install the LunarG Vulkan SDK and set MOLTENVK=…/libMoltenVK.dylib" >&2; exit 1; }
  ARCHS="$(lipo -archs "$MOLTENVK")"
  echo "MoltenVK: $MOLTENVK ($ARCHS)"
  BUILT=()
  for T in x86_64-apple-darwin aarch64-apple-darwin; do
    A="${T%%-*}"; [ "$A" = aarch64 ] && A=arm64
    if [[ " $ARCHS " != *" $A "* ]]; then
      echo "skipping $T: this MoltenVK has no $A (use the LunarG Vulkan SDK's universal one)"
      continue
    fi
    step "Mac $A"
    target "$T"
    cargo build --release --locked --target "$T" -p kit-editor -p kit-player
    BUILT+=("$T")
  done
  [ ${#BUILT[@]} -gt 0 ] || { echo "nothing built" >&2; exit 1; }
  # One player that runs on both Intel and Apple Silicon Macs.
  mkdir -p dist/players/mac
  lipo -create $(for T in "${BUILT[@]}"; do echo "target/$T/release/kit-player"; done) -output dist/players/mac/kit-player
  # Mac games exported from Linux or Windows need MoltenVK next to the player.
  cp "$MOLTENVK" dist/players/mac/libMoltenVK.dylib
  step "Packages"
  for T in "${BUILT[@]}"; do packaging/macos/package.sh "$T"; done
  ;;
*)
  echo "build-all.sh runs on Linux or macOS (on Windows, use packaging/windows/package.ps1)" >&2
  exit 1
  ;;
esac

step "Done: Kit $VERSION"
ls -lh dist | grep -v "^total" | grep -v " players$" || true
