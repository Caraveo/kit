#!/usr/bin/env bash
# Package Kit for Linux: a portable .tar.gz and a .deb.
# usage: packaging/linux/package.sh [target-triple]   (run from the repo root after `cargo build --release`)
set -euo pipefail

TARGET="${1:-}"
VERSION="$(grep -m1 '^version' Cargo.toml | cut -d'"' -f2)"
BIN_DIR="target/${TARGET:+$TARGET/}release"
case "${TARGET:-$(uname -m)}" in
  *aarch64*|arm64) ARCH=arm64; TAR_ARCH=aarch64 ;;
  *) ARCH=amd64; TAR_ARCH=x86_64 ;;
esac
DIST=dist
mkdir -p "$DIST"

# --- portable tarball -------------------------------------------------------
NAME="kit-${VERSION}-linux-${TAR_ARCH}"
STAGE="$DIST/$NAME"
rm -rf "$STAGE"
mkdir -p "$STAGE/examples"
install -m755 "$BIN_DIR/kit-editor" "$BIN_DIR/kit-player" "$STAGE/"
WEB_PLAYER=target/wasm32-unknown-unknown/release/kit_web.wasm
install -Dm644 "$WEB_PLAYER" "$STAGE/players/web/kit_web.wasm"
# Players for other systems built alongside (dist/players/<os>/).
[ -d "$DIST/players" ] && cp -r "$DIST/players/." "$STAGE/players/"
cp -r examples/hello "$STAGE/examples/"
rm -rf "$STAGE"/examples/*/export   # games exported from the example
install -m644 packaging/icons/kit-256.png "$STAGE/kit.png"
install -m644 packaging/linux/io.github.caraveo.Kit.desktop README.md "$STAGE/"
install -m644 LICENSE BRANDING.md "$STAGE/"
tar -C "$DIST" -czf "$DIST/$NAME.tar.gz" "$NAME"
rm -rf "$STAGE"
echo "wrote $DIST/$NAME.tar.gz"

# --- .deb ---------------------------------------------------------------------
PKG="$DIST/deb"
rm -rf "$PKG"
mkdir -p "$PKG/DEBIAN" "$PKG/usr/bin" "$PKG/usr/share/kit/examples" "$PKG/usr/share/applications" "$PKG/usr/share/doc/kit"
install -m755 "$BIN_DIR/kit-editor" "$BIN_DIR/kit-player" "$PKG/usr/bin/"
install -Dm644 "$WEB_PLAYER" "$PKG/usr/share/kit/players/web/kit_web.wasm"
[ -d "$DIST/players" ] && cp -r "$DIST/players/." "$PKG/usr/share/kit/players/"
cp -r examples/hello "$PKG/usr/share/kit/examples/"
rm -rf "$PKG"/usr/share/kit/examples/*/export
install -m644 packaging/linux/io.github.caraveo.Kit.desktop "$PKG/usr/share/applications/"
for n in 16 32 48 64 128 256 512; do
  d="$PKG/usr/share/icons/hicolor/${n}x${n}/apps"
  mkdir -p "$d"
  install -m644 "packaging/icons/kit-$n.png" "$d/io.github.caraveo.Kit.png"
done
install -m644 README.md BRANDING.md "$PKG/usr/share/doc/kit/"
install -m644 LICENSE "$PKG/usr/share/doc/kit/copyright"
SIZE_KB="$(du -sk "$PKG/usr" | cut -f1)"
cat > "$PKG/DEBIAN/control" <<CONTROL
Package: kit
Version: ${VERSION}
Architecture: ${ARCH}
Maintainer: Caraveo <noreply@github.com>
Installed-Size: ${SIZE_KB}
Depends: libvulkan1, libxkbcommon0, libc6
Recommends: mesa-vulkan-drivers | nvidia-vulkan-icd, xdg-desktop-portal
Section: devel
Priority: optional
Homepage: https://github.com/Caraveo/kit
Description: Kit modular game engine and editor
 Kit is a modular 2D/3D game engine written in Rust with a Vulkan renderer,
 an ECS with a scene graph, the x scripting language, a scene editor
 (kit-editor) and a player (kit-player).
CONTROL
dpkg-deb --root-owner-group --build "$PKG" "$DIST/kit_${VERSION}_${ARCH}.deb" >/dev/null
rm -rf "$PKG"
echo "wrote $DIST/kit_${VERSION}_${ARCH}.deb"
