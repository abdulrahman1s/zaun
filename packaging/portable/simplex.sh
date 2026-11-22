#!/usr/bin/env bash
# The Chat engine for the portable build (ADR-0033, ADR-0041): libsimplex.so and the Haskell
# libraries beside it, taken from the official desktop AppImage (pinned in sources.sh) as
# package.nix does, plus the system's libgmp so the folder needs only glibc and zlib. Every
# library finds the others through its `$ORIGIN` run path; the other run path entries,
# folders of SimpleX's build machine (/home/builder/.cabal/…), are removed, so nothing is
# ever loaded from a folder of that name on the user's computer.
#
#   packaging/portable/simplex.sh OUTDIR [WORKDIR]
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=sources.sh
. "$here/sources.sh"

out="${1:?usage: simplex.sh OUTDIR [WORKDIR]}"
work="${2:-$(mktemp -d)}"
mkdir -p "$work"
appimage="$work/simplex-desktop-$SIMPLEX_VERSION.AppImage"
fetch "$SIMPLEX_URL" "$appimage" "$SIMPLEX_SHA256"

# The squashfs image starts where the AppImage's runtime (an ELF file) ends.
offset="$(python3 - "$appimage" <<'PY'
import struct, sys
head = open(sys.argv[1], "rb").read(64)
shoff, = struct.unpack_from("<Q", head, 0x28)
shentsize, shnum = struct.unpack_from("<HH", head, 0x3A)
print(shoff + shentsize * shnum)
PY
)"
rm -rf "$work/squashfs" "$out"
unsquashfs -q -n -o "$offset" -d "$work/squashfs" "$appimage" usr/lib/app/resources >/dev/null
res="$work/squashfs/usr/lib/app/resources"

mkdir -p "$out"
# Not vlc/ (the desktop app's media player) and not libapp-lib.so (its JNI glue).
cp -L "$res"/libsimplex.so "$res"/libHS*.so "$res"/libffi.so.8 "$res"/libcrypto.so.3 "$out/"
gmp="$(ldconfig -p | awk '$1 == "libgmp.so.10" && /x86-64/ { print $NF; exit }')"
[[ -n "$gmp" ]] || { echo "simplex: libgmp.so.10 not found" >&2; exit 1; }
cp -L "$gmp" "$out/libgmp.so.10"
for f in "$out"/*.so*; do
    if [[ -n "$(patchelf --print-rpath "$f")" ]]; then
        # shellcheck disable=SC2016 # the loader expands $ORIGIN, not the shell
        patchelf --set-rpath '$ORIGIN' "$f"
    fi
done
chmod 0644 "$out"/*.so*
printf 'SimpleX Chat %s (%s), from %s\n' "$SIMPLEX_VERSION" "$SIMPLEX_SOURCE" "$SIMPLEX_URL" >"$out/SOURCE"
rm -rf "$work/squashfs"
echo "simplex: $(ls "$out" | wc -l) files in $out"
