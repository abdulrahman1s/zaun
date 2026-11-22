# Pinned third-party sources of the portable build (ADR-0041). Sourced by the scripts here.
# shellcheck shell=bash disable=SC2034

FFMPEG_VERSION=8.1.3
FFMPEG_URL="https://ffmpeg.org/releases/ffmpeg-${FFMPEG_VERSION}.tar.xz"
FFMPEG_SHA256=7138d28c96d9d3e3af4ee3d8cad72741f8ffb40da90c1112235dea3ecd3178a3

# SDK 12.1: NVENC works with NVIDIA drivers from 530 on (13.x would need 570).
NVCODEC_VERSION=n12.1.14.0
NVCODEC_URL="https://github.com/FFmpeg/nv-codec-headers/archive/refs/tags/${NVCODEC_VERSION}.tar.gz"
NVCODEC_SHA256=2fefaa227d2a3b4170797796425a59d1dd2ed5fd231db9b4244468ba327acd0b

# The Chat engine (ADR-0033): the official desktop AppImage, the same file nixpkgs pins for
# simplex-chat-desktop.
SIMPLEX_VERSION=7.0.3
SIMPLEX_URL="https://github.com/simplex-chat/simplex-chat/releases/download/v${SIMPLEX_VERSION}/simplex-desktop-x86_64.AppImage"
SIMPLEX_SHA256=404220f16f1c03879b8d7402e31e8ce525dc3ce7a6904637bd48e19974f2536a
SIMPLEX_SOURCE="https://github.com/simplex-chat/simplex-chat/tree/v${SIMPLEX_VERSION}"

# Download $1 to $2 unless it is there with sha256 $3; fail on a mismatch.
fetch() {
    local url="$1" out="$2" sum="$3"
    if [[ ! -f "$out" ]] || ! echo "$sum  $out" | sha256sum -c --quiet - 2>/dev/null; then
        curl -sSfL --retry 3 -o "$out.part" "$url"
        mv "$out.part" "$out"
    fi
    echo "$sum  $out" | sha256sum -c --quiet - || { echo "fetch: $out does not match $sum" >&2; exit 1; }
}
