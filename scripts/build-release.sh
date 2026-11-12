#!/usr/bin/env bash
# Release build of EasyPlay (PKG-001, RELEASE_STRATEGY §2.1).
#
# Builds `easyplay` (glibc, dynamic) and `easyplay-init` (static), checks them, and stages
# the install tree of DESKTOP_INTEGRATION §10. Every package format starts from this script:
#
#   scripts/build-release.sh --prefix /usr --destdir pkgroot        # distribution packages
#   scripts/build-release.sh --tarball                              # easyplay-<v>-x86_64-linux.tar.zst
#
# Phases can be split for packagers whose build and install steps run separately
# (makepkg, rpmbuild): `--build-only` in build(), `--stage-only` in package(). Both calls
# must pass the same --prefix/--libexec-dir, because the shim location is compiled in.
#
# Guarantees:
# - `cargo build --locked` (plus `--offline` with --offline, for vendored builds).
# - Refuses the `dev-inspection` feature (it would ship egui's inspection UI).
# - Embedded build metadata is the version and the commit only; source paths are remapped
#   and the tarball has fixed owners, order and mtimes (SOURCE_DATE_EPOCH = commit time).
# - Binary sizes within PERFORMANCE §1: easyplay <= 35 MB, easyplay-init <= 1 MB.
# - easyplay-init is static (no program interpreter, no DT_NEEDED).
set -euo pipefail

usage() {
    cat <<'EOF'
Usage: scripts/build-release.sh [options]

Layout
  --prefix DIR         Install prefix (default /usr; /usr/local with --tarball)
  --libexec-dir DIR    Directory of easyplay-init, compiled in as EASYPLAY_LIBEXEC_DIR
                       (default PREFIX/libexec/easyplay, /usr/libexec/easyplay with
                       --tarball; Arch uses /usr/lib/easyplay)
  --destdir DIR        Stage into DIR/PREFIX (default dist/stage)
  --tarball            Build the relocatable tarball in dist/ instead of a staged tree

Build
  --target TRIPLE      Target of easyplay (default <arch>-unknown-linux-gnu)
  --init-target TRIPLE Target of easyplay-init (default <arch>-unknown-linux-musl; a *-gnu
                       triple is linked statically with +crt-static and needs glibc's static libs)
  --features LIST      Extra cargo features of easyplay (dev-inspection is refused)
  --no-default-features
  --offline            Build without network (vendored sources or a pre-fetched registry)
  --jobs N             Parallel cargo jobs

Phases
  --build-only         Build and check, do not stage
  --stage-only         Stage already-built binaries (same options as the build call)
  -h, --help           This text

Environment: CARGO_TARGET_DIR is honoured; EASYPLAY_COMMIT overrides the embedded commit.
EOF
}

die() { printf 'build-release: error: %s\n' "$*" >&2; exit 1; }
note() { printf 'build-release: %s\n' "$*" >&2; }

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
ARCH="$(uname -m)"

prefix=""
libexec_dir=""
destdir=""
tarball=0
target="${ARCH}-unknown-linux-gnu"
init_target="${ARCH}-unknown-linux-musl"
features=""
no_default_features=0
offline=0
jobs=""
do_build=1
do_stage=1

while (($#)); do
    case "$1" in
        --prefix) prefix="${2:?}"; shift ;;
        --libexec-dir) libexec_dir="${2:?}"; shift ;;
        --destdir) destdir="${2:?}"; shift ;;
        --tarball) tarball=1 ;;
        --target) target="${2:?}"; shift ;;
        --init-target) init_target="${2:?}"; shift ;;
        --features) features="${2?}"; shift ;;
        --all-features) die "--all-features would enable dev-inspection; name the features instead" ;;
        --no-default-features) no_default_features=1 ;;
        --offline) offline=1 ;;
        --jobs) jobs="${2:?}"; shift ;;
        --build-only) do_stage=0 ;;
        --stage-only) do_build=0 ;;
        -h | --help) usage; exit 0 ;;
        *) usage >&2; die "unknown option: $1" ;;
    esac
    shift
done

((do_build || do_stage)) || die "--build-only and --stage-only exclude each other"

# ---- Configuration --------------------------------------------------------------------

