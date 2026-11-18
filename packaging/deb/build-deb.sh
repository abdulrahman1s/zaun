#!/usr/bin/env bash
# Build the Zaun .deb (PKG-004) from a tree staged by scripts/build-release.sh.
#
#   scripts/build-release.sh --prefix /usr --destdir dist/deb-stage
#   packaging/deb/build-deb.sh --stage dist/deb-stage --out dist
#
# or from the unpacked release tarball (built with the shim path /usr/libexec/zaun):
#
#   packaging/deb/build-deb.sh --from-tarball dist/zaun-<version>-x86_64-linux --out dist
#
# The binaries are not rebuilt here, so the package contains exactly what the release build
# checked (sizes, static shim, no dev-inspection). Adds the AppArmor profile (PKG-006) as a
# conffile, Debian's copyright and changelog, compressed man pages, and maintainer scripts
# that only load or unload the profile. Needs dpkg-deb; objdump (binutils) refines the libc6
# version, otherwise the glibc of the build host is assumed.
set -euo pipefail

die() { printf 'build-deb: error: %s\n' "$*" >&2; exit 1; }

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(cd "$HERE/../.." && pwd)"

stage=""
tarball=""
out="$ROOT/dist"
revision=1
maintainer="Abdulrahman <mail@abdulrahman.dev>"

while (($#)); do
    case "$1" in
        --stage) stage="${2:?}"; shift ;;
        --from-tarball) tarball="${2:?}"; shift ;;
        --out) out="${2:?}"; shift ;;
        --revision) revision="${2:?}"; shift ;;
        --maintainer) maintainer="${2:?}"; shift ;;
        -h | --help) sed -n '2,11p' "$0" | sed 's/^# \{0,1\}//'; exit 0 ;;
        *) die "unknown option: $1" ;;
    esac
    shift
done

mkdir -p "$out"
out="$(cd "$out" && pwd)"
work="$(mktemp -d "$out/.deb-work.XXXXXX")"
trap 'rm -rf "$work"' EXIT

version=""
if [[ -n "$tarball" ]]; then
    [[ -z "$stage" ]] || die "--stage and --from-tarball exclude each other"
    [[ -f "$tarball/VERSION" ]] || die "$tarball is not an unpacked release tarball"
    version="$(cat "$tarball/VERSION")"
    # FHS layout: the binary in /usr/bin, the shim at its compiled-in path.
    stage="$work/stage"
    install -Dm755 "$tarball/libexec/zaun/zaun" "$stage/usr/bin/zaun"
    install -Dm755 "$tarball/libexec/zaun/zaun-init" "$stage/usr/libexec/zaun/zaun-init"
    cp -R "$tarball/share" "$stage/usr/"
fi
[[ -n "$stage" ]] || die "--stage or --from-tarball is required"
stage="$(cd "$stage" && pwd)"
[[ -x "$stage/usr/bin/zaun" ]] || die "$stage/usr/bin/zaun is missing (stage with --prefix /usr)"
[[ -x "$stage/usr/libexec/zaun/zaun-init" ]] ||
    die "$stage/usr/libexec/zaun/zaun-init is missing (Debian uses /usr/libexec/zaun)"
command -v dpkg-deb >/dev/null || die "dpkg-deb not found"

if [[ -z "$version" ]]; then
    version="$(awk '
        $0 == "[workspace.package]" { p = 1; next }
        /^\[/ && p { exit }
        p && $1 == "version" { gsub(/"/, "", $3); print $3; exit }
    ' "$ROOT/Cargo.toml")"
fi
# Debian orders pre-releases with "~": 1.0.0-rc.1 -> 1.0.0~rc.1.
deb_version="${version/-/\~}-${revision}"

case "$(uname -m)" in
    x86_64) arch=amd64 ;;
    aarch64) arch=arm64 ;;
    *) die "unsupported architecture $(uname -m)" ;;
esac

pkg="$work/root"
mkdir -p "$pkg"

cp -a "$stage/." "$pkg/"
install -Dm644 "$ROOT/packaging/apparmor/zaun" "$pkg/etc/apparmor.d/zaun"

