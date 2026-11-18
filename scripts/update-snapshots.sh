#!/usr/bin/env bash
# UI snapshot workflow (TEST-030, TESTING_STRATEGY §9).
#
#   scripts/update-snapshots.sh            re-run the UI tests, rewrite the snapshots that
#                                          fail, and write a review page
#   scripts/update-snapshots.sh --force    rewrite every snapshot (after a deliberate
#                                          theme change)
#   scripts/update-snapshots.sh --check    only run the snapshot tests (what CI does)
#   scripts/update-snapshots.sh --clean    delete *.old.png / *.new.png / *.diff.png
#
# Extra arguments after `--` go to `cargo test` (for example `-- --test it details::`; most
# UI tests are modules of the one `it` binary).
#
# Review: open target/snapshot-review/index.html (old | new | diff for every changed
# image), then commit the changed PNGs with the code change that caused them. Snapshots are
# changed only after this visual review; `git diff --stat crates/*/tests/snapshots` lists
# them. Rendering is deterministic: fonts are embedded (UI-003) and `kittest.toml` sets the
# tolerance; CI renders with Mesa's lavapipe (software Vulkan), see .github/workflows/ci.yml.
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "${root}"
mode="update"
cargo_args=()
while [[ $# -gt 0 ]]; do
    case "$1" in
        --force) mode="force" ;;
        --check) mode="check" ;;
        --clean) mode="clean" ;;
        --) shift; cargo_args=("$@"); break ;;
        -h|--help) sed -n '2,19p' "$0"; exit 0 ;;
        *) echo "unknown option $1" >&2; exit 2 ;;
    esac
    shift
done

snapshot_dirs=(crates/*/tests/snapshots)

clean() {
    find "${snapshot_dirs[@]}" \( -name '*.old.png' -o -name '*.new.png' -o -name '*.diff.png' \) -delete
}

case "${mode}" in
    clean) clean; exit 0 ;;
    check) exec cargo test -p zaun-ui --tests "${cargo_args[@]}" ;;
    update) export UPDATE_SNAPSHOTS=1 ;;
    force) export UPDATE_SNAPSHOTS=force ;;
esac

clean
status=0
cargo test -p zaun-ui --tests "${cargo_args[@]}" || status=$?

# The review page: every snapshot the run rewrote has a *.old.png backup next to it.
review="${CARGO_TARGET_DIR:-${root}/target}/snapshot-review"
mkdir -p "${review}"
page="${review}/index.html"
changed=0
{
    echo '<!doctype html><meta charset="utf-8"><title>Snapshot review</title>'
    echo '<style>body{font:14px sans-serif;background:#222;color:#eee}img{max-width:32vw;border:1px solid #555}td{vertical-align:top;padding:4px}</style>'
    echo '<h1>Changed snapshots</h1><table><tr><th>old</th><th>new</th><th>diff</th></tr>'
    while IFS= read -r old; do
        changed=$((changed + 1))
        base="${old%.old.png}"
        name="$(basename "${base}")"
        diff=""
        [[ -f "${base}.diff.png" ]] && diff="<img src=\"file://${root}/${base}.diff.png\">"
        echo "<tr><td colspan=3><b>${name}</b></td></tr>"
        echo "<tr><td><img src=\"file://${root}/${old}\"></td><td><img src=\"file://${root}/${base}.png\"></td><td>${diff}</td></tr>"
    done < <(find "${snapshot_dirs[@]}" -name '*.old.png' | sort)
    echo '</table>'
} > "${page}"

new_files="$(git status --porcelain -- "${snapshot_dirs[@]}" | grep -c '^??.*\.png$' || true)"
echo
echo "snapshots rewritten: ${changed}; new snapshot files: ${new_files}"
if [[ ${changed} -gt 0 ]]; then
    echo "review: ${page}"
    echo "then:   git add -p crates/*/tests/snapshots && scripts/update-snapshots.sh --clean"
fi
exit "${status}"
