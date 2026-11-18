#!/bin/sh
# Remove an Zaun tarball install (PKG-007).
#
#   PREFIX/libexec/zaun/uninstall.sh       the installed copy knows its prefix
#   ./uninstall.sh [--user|--system|--prefix DIR]
#
# Removes exactly the files and directories listed in PREFIX/libexec/zaun/install-manifest
# (written by install.sh); directories are removed only when empty. Games, settings, the
# library and downloaded runtimes in your home directory are kept: Zaun's data lives in
# ~/.local/share/zaun, ~/.config/zaun, ~/.local/state/zaun and ~/.cache/zaun.
# DESTDIR is honoured.
set -eu

die() { printf 'uninstall.sh: error: %s\n' "$*" >&2; exit 1; }

self_dir="$(cd "$(dirname "$0")" && pwd)"
prefix=""
refresh=1

while [ $# -gt 0 ]; do
    case "$1" in
        --prefix) [ $# -ge 2 ] || die "--prefix needs a directory"; prefix="$2"; shift ;;
        --prefix=*) prefix="${1#--prefix=}" ;;
        --user) prefix="${HOME:?HOME is not set}/.local" ;;
        --system) prefix="/usr/local" ;;
        --no-cache-refresh) refresh=0 ;;
        -h | --help) sed -n '2,12p' "$0" | sed 's/^# \{0,1\}//'; exit 0 ;;
        *) die "unknown option: $1" ;;
    esac
    shift
done

destdir="${DESTDIR:-}"
if [ -z "$prefix" ]; then
    case "$self_dir" in
        */libexec/zaun)
            if [ -f "$self_dir/install-manifest" ]; then
                prefix="${self_dir%/libexec/zaun}"
                # Running the copy inside a DESTDIR tree: the prefix is below DESTDIR.
                [ -n "$destdir" ] && prefix="${prefix#"$destdir"}"
            fi ;;
    esac
fi
if [ -z "$prefix" ]; then
    if [ "$(id -u)" -eq 0 ]; then prefix="/usr/local"; else prefix="${HOME:?HOME is not set}/.local"; fi
fi
case "$prefix" in /*) ;; *) die "the prefix must be an absolute path: $prefix" ;; esac
prefix="${prefix%/}"

dest="$destdir$prefix"
manifest="$dest/libexec/zaun/install-manifest"
[ -f "$manifest" ] || die "no Zaun tarball install found under $dest ($manifest is missing)"

dirs="$(mktemp "${TMPDIR:-/tmp}/zaun-uninstall.XXXXXX")"
trap 'rm -f "$dirs"' EXIT INT TERM
# Deepest first: sort by the number of path components, descending.
sed -n 's/^d //p' "$manifest" | awk -F/ '{ print NF "\t" $0 }' | sort -rn | cut -f2- >"$dirs"

# Files and symbolic links first.
sed -n 's/^f //p' "$manifest" | while IFS= read -r rel; do
    case "$rel" in "" | /* | ../* | */../* | *..) continue ;; esac
    if [ -f "$dest/$rel" ] || [ -L "$dest/$rel" ]; then rm -f "$dest/$rel"; fi
done
rm -f "$manifest"

# Then the directories install.sh created, only when empty.
while IFS= read -r rel; do
    case "$rel" in "" | /* | ../* | */../* | *..) continue ;; esac
    rmdir "$dest/$rel" 2>/dev/null || true
done <"$dirs"

if [ "$refresh" -eq 1 ] && [ -z "$destdir" ]; then
    apps="$dest/share/applications"
    if [ -f "$apps/mimeinfo.cache" ] && command -v update-desktop-database >/dev/null 2>&1; then
        update-desktop-database -q "$apps" || true
    fi
    icons="$dest/share/icons/hicolor"
    if [ -d "$icons" ]; then
        touch "$icons" 2>/dev/null || true
        if [ -f "$icons/icon-theme.cache" ] && command -v gtk-update-icon-cache >/dev/null 2>&1; then
            gtk-update-icon-cache -q -t -f "$icons" || true
        fi
    fi
fi

printf 'Zaun removed from %s.\n' "$dest"
printf 'Your games, settings and runtimes were kept (~/.local/share/zaun, ~/.config/zaun).\n'
