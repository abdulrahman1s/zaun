#!/bin/sh
# Install EasyPlay from the release tarball (PKG-007, RELEASE_STRATEGY §2).
#
#   ./install.sh            as root: /usr/local; otherwise ~/.local
#   ./install.sh --user     ~/.local
#   ./install.sh --system   /usr/local (run as root)
#   ./install.sh --prefix DIR
#
# DESTDIR is honoured for staged installs (caches are then left alone). Every file and every
# directory this script creates is recorded in PREFIX/libexec/easyplay/install-manifest, so
# PREFIX/libexec/easyplay/uninstall.sh removes exactly what was added. Installing over an
# earlier tarball install upgrades it and removes files the new version no longer ships.
#
# This script never changes MIME defaults, never touches EasyPlay's data or settings, needs
# no network and installs nothing setuid. EasyPlay needs `bwrap` (bubblewrap) and `umu-run`
# (umu-launcher) from the distribution; the Doctor screen explains anything missing.
set -eu

usage() {
    sed -n '2,15p' "$0" | sed 's/^# \{0,1\}//'
    cat <<'EOF'
Options:
  --prefix DIR        Install under DIR
  --user              Install under ~/.local
  --system            Install under /usr/local
  --no-cache-refresh  Do not refresh desktop and icon caches
  --force             Install over an EasyPlay that this script did not install
  -h, --help          This text
EOF
}

die() { printf 'install.sh: error: %s\n' "$*" >&2; exit 1; }
say() { printf '%s\n' "$*"; }

src="$(cd "$(dirname "$0")" && pwd)"
prefix=""
refresh=1
force=0

while [ $# -gt 0 ]; do
    case "$1" in
        --prefix) [ $# -ge 2 ] || die "--prefix needs a directory"; prefix="$2"; shift ;;
        --prefix=*) prefix="${1#--prefix=}" ;;
        --user) prefix="${HOME:?HOME is not set}/.local" ;;
        --system) prefix="/usr/local" ;;
        --no-cache-refresh) refresh=0 ;;
        --force) force=1 ;;
        -h | --help) usage; exit 0 ;;
        *) usage >&2; die "unknown option: $1" ;;
    esac
    shift
done

[ -f "$src/FILES" ] || die "run this script from the extracted release tarball (FILES is missing)"

if [ -z "$prefix" ]; then
    if [ "$(id -u)" -eq 0 ]; then prefix="/usr/local"; else prefix="${HOME:?HOME is not set}/.local"; fi