if ((tarball)); then
    [[ -z "$destdir" ]] || die "--destdir and --tarball exclude each other"
    prefix="${prefix:-/usr/local}"
else
    prefix="${prefix:-/usr}"
fi
[[ "$prefix" == /* ]] || die "--prefix must be absolute: $prefix"
prefix="${prefix%/}"
if ((tarball)); then
    # One release binary for the tarball, the .deb and the prebuilt .rpm: they install the
    # shim at /usr/libexec/easyplay. Tarball installs (/usr/local, ~/.local) find it next to
    # the binary instead, through bin/easyplay -> ../libexec/easyplay/easyplay.
    libexec_dir="${libexec_dir:-/usr/libexec/easyplay}"
else
    libexec_dir="${libexec_dir:-$prefix/libexec/easyplay}"
fi
[[ "$libexec_dir" == /* ]] || die "--libexec-dir must be absolute: $libexec_dir"

case ",${features// /,}," in
    *,dev-inspection,* | *,easyplay/dev-inspection,* | *,easyplay-ui/dev-inspection,*)
        die "the dev-inspection feature must never be in a release build" ;;
esac

version="$(awk '
    $0 == "[workspace.package]" { p = 1; next }
    /^\[/ && p { exit }
    p && $1 == "version" { gsub(/"/, "", $3); print $3; exit }
' "$ROOT/Cargo.toml")"
[[ -n "$version" ]] || die "cannot read workspace.package.version from Cargo.toml"

# Only this checkout's own .git: a source archive unpacked inside another repository (makepkg
# in a packaging checkout) must not pick up that repository's commit.
have_git=0
[[ -e "$ROOT/.git" ]] && command -v git >/dev/null && have_git=1
if [[ -z "${EASYPLAY_COMMIT:-}" ]] && ((have_git)); then
    EASYPLAY_COMMIT="$(git -C "$ROOT" rev-parse --short=12 HEAD)"
    git -C "$ROOT" diff --quiet HEAD -- 2>/dev/null || EASYPLAY_COMMIT="${EASYPLAY_COMMIT}-dirty"
fi
export EASYPLAY_COMMIT="${EASYPLAY_COMMIT:-unknown}"
export EASYPLAY_LIBEXEC_DIR="$libexec_dir"

if [[ -z "${SOURCE_DATE_EPOCH:-}" ]]; then
    SOURCE_DATE_EPOCH=0
    ((have_git)) && SOURCE_DATE_EPOCH="$(git -C "$ROOT" log -1 --format=%ct)"
fi
export SOURCE_DATE_EPOCH

target_dir="${CARGO_TARGET_DIR:-$ROOT/target}"
easyplay_bin="$target_dir/$target/release/easyplay"
init_bin="$target_dir/$init_target/init-release/easyplay-init"

# Budgets of PERFORMANCE §1 (decimal megabytes).
max_easyplay=$((35 * 1000 * 1000))
max_init=$((1000 * 1000))

cargo_common=(--locked)
((offline)) && cargo_common+=(--offline)
[[ -n "$jobs" ]] && cargo_common+=(-j "$jobs")

feature_args=()
((no_default_features)) && feature_args+=(--no-default-features)
[[ -n "$features" ]] && feature_args+=(--features "$features")

# ---- Build ------------------------------------------------------------------------------

build() {
    command -v cargo >/dev/null || die "cargo not found"
    cd "$ROOT"

    # The feature graph, not just the command line: refuse dev-inspection however it would
    # be enabled (a default feature, another crate's feature).
    local tree
    tree="$(cargo tree "${cargo_common[@]}" -p easyplay --target "$target" -e features \
        "${feature_args[@]}")" || die "cargo tree failed; cannot check the feature set"
    if grep -q 'feature "dev-inspection"' <<<"$tree"; then
        die "the resolved feature set contains dev-inspection"
    fi

    # Reproducibility: no absolute build paths in panic messages. CARGO_ENCODED_RUSTFLAGS
    # keeps paths with spaces intact; the packager's RUSTFLAGS (makepkg, rpmbuild) are kept.
    local cargo_home="${CARGO_HOME:-$HOME/.cargo}"
    local -a base=() remap=("--remap-path-prefix=$ROOT=/build/easyplay" "--remap-path-prefix=$cargo_home=/cargo")
    read -ra base <<<"${RUSTFLAGS:-}"

    # The distributed binary: whole-program LTO, one codegen unit and no symbol table, on
    # top of [profile.release] (which stays fast for local release builds). Needed for the
    # size budget; panic stays `unwind` (the supervisor relies on it).
    export CARGO_PROFILE_RELEASE_LTO="${CARGO_PROFILE_RELEASE_LTO:-fat}"
    export CARGO_PROFILE_RELEASE_CODEGEN_UNITS="${CARGO_PROFILE_RELEASE_CODEGEN_UNITS:-1}"
    export CARGO_PROFILE_RELEASE_STRIP="${CARGO_PROFILE_RELEASE_STRIP:-symbols}"

    note "easyplay $version ($EASYPLAY_COMMIT) for $target, shim in $libexec_dir"
    CARGO_ENCODED_RUSTFLAGS="$(join_flags "${base[@]}" "${remap[@]}")" \
        cargo build "${cargo_common[@]}" --release -p easyplay --target "$target" "${feature_args[@]}"

    local -a init_flags=("${remap[@]}")
    if [[ "$init_target" == *-gnu ]]; then
        init_flags+=(-C target-feature=+crt-static)
    fi
    note "easyplay-init for $init_target (profile init-release)"
    CARGO_ENCODED_RUSTFLAGS="$(join_flags "${base[@]}" "${init_flags[@]}")" \
        cargo build "${cargo_common[@]}" --profile init-release -p easyplay-init --target "$init_target"
}

join_flags() {
    local IFS=$'\x1f'
    printf '%s' "$*"
}

# ---- Checks -----------------------------------------------------------------------------

check_size() {
    local file="$1" max="$2" size
    size="$(stat -c %s "$file")"
    note "$(basename "$file"): $size bytes (budget $max)"
    ((size <= max)) || die "$(basename "$file") is $size bytes, over the budget of $max (PERFORMANCE §1)"
}

check_static() {
    local file="$1"
    if command -v readelf >/dev/null; then
        if readelf -lW "$file" | grep -q 'INTERP'; then die "$file has a program interpreter (not static)"; fi
        if readelf -dW "$file" 2>/dev/null | grep -q '(NEEDED)'; then die "$file has shared library dependencies"; fi
    elif command -v file >/dev/null; then
        file -b "$file" | grep -Eq 'static(-pie)? linked' || die "$file is not statically linked"
    else
        die "neither readelf nor file is available to check that $file is static"
    fi
}

check() {
    [[ -x "$easyplay_bin" ]] || die "missing $easyplay_bin (run without --stage-only first)"
    [[ -x "$init_bin" ]] || die "missing $init_bin (run without --stage-only first)"
    check_size "$easyplay_bin" "$max_easyplay"
    check_size "$init_bin" "$max_init"
    check_static "$init_bin"

    local got
    got="$("$easyplay_bin" --version)" || die "easyplay --version failed"
    [[ "$got" == *"$version"* ]] || die "easyplay --version says '$got', expected $version"
    note "$got"
}

# ---- Stage ------------------------------------------------------------------------------

# Copy the share/ tree, documentation and licences under $1 (the staged prefix).
stage_share() {
    local out="$1" gen
    install -d "$out/share"
    cp -R "$ROOT/assets/share/." "$out/share/"

    gen="$(mktemp -d "${target_dir}/generate-assets.XXXXXX")"
    "$easyplay_bin" generate-assets --out "$gen"
    install -Dm644 -t "$out/share/man/man1" "$gen"/man/man1/*.1
    install -Dm644 "$gen/completions/easyplay.bash" "$out/share/bash-completion/completions/easyplay"
    install -Dm644 "$gen/completions/_easyplay" "$out/share/zsh/site-functions/_easyplay"
    install -Dm644 "$gen/completions/easyplay.fish" "$out/share/fish/vendor_completions.d/easyplay.fish"
    rm -rf "$gen"

    local lic="$out/share/licenses/easyplay"
    install -Dm644 "$ROOT/LICENSE" "$lic/LICENSE"
    install -Dm644 -t "$lic" "$ROOT"/assets/licenses/*.txt
    # Third-party crates and their licences (RELEASE_STRATEGY §8), from the lock file.
    (
        cd "$ROOT"
        printf 'Rust crates compiled into easyplay %s (name version licence):\n\n' "$version"
        cargo tree "${cargo_common[@]}" -p easyplay -p easyplay-init --target "$target" \
            -e normal,build --prefix none --format '{p} {l}' "${feature_args[@]}" 2>/dev/null |
            sed -e 's/ (\*)$//' -e 's/ (proc-macro)//' -e 's| (/[^)]*)||' |
            grep -v -e '^easyplay' -e '^$' | LC_ALL=C sort -u
    ) >"$lic/THIRD-PARTY.txt"
    chmod 644 "$lic/THIRD-PARTY.txt"

    local doc="$out/share/doc/easyplay"
    install -Dm644 "$ROOT/README.md" "$doc/README.md"
    [[ -f "$ROOT/CHANGELOG.md" ]] && install -Dm644 "$ROOT/CHANGELOG.md" "$doc/CHANGELOG.md"

    find "$out/share" -type d -exec chmod 755 {} +
    find "$out/share" -type f -exec chmod 644 {} +
}

validate_metadata() {
    local out="$1"
    if command -v desktop-file-validate >/dev/null; then
        desktop-file-validate "$out"/share/applications/*.desktop || die "desktop-file-validate failed"
    else
        note "desktop-file-validate not found; skipped"
    fi
    if command -v appstreamcli >/dev/null; then
        appstreamcli validate --no-net --explain "$out"/share/metainfo/*.metainfo.xml ||
            die "appstreamcli validate failed"
    else
        note "appstreamcli not found; skipped"
    fi
}

# Distribution layout: DESTDIR/PREFIX/bin/easyplay, DESTDIR/LIBEXEC_DIR/easyplay-init.
stage_fhs() {
    local root="${destdir:-$ROOT/dist/stage}"
    local out="$root$prefix"
    install -Dm755 "$easyplay_bin" "$out/bin/easyplay"
    install -Dm755 "$init_bin" "$root$libexec_dir/easyplay-init"
    stage_share "$out"
    validate_metadata "$out"
    note "staged into $root (prefix $prefix)"
}

# Relocatable layout for /usr/local or ~/.local: the binary lives next to the shim in
# libexec/easyplay and bin/easyplay is a relative symlink. The shim is found next to the
# running binary (DESKTOP_INTEGRATION §10, third lookup) wherever the tree is installed.
stage_tarball() {
    local name="easyplay-${version}-${ARCH}-linux"
    local dist="$ROOT/dist" out
    out="$dist/$name"
    rm -rf "$out"
    install -Dm755 "$easyplay_bin" "$out/libexec/easyplay/easyplay"
    install -Dm755 "$init_bin" "$out/libexec/easyplay/easyplay-init"
    install -d "$out/bin"
    ln -s ../libexec/easyplay/easyplay "$out/bin/easyplay"
    stage_share "$out"
    validate_metadata "$out"
    install -m755 "$ROOT/packaging/tarball/install.sh" "$ROOT/packaging/tarball/uninstall.sh" "$out/"

    # The file list install.sh copies (and uninstall.sh removes), relative to the prefix.
    (cd "$out" && find bin libexec share \( -type f -o -type l \) | LC_ALL=C sort) >"$out/FILES"
    printf '%s\n' "$version" >"$out/VERSION"
    chmod 644 "$out/FILES" "$out/VERSION"

    local archive="$dist/$name.tar.zst"
    tar -C "$dist" --sort=name --owner=0 --group=0 --numeric-owner \
        --mtime="@$SOURCE_DATE_EPOCH" --pax-option=exthdr.name=%d/PaxHeaders/%f,delete=atime,delete=ctime \
        -cf - "$name" | zstd -q -19 -T0 -f -o "$archive"
    (cd "$dist" && sha256sum "$name.tar.zst" >"$name.tar.zst.sha256")
    note "wrote $archive"
}

# ---- Main -------------------------------------------------------------------------------

((do_build)) && build
check
if ((do_stage)); then
    if ((tarball)); then stage_tarball; else stage_fhs; fi
fi
