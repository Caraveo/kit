#!/usr/bin/env bash
# Build the standalone decoder used by the desktop editor.
set -euo pipefail
version=8.1.3
sha=7138d28c96d9d3e3af4ee3d8cad72741f8ffb40da90c1112235dea3ecd3178a3
archive="ffmpeg-$version.tar.xz"
target="${1:?Pass the desktop Rust target triple}"
output="${2:-target/ffmpeg/$target}"
mkdir -p "$output"
output="$(cd "$output" && pwd -P)"
arch="${target%%-*}"
case "$arch" in x86_64|aarch64) ;; *) echo "Unsupported FFmpeg architecture: $arch" >&2; exit 1 ;; esac
exe=ffmpeg
options=()
case "$target" in
  *-linux-gnu)
    if [[ "$(uname -m)" != "$arch" ]]; then
      options+=(--enable-cross-compile --target-os=linux "--arch=$arch" "--cross-prefix=$arch-linux-gnu-")
    fi ;;
  *-apple-darwin)
    mac_arch="$arch"
    if [[ "$mac_arch" == aarch64 ]]; then mac_arch=arm64; fi
    options+=(--target-os=darwin "--arch=$arch" "--cc=clang -arch $mac_arch" --host-cc=clang)
    if [[ "$(uname -m)" != "$mac_arch" ]]; then options+=(--enable-cross-compile); fi ;;
  *-windows-*)
    exe=ffmpeg.exe
    case "${MSYSTEM:-}" in
      CLANG64|CLANGARM64) options+=(--target-os=mingw32 "--arch=$arch" --cc=clang --ar=llvm-ar --nm=llvm-nm --ranlib=llvm-ranlib --extra-ldflags=-static) ;;
      *)
        cross="$arch-w64-mingw32-"
        options+=(--enable-cross-compile --target-os=mingw32 "--arch=$arch" "--cross-prefix=$cross" --extra-ldflags=-static) ;;
    esac ;;
  *) echo "Unsupported FFmpeg target: $target" >&2; exit 1 ;;
esac

cache="$output/$archive"
if [[ ! -f "$cache" ]]; then
  if [[ -n "${KIT_FFMPEG_SOURCE:-}" ]]; then
    cp "$KIT_FFMPEG_SOURCE" "$cache.tmp"
  else
    curl -fsSL --retry 3 "https://ffmpeg.org/releases/$archive" -o "$cache.tmp"
  fi
  mv "$cache.tmp" "$cache"
fi
if command -v sha256sum >/dev/null; then
  actual="$(sha256sum "$cache" | cut -d' ' -f1)"
else
  actual="$(shasum -a 256 "$cache" | cut -d' ' -f1)"
fi
if [[ "$actual" != "$sha" ]]; then
  echo "FFmpeg source checksum mismatch. Remove $cache and retry." >&2
  exit 1
fi

# Include source and the exact recipe beside the decoder in each distribution.
if [[ -f "$output/$exe" && -f "$output/COPYING.LGPLv2.1" && -f "$output/build.sh" ]] &&
   cmp -s "$0" "$output/build.sh"; then
  exit 0
fi
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
script="$(cd "$(dirname "$0")" && pwd -P)/$(basename "$0")"
tar -xJf "$cache" -C "$work"
(
  cd "$work/ffmpeg-$version"
  ./configure "${options[@]}" \
    --disable-autodetect --disable-network --disable-doc --disable-debug \
    --disable-ffplay --disable-ffprobe --disable-x86asm \
    --disable-encoders --enable-encoder=mjpeg,pcm_s16le \
    --disable-muxers --enable-muxer=image2pipe,pcm_s16le
  make -j"${KIT_FFMPEG_JOBS:-$(getconf _NPROCESSORS_ONLN 2>/dev/null || sysctl -n hw.ncpu || echo 4)}" "$exe"
  install -m755 "$exe" "$output/$exe"
  install -m644 COPYING.LGPLv2.1 LICENSE.md "$output/"
  install -m644 "$script" "$output/build.sh"
)
