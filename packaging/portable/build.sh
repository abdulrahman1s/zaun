#!/usr/bin/env bash
# The portable build (ADR-0041): binaries that start on any x86_64 distribution with glibc
# 2.36 or newer, and on NixOS through nix-ld. Run it in a Debian 12 container, which has
# the oldest glibc Zaun supports; CI's release workflow does, and locally:
#
#   podman run --rm -v "$PWD:/src" -v zaun-portable:/work -w /src docker.io/library/debian:12 \
#       packaging/portable/build.sh
#
# FFmpeg is linked in statically (build-ffmpeg.sh); everything else Zaun links is a library
# every desktop has (libstdc++, libudev, libpipewire, libpulse, libopus, libva), and the GPU
# stack is the system's own, loaded at run time. The result is scripts/build-release.sh's
# tarball tree in dist/, with the Chat engine in libexec/zaun/simplex.
#
# Debian 12's desktop-file-validate and appstreamcli are too old for the desktop entry
# (Version=1.5), so they are not installed here: build-release.sh then skips that check, which
# CI's test job and the packages' own checks make with current tools.
#
# Environment: ZAUN_PORTABLE_WORK (default /work, else a folder in /var/tmp) keeps the
# toolchain, the sources, FFmpeg and cargo's target folder between runs.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
work="${ZAUN_PORTABLE_WORK:-$([[ -d /work && -w /work ]] && echo /work || echo /var/tmp/zaun-portable)}"
mkdir -p "$work"

# The container is new each time; the packages are not kept in $work.
if [[ "$(id -u)" -eq 0 ]] && command -v apt-get >/dev/null && ! command -v unsquashfs >/dev/null; then
    export DEBIAN_FRONTEND=noninteractive
    apt-get update -qq
    apt-get install -y -qq --no-install-recommends \
        ca-certificates curl git xz-utils zstd squashfs-tools python3 file binutils patchelf \
        build-essential pkg-config clang libclang-dev cmake perl musl-tools \
        libudev-dev libxkbcommon-dev libwayland-dev libgl-dev libva-dev \
        libpipewire-0.3-dev libspa-0.2-dev libopus-dev libpulse-dev >/dev/null
fi

export CARGO_HOME="${CARGO_HOME:-$work/cargo}" RUSTUP_HOME="${RUSTUP_HOME:-$work/rustup}"
export PATH="$CARGO_HOME/bin:$PATH"
if ! command -v rustup >/dev/null; then
    curl -sSf https://sh.rustup.rs | sh -s -- -y -q --no-modify-path --profile minimal --default-toolchain none
fi
# rust-toolchain.toml names the version and the musl target of zaun-init.
(cd "$ROOT" && rustup show active-toolchain >/dev/null 2>&1 || rustup toolchain install)

"$ROOT/packaging/portable/build-ffmpeg.sh" "$work/ffmpeg" "$work/src"
"$ROOT/packaging/portable/simplex.sh" "$work/simplex" "$work/src"

# Notices of what is built in or shipped beside Zaun, for share/licenses/zaun.
mkdir -p "$work/licenses"
cp "$work/ffmpeg/share/licenses/LGPL-2.1-FFmpeg.txt" "$work/licenses/"
{
    printf 'GNU MP (libgmp.so.10, from Debian 12), shipped with the Chat engine in\n'
    printf 'libexec/zaun/simplex, under the GNU Lesser General Public License 3 (or the GNU GPL 2).\n'
    printf 'Source: https://sources.debian.org/src/gmp/\n\n'
    cat /usr/share/common-licenses/LGPL-3
} >"$work/licenses/LGPL-3.0-GMP.txt"

# Its own headers, whatever FFMPEG_INCLUDE_DIR the caller has (CI sets the system's for
# its Ubuntu jobs).
export FFMPEG_PKG_CONFIG_PATH="$work/ffmpeg/lib/pkgconfig" FFMPEG_LINK_MODE=static
export FFMPEG_INCLUDE_DIR="$work/ffmpeg/include"
llvm="$(clang -dumpversion | cut -d. -f1)"
export LIBCLANG_PATH="/usr/lib/llvm-$llvm/lib"
export BINDGEN_EXTRA_CLANG_ARGS="-I$(clang -print-resource-dir)/include"
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$work/target}"
# A checkout mounted from the host belongs to another user.
git config --global --add safe.directory "$ROOT" 2>/dev/null || true

"$ROOT/scripts/build-release.sh" --tarball --simplex "$work/simplex" --licenses "$work/licenses" "$@"
