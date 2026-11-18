#!/usr/bin/env bash
# Build and sign the update manifest of a release (ADR-0040 §2, RELEASE_STRATEGY §4.1).
#
#   ZAUN_RELEASE_KEY=/path/to/key.pem scripts/sign-release-manifest.sh --version 0.2.0 --dir dist
#
# Reads, in DIR, the gzip-compressed files of the tarball channel:
#   zaun-VERSION-ARCH-linux.zaun.gz        (libexec/zaun/zaun)
#   zaun-VERSION-ARCH-linux.zaun-init.gz   (libexec/zaun/zaun-init)
# and writes DIR/manifest.json and DIR/manifest.json.sig: the base64 Ed25519 signature over
# the manifest's exact bytes, which the app verifies with the public key compiled into it.
#
# The private key (PEM, Ed25519) comes from the file named by ZAUN_RELEASE_KEY, or from the
# PEM text in ZAUN_RELEASE_KEY_PEM (CI: the ZAUN_UPDATE_SIGNING_KEY secret). It is never
# written to DIR and never printed. The repository URL is read from the one constant in
# crates/zaun-core/src/consts.rs.
set -euo pipefail

usage() { sed -n '2,15p' "$0" | sed 's/^# \{0,1\}//'; }
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

entries=()
for name in zaun zaun-init; do
    file="zaun-${version}-${arch}-linux.${name}.gz"
    [[ -f "$dir/$file" ]] || die "missing $dir/$file"
    gzip -t "$dir/$file" || die "$file is not gzip"
    size="$(stat -c %s "$dir/$file")"
    sha="$(sha256sum "$dir/$file" | cut -d' ' -f1)"
    usize="$(gzip -dc "$dir/$file" | wc -c)"
    usha="$(gzip -dc "$dir/$file" | sha256sum | cut -d' ' -f1)"
    entries+=("$name|$file|$size|$sha|$usize|$usha")
done

ZAUN_REPO="$repo" ZAUN_VERSION="$version" ZAUN_DATE="$date" ZAUN_NOTES="$notes" ZAUN_ARCH="$arch" \
    python3 -I - "${entries[@]}" >"$dir/manifest.json" <<'EOF'
import json, os, sys
repo, version, arch = os.environ["ZAUN_REPO"], os.environ["ZAUN_VERSION"], os.environ["ZAUN_ARCH"]
artifacts = []
for e in sys.argv[1:]:
    name, file, size, sha, usize, usha = e.split("|")
    artifacts.append({
        "channel": "tarball", "arch": arch, "path": f"libexec/zaun/{name}",
        "url": f"{repo}/releases/download/v{version}/{file}",
        "size": int(size), "sha256": sha, "unpacked_size": int(usize), "unpacked_sha256": usha,
    })
manifest = {
    "format": 1, "product": "zaun", "version": version, "date": os.environ["ZAUN_DATE"],
    "notes": os.environ["ZAUN_NOTES"][:280],
    "notes_url": f"{repo}/releases/tag/v{version}",
    "artifacts": artifacts,
}
sys.stdout.write(json.dumps(manifest, indent=2) + "\n")
EOF

openssl pkeyutl -sign -inkey "$tmp/key.pem" -rawin -in "$dir/manifest.json" -out "$tmp/sig"
[[ "$(stat -c %s "$tmp/sig")" == 64 ]] || die "the key is not an Ed25519 key"
openssl pkey -in "$tmp/key.pem" -pubout -out "$tmp/pub.pem"
openssl pkeyutl -verify -pubin -inkey "$tmp/pub.pem" -rawin -in "$dir/manifest.json" \
    -sigfile "$tmp/sig" >/dev/null || die "the signature does not verify"
base64 -w0 "$tmp/sig" >"$dir/manifest.json.sig"
printf '\n' >>"$dir/manifest.json.sig"
chmod 644 "$dir/manifest.json" "$dir/manifest.json.sig"

pub="$(openssl pkey -in "$tmp/key.pem" -pubout -outform DER | tail -c 32 | od -An -tx1 | tr -d ' \n')"
printf 'sign-release-manifest: signed %s/manifest.json (Zaun %s) with the key whose public half is %s\n' \
    "$dir" "$version" "$pub" >&2
