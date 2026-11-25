#!/usr/bin/env bash
# Package Kit for Windows as a .zip from Linux or macOS (after cross-compiling
# with mingw-w64). On Windows itself, use package.ps1.
# usage: packaging/windows/package.sh [target-triple]   (default x86_64-pc-windows-gnu)
set -euo pipefail
TARGET="${1:-x86_64-pc-windows-gnu}"
VERSION="$(grep -m1 '^version' Cargo.toml | cut -d'"' -f2)"
BIN_DIR="target/$TARGET/release"
case "$TARGET" in aarch64*) ARCH=arm64 ;; *) ARCH=x86_64 ;; esac
NAME="kit-${VERSION}-windows-${ARCH}"
FFMPEG_TARGET="$TARGET"
FFMPEG_DIR="target/ffmpeg/$FFMPEG_TARGET"
packaging/ffmpeg/build.sh "$FFMPEG_TARGET" "$FFMPEG_DIR"

DIST=dist
STAGE="$DIST/$NAME"
rm -rf "$STAGE" "$DIST/$NAME.zip"
mkdir -p "$STAGE/examples" "$STAGE/players/web"
cp "$BIN_DIR/kit-editor.exe" "$BIN_DIR/kit-player.exe" "$STAGE/"
mkdir -p "$STAGE/ffmpeg"
cp -R "$FFMPEG_DIR/." "$STAGE/ffmpeg/"
cp target/wasm32-unknown-unknown/release/kit_web.wasm "$STAGE/players/web/"
# Players for other systems built alongside (dist/players/<os>/).
[ -d "$DIST/players" ] && cp -r "$DIST/players/." "$STAGE/players/"
cp -r examples/hello "$STAGE/examples/"
rm -rf "$STAGE"/examples/*/export   # games exported from the example
cp README.md "$STAGE/"
cp LICENSE BRANDING.md "$STAGE/"
(cd "$DIST" && zip -qr "$NAME.zip" "$NAME")
rm -rf "$STAGE"
echo "wrote $DIST/$NAME.zip"
