#!/usr/bin/env bash
# Build and sign the update manifests of a release (ADR-0040 §2, ADR-0041 §3,
# RELEASE_STRATEGY §4.1).
#
#   ZAUN_RELEASE_KEY=/path/to/key.pem scripts/sign-release-manifest.sh --version 0.2.0 --dir dist
#
# Reads, in DIR, the gzip-compressed files of the bundle:
#   zaun-VERSION-ARCH-linux.{zaun,zaun-init,zaun-overlay,zaun-chat}.gz
#   zaun-simplex-SIMPLEXVERSION-ARCH.tar.gz   (the Chat engine folder, a GNU tar of plain files)
# and writes DIR/bundle.json (format 2, read by Zaun from 0.3 on) and DIR/manifest.json
# (format 1: zaun and zaun-init only, for copies from before, which update a tarball in
# place), each with a .sig: the base64 Ed25519 signature over the file's exact bytes, which
# the app verifies with the public key compiled into it.
#
# The private key (PEM, Ed25519) comes from the file named by ZAUN_RELEASE_KEY, or from the
# PEM text in ZAUN_RELEASE_KEY_PEM (CI: the ZAUN_UPDATE_SIGNING_KEY secret). It is never
# written to DIR and never printed. The repository URL is read from the one constant in
# crates/zaun-core/src/consts.rs.
set -euo pipefail

usage() { sed -n '2,19p' "$0" | sed 's/^# \{0,1\}//'; }
die() { printf 'sign-release-manifest: error: %s\n' "$*" >&2; exit 1; }

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
version=""
dir=""
arch="$(uname -m)"
date="$(date -u +%Y-%m-%d)"
notes=""

while (($#)); do
    case "$1" in
        --version) version="${2:?}"; shift ;;
        --dir) dir="${2:?}"; shift ;;
        --arch) arch="${2:?}"; shift ;;
        --date) date="${2:?}"; shift ;;
        --notes) notes="${2?}"; shift ;;
        -h | --help) usage; exit 0 ;;
        *) usage >&2; die "unknown option: $1" ;;
    esac
    shift
done

[[ -n "$version" && -n "$dir" ]] || { usage >&2; die "--version and --dir are required"; }
version="${version#v}"
[[ "$version" =~ ^[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z.-]+)?$ ]] || die "not a version: $version"
command -v openssl >/dev/null || die "openssl is needed"
command -v python3 >/dev/null || die "python3 is needed"

repo="$(sed -n '/macro_rules! repo_url/,/^}/s/^ *"\(https:[^"]*\)".*/\1/p' "$ROOT/crates/zaun-core/src/consts.rs")"
[[ "$repo" == https://github.com/* ]] || die "cannot read the repository URL from consts.rs"

tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT
umask 077
if [[ -n "${ZAUN_RELEASE_KEY_PEM:-}" ]]; then
    printf '%s\n' "$ZAUN_RELEASE_KEY_PEM" >"$tmp/key.pem"
elif [[ -n "${ZAUN_RELEASE_KEY:-}" ]]; then
    [[ -r "$ZAUN_RELEASE_KEY" ]] || die "ZAUN_RELEASE_KEY is not a readable file"
    cp "$ZAUN_RELEASE_KEY" "$tmp/key.pem"
else
    die "set ZAUN_RELEASE_KEY (a key file) or ZAUN_RELEASE_KEY_PEM (the key's text)"
fi
openssl pkey -in "$tmp/key.pem" -noout 2>/dev/null || die "the key is not a PEM private key"

# name|path|file|size|sha256|unpacked size|unpacked sha256
describe() {
    local name="$1" path="$2" file="$3"
    [[ -f "$dir/$file" ]] || die "missing $dir/$file"
    gzip -t "$dir/$file" || die "$file is not gzip"
    printf '%s|%s|%s|%s|%s|%s|%s\n' "$name" "$path" "$file" \
        "$(stat -c %s "$dir/$file")" "$(sha256sum "$dir/$file" | cut -d' ' -f1)" \
        "$(gzip -dc "$dir/$file" | wc -c)" "$(gzip -dc "$dir/$file" | sha256sum | cut -d' ' -f1)"
}

files=()
for name in zaun zaun-init zaun-overlay zaun-chat; do
    files+=("$(describe "$name" "$name" "zaun-${version}-${arch}-linux.${name}.gz")")
done
shopt -s nullglob
engines=("$dir"/zaun-simplex-*-"$arch".tar.gz)
shopt -u nullglob
((${#engines[@]} == 1)) || die "expected one zaun-simplex-*-$arch.tar.gz in $dir, found ${#engines[@]}"
files+=("$(describe simplex simplex/ "$(basename "${engines[0]}")")")

# write FORMAT OUT: the manifest of that format from the described files.
write() {
    ZAUN_REPO="$repo" ZAUN_VERSION="$version" ZAUN_DATE="$date" ZAUN_NOTES="$notes" \
        ZAUN_ARCH="$arch" ZAUN_FORMAT="$1" python3 -I - "${files[@]}" >"$2" <<'PY'
import json, os, sys
repo, version, arch = os.environ["ZAUN_REPO"], os.environ["ZAUN_VERSION"], os.environ["ZAUN_ARCH"]
fmt = int(os.environ["ZAUN_FORMAT"])
artifacts = []
for e in sys.argv[1:]:
    name, path, file, size, sha, usize, usha = e.split("|")
    if fmt == 1 and name not in ("zaun", "zaun-init"):
        continue
    artifacts.append({
        "channel": "bundle" if fmt == 2 else "tarball", "arch": arch,
        "path": path if fmt == 2 else f"libexec/zaun/{name}",
        "url": f"{repo}/releases/download/v{version}/{file}",
        "size": int(size), "sha256": sha, "unpacked_size": int(usize), "unpacked_sha256": usha,
    })
if fmt == 1:
    # Copies from before replace the shim first, then the binary.
    artifacts.sort(key=lambda a: a["path"] != "libexec/zaun/zaun-init")
manifest = {
    "format": fmt, "product": "zaun", "version": version, "date": os.environ["ZAUN_DATE"],
    "notes": os.environ["ZAUN_NOTES"][:280],
    "notes_url": f"{repo}/releases/tag/v{version}",
    "artifacts": artifacts,
}
sys.stdout.write(json.dumps(manifest, indent=2) + "\n")
PY
}

# sign FILE: FILE.sig, verified before it is written.
sign() {
    openssl pkeyutl -sign -inkey "$tmp/key.pem" -rawin -in "$1" -out "$tmp/sig"
    [[ "$(stat -c %s "$tmp/sig")" == 64 ]] || die "the key is not an Ed25519 key"
    openssl pkeyutl -verify -pubin -inkey "$tmp/pub.pem" -rawin -in "$1" \
        -sigfile "$tmp/sig" >/dev/null || die "the signature of $1 does not verify"
    base64 -w0 "$tmp/sig" >"$1.sig"
    printf '\n' >>"$1.sig"
    chmod 644 "$1" "$1.sig"
}

openssl pkey -in "$tmp/key.pem" -pubout -out "$tmp/pub.pem"
write 2 "$dir/bundle.json"
write 1 "$dir/manifest.json"
sign "$dir/bundle.json"
sign "$dir/manifest.json"

pub="$(openssl pkey -in "$tmp/key.pem" -pubout -outform DER | tail -c 32 | od -An -tx1 | tr -d ' \n')"
printf 'sign-release-manifest: signed %s/bundle.json and manifest.json (Zaun %s) with the key whose public half is %s\n' \
    "$dir" "$version" "$pub" >&2
