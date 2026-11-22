#!/usr/bin/env bash
# A minimal static FFmpeg for the portable build (ADR-0041): only the two hardware H.264
# encoders Zaun uses (ADR-0036), so `zaun` carries its own libavcodec and runs whatever
# FFmpeg the system has (Fedora's 9, or none). NVENC loads the NVIDIA driver at run time;
# VA-API links the system's libva.
#
#   packaging/portable/build-ffmpeg.sh PREFIX [WORKDIR]
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=sources.sh
. "$here/sources.sh"

prefix="${1:?usage: build-ffmpeg.sh PREFIX [WORKDIR]}"
work="${2:-$(mktemp -d)}"
mkdir -p "$work" "$prefix"

if [[ -f "$prefix/.ffmpeg-$FFMPEG_VERSION-$NVCODEC_VERSION" ]]; then
    echo "build-ffmpeg: $prefix is up to date"
    exit 0
fi

fetch "$NVCODEC_URL" "$work/nv-codec-headers.tar.gz" "$NVCODEC_SHA256"
fetch "$FFMPEG_URL" "$work/ffmpeg.tar.xz" "$FFMPEG_SHA256"

rm -rf "$work/nv" "$work/ffmpeg"
mkdir -p "$work/nv" "$work/ffmpeg"
tar -xzf "$work/nv-codec-headers.tar.gz" -C "$work/nv" --strip-components=1
tar -xJf "$work/ffmpeg.tar.xz" -C "$work/ffmpeg" --strip-components=1

make -C "$work/nv" PREFIX="$prefix" install >/dev/null
export PKG_CONFIG_PATH="$prefix/lib/pkgconfig${PKG_CONFIG_PATH:+:$PKG_CONFIG_PATH}"

cd "$work/ffmpeg"
# Every library is built (rusty_ffmpeg probes all of them), but no component except the two
# encoders: the unused libraries are a few kilobytes and the linker drops them.
./configure \
    --prefix="$prefix" \
    --pkg-config-flags=--static \
    --enable-static --disable-shared --enable-pic \
    --disable-programs --disable-doc --disable-network --disable-autodetect \
    --disable-everything --disable-x86asm \
    --disable-xlib --disable-libxcb \
    --enable-ffnvcodec --enable-nvenc --enable-vaapi \
    --enable-encoder=h264_nvenc,h264_vaapi \
    >"$work/ffmpeg-configure.log" 2>&1 || { tail -40 "$work/ffmpeg-configure.log" ffbuild/config.log; exit 1; }
# Compiler warnings from FFmpeg's own code are not ours to fix; the log keeps them.
make -j"$(nproc)" >"$work/ffmpeg-make.log" 2>&1 || { tail -60 "$work/ffmpeg-make.log"; exit 1; }
make install >/dev/null
touch "$prefix/.ffmpeg-$FFMPEG_VERSION-$NVCODEC_VERSION"
echo "build-ffmpeg: FFmpeg $FFMPEG_VERSION in $prefix"
