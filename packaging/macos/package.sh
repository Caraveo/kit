#!/usr/bin/env bash
# Package Kit for macOS: Kit.app (with MoltenVK bundled) inside a .dmg.
# usage: packaging/macos/package.sh [target-triple]   (run from the repo root after `cargo build --release`)
# Needs MoltenVK: `brew install molten-vk` (or set MOLTENVK=/path/to/libMoltenVK.dylib).
set -euo pipefail

TARGET="${1:-}"
VERSION="$(grep -m1 '^version' Cargo.toml | cut -d'"' -f2)"
BIN_DIR="target/${TARGET:+$TARGET/}release"
case "${TARGET:-$(uname -m)}" in
  *aarch64*|arm64) ARCH=arm64 ;;
  *) ARCH=x86_64 ;;
esac
MOLTENVK="${MOLTENVK:-$(brew --prefix molten-vk 2>/dev/null)/lib/libMoltenVK.dylib}"
[ -f "$MOLTENVK" ] || { echo "MoltenVK not found at $MOLTENVK" >&2; exit 1; }

DIST=dist
APP="$DIST/Kit.app"
rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources/examples" "$APP/Contents/Frameworks"
install -m755 "$BIN_DIR/kit-editor" "$BIN_DIR/kit-player" "$APP/Contents/MacOS/"
cp "$MOLTENVK" "$APP/Contents/Frameworks/libMoltenVK.dylib"
cp -r examples/hello "$APP/Contents/Resources/examples/"
rm -rf "$APP"/Contents/Resources/examples/*/export   # games exported from the example
mkdir -p "$APP/Contents/Resources/players/web"
cp target/wasm32-unknown-unknown/release/kit_web.wasm "$APP/Contents/Resources/players/web/"
# Players for other systems built alongside (dist/players/<os>/).
[ -d "$DIST/players" ] && cp -R "$DIST/players/." "$APP/Contents/Resources/players/"
cp packaging/icons/kit.icns "$APP/Contents/Resources/"
sed "s/__VERSION__/$VERSION/g" packaging/macos/Info.plist > "$APP/Contents/Info.plist"
# Ad-hoc signature so the bundle runs on Apple Silicon (not notarized).
codesign --force --deep --sign - "$APP"

DMG="$DIST/kit-${VERSION}-macos-${ARCH}.dmg"
rm -f "$DMG"
STAGE="$DIST/dmg"
rm -rf "$STAGE" && mkdir -p "$STAGE"
cp -R "$APP" "$STAGE/"
ln -s /Applications "$STAGE/Applications"
hdiutil create -volname "Kit $VERSION" -srcfolder "$STAGE" -ov -format UDZO "$DMG" >/dev/null
rm -rf "$STAGE"
echo "wrote $DMG"