fi
case "$prefix" in /*) ;; *) die "the prefix must be an absolute path: $prefix" ;; esac
prefix="${prefix%/}"
case "$prefix" in
    "" | /usr | /)
        [ "$force" -eq 1 ] || die "$prefix/ belongs to the package manager; use a distribution package or --prefix /usr/local" ;;
esac

destdir="${DESTDIR:-}"
dest="$destdir$prefix"
state="$dest/libexec/easyplay"
manifest="$state/install-manifest"

if [ -e "$dest/bin/easyplay" ] || [ -L "$dest/bin/easyplay" ]; then
    if [ ! -f "$manifest" ] && [ "$force" -ne 1 ]; then
        die "$dest/bin/easyplay exists and was not installed by this script (use --force to replace it)"
    fi
fi

if command -v easyplay >/dev/null 2>&1; then
    other="$(command -v easyplay)"
    [ "$other" = "$prefix/bin/easyplay" ] || say "Note: another easyplay is on PATH at $other."
fi

tmp="$(mktemp -d "${TMPDIR:-/tmp}/easyplay-install.XXXXXX")"
trap 'rm -rf "$tmp"' EXIT INT TERM

# New manifest: directories created now, directories created by an earlier install (still
# ours to remove when empty), then files.
: >"$tmp/dirs"
if [ -f "$manifest" ]; then
    sed -n 's/^d //p' "$manifest" >>"$tmp/dirs"
fi

make_dir() { # $1: path relative to the prefix
    [ -d "$dest/$1" ] && return 0
    parent="$(dirname "$1")"
    [ "$parent" = "." ] || make_dir "$parent"
    mkdir -m 755 "$dest/$1"
    printf '%s\n' "$1" >>"$tmp/dirs"
}

mkdir -p "$dest"

# Absolute Exec for the desktop entry, so it works when PREFIX/bin is not on the session's
# PATH (often the case for ~/.local/bin).
exec_path="$prefix/bin/easyplay"
case "$exec_path" in
    *[\"\`\$\\]*) desktop_exec="easyplay" ;; # Characters that need desktop-entry escaping.
    *[[:space:]]*) desktop_exec="\"$exec_path\"" ;;
    *) desktop_exec="$exec_path" ;;
esac

while IFS= read -r rel; do
    [ -n "$rel" ] || continue
    make_dir "$(dirname "$rel")"
    from="$src/$rel"
    to="$dest/$rel"
    # Copy beside the target, then rename: replaces a running binary safely.
    if [ -L "$from" ]; then
        rm -f "$to.new"
        cp -P "$from" "$to.new"
    else
        cp "$from" "$to.new"
        chmod "$( [ -x "$from" ] && echo 755 || echo 644 )" "$to.new"
    fi
    case "$rel" in
        share/applications/*.desktop)
            sed -e "s|^Exec=easyplay |Exec=$desktop_exec |" \
                -e "s|^TryExec=easyplay\$|TryExec=$exec_path|" "$to.new" >"$tmp/desktop"
            cat "$tmp/desktop" >"$to.new" ;;
    esac
    mv -f "$to.new" "$to"
done <"$src/FILES"

make_dir libexec/easyplay
cp "$src/uninstall.sh" "$state/uninstall.sh.new"
chmod 755 "$state/uninstall.sh.new"
mv -f "$state/uninstall.sh.new" "$state/uninstall.sh"

{
    cat "$src/FILES"
    printf '%s\n' libexec/easyplay/uninstall.sh
} | LC_ALL=C sort -u >"$tmp/files"

# Upgrade: remove what the previous install had and this version no longer ships.
if [ -f "$manifest" ]; then
    sed -n 's/^f //p' "$manifest" | LC_ALL=C sort -u >"$tmp/old-files"
    LC_ALL=C comm -23 "$tmp/old-files" "$tmp/files" | while IFS= read -r rel; do
        [ -n "$rel" ] && rm -f "$dest/$rel"
    done
fi

{
    printf '# EasyPlay tarball install of %s; read by uninstall.sh. Paths are relative to %s.\n' \
        "$(cat "$src/VERSION" 2>/dev/null || echo unknown)" "$prefix"
    LC_ALL=C sort -u "$tmp/dirs" | sed 's/^/d /'
    sed 's/^/f /' "$tmp/files"
} >"$manifest.new"
mv -f "$manifest.new" "$manifest"

# Refresh caches that already exist; never create new ones (they would be stray files, and
# a stale icon cache would hide icons that other programs add later).
if [ "$refresh" -eq 1 ] && [ -z "$destdir" ]; then
    apps="$dest/share/applications"
    if [ -f "$apps/mimeinfo.cache" ] && command -v update-desktop-database >/dev/null 2>&1; then
        update-desktop-database -q "$apps" || true
    fi
    icons="$dest/share/icons/hicolor"
    touch "$icons" 2>/dev/null || true
    if [ -f "$icons/icon-theme.cache" ] && command -v gtk-update-icon-cache >/dev/null 2>&1; then
        gtk-update-icon-cache -q -t -f "$icons" || true
    fi
fi

say "EasyPlay installed under $dest."
say "Uninstall with: $prefix/libexec/easyplay/uninstall.sh"
case ":${PATH:-}:" in
    *":$prefix/bin:"*) ;;
    *) say "Note: $prefix/bin is not on PATH; the menu entry still works. Add it to PATH to use 'easyplay' in a terminal." ;;
esac
command -v bwrap >/dev/null 2>&1 || say "Note: bwrap (bubblewrap) was not found; install it with your package manager."
command -v umu-run >/dev/null 2>&1 || say "Note: umu-run (umu-launcher) was not found; EasyPlay's Doctor screen explains how to install it."