doc="$pkg/usr/share/doc/zaun"
install -Dm644 "$HERE/copyright" "$doc/copyright"
# Debian ships the GPL in common-licenses; keep the other licence texts and the crate list.
rm -f "$pkg/usr/share/licenses/zaun/LICENSE"
date="$(date -u -R -d "@${SOURCE_DATE_EPOCH:-$(git -C "$ROOT" log -1 --format=%ct 2>/dev/null || date +%s)}")"
{
    printf 'zaun (%s) unstable; urgency=medium\n\n' "$deb_version"
    printf '  * Release %s. See /usr/share/doc/zaun/CHANGELOG.md.gz.\n\n' "$version"
    printf ' -- %s  %s\n' "$maintainer" "$date"
} | gzip -9n >"$doc/changelog.Debian.gz"
for f in "$doc"/*.md; do [[ -e "$f" ]] && gzip -9nf "$f"; done
for f in "$pkg"/usr/share/man/man*/*.[0-9]; do [[ -e "$f" ]] && gzip -9nf "$f"; done

install -Dm644 /dev/stdin "$pkg/usr/share/lintian/overrides/zaun" <<'EOF'
# The in-sandbox init shim is static on purpose (ARCHITECTURE §12): it is bind-mounted into
# every sandbox and must not depend on the sandbox's libraries.
zaun: statically-linked-binary [usr/libexec/zaun/zaun-init]
EOF

# libc6: the newest GLIBC_ symbol version the binary references.
libc_min=""
if command -v objdump >/dev/null; then
    libc_min="$(objdump -T "$pkg/usr/bin/zaun" | grep -o 'GLIBC_[0-9.]*' |
        sed 's/GLIBC_//' | sort -uV | tail -n1)"
fi
if [[ -z "$libc_min" ]]; then
    libc_min="$(ldd --version 2>/dev/null | head -n1 | grep -o '[0-9]\+\.[0-9]\+$' || echo 2.36)"
fi
depends="libc6 (>= $libc_min)"
if command -v readelf >/dev/null && readelf -dW "$pkg/usr/bin/zaun" | grep -q 'libgcc_s'; then
    depends="$depends, libgcc-s1"
fi
# Display, input and graphics libraries are loaded with dlopen (RELEASE_STRATEGY §2.1).
depends="$depends, bubblewrap (>= 0.8.0), libxkbcommon0, libwayland-client0 | libx11-6, libvulkan1 | libgl1, libudev1"

install -d "$pkg/DEBIAN"
installed_size="$(du -sk --exclude=DEBIAN "$pkg" | cut -f1)"
cat >"$pkg/DEBIAN/control" <<EOF
Package: zaun
Version: $deb_version
Architecture: $arch
Maintainer: $maintainer
Installed-Size: $installed_size
Depends: $depends
Recommends: umu-launcher, libwayland-client0, libx11-6, libxcursor1, libxi6, libxrandr2, libxkbcommon-x11-0, xdg-desktop-portal, xdg-utils
Suggests: mangohud, gamescope
Section: games
Priority: optional
Homepage: https://github.com/zaun/zaun
Description: run Windows games in a per-game sandbox
 Zaun runs Windows games on Linux through umu-launcher and Proton. Open
 an .exe from the file manager, answer one question about what the game may
 access, and play. Every game runs inside its own bubblewrap sandbox with its
 own Windows prefix, and only sees its own folder unless allowed more.
 .
 Play time statistics stay on the computer; there is no telemetry.
 .
 umu-launcher (umu-run) is required at run time; where the distribution does
 not package it, Zaun's Doctor screen explains how to install it.
EOF
printf '/etc/apparmor.d/zaun\n' >"$pkg/DEBIAN/conffiles"
install -m755 "$HERE/postinst" "$HERE/postrm" "$pkg/DEBIAN/"
(cd "$pkg" && find . -path ./DEBIAN -prune -o -type f -print | sed 's|^\./||' | LC_ALL=C sort |
    xargs -d '\n' md5sum) >"$pkg/DEBIAN/md5sums"

find "$pkg" -type d -exec chmod 755 {} +
find "$pkg" -type f ! -perm -u+x -exec chmod 644 {} +
chmod 644 "$pkg/DEBIAN/control" "$pkg/DEBIAN/conffiles" "$pkg/DEBIAN/md5sums"

deb="$out/zaun_${deb_version}_${arch}.deb"
dpkg-deb --root-owner-group -Zxz --build "$pkg" "$deb" >/dev/null
printf 'build-deb: wrote %s\n' "$deb"
