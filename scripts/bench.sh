#!/usr/bin/env bash
# Run every PERFORMANCE §1 bench optimised and gate the results (PERF-001).
#
#   scripts/bench.sh [RESULTS.jsonl] [-- bench-gate options, e.g. --record]
#
# Builds with `--profile perf` (release code generation in target/perf, so release
# binaries are not replaced) and appends one JSON line per row to RESULTS (default
# target/bench-results.jsonl), then runs scripts/bench-gate.py on it.
set -euo pipefail
root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "${root}"
out="${CARGO_TARGET_DIR:-${root}/target}/bench-results.jsonl"
if [[ $# -gt 0 && "$1" != "--" ]]; then out="$1"; shift; fi
# Absolute: `cargo bench` runs each bench from its crate's folder.
[[ "${out}" = /* ]] || out="${root}/${out}"
[[ "${1:-}" == "--" ]] && shift
rm -f "${out}"
mkdir -p "$(dirname "${out}")"
ZAUN_BENCH_OUT="${out}" cargo bench --locked --profile perf -p zaun-core -p zaun-ui
exec python3 scripts/bench-gate.py "${out}" "$@"
