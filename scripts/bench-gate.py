#!/usr/bin/env python3
"""Benchmark regression gate (PERF-001, PERFORMANCE §1, TESTING_STRATEGY §11).

    scripts/bench-gate.py RESULTS.jsonl [--baseline FILE] [--record] [--strict-budget]

RESULTS is what the bench binaries append to `$EASYPLAY_BENCH_OUT` (one JSON object per
row). For every row the gate compares the measured statistic (median or p99, as the row
says) with

  * the recorded baseline: a regression is more than 25 % slower than the baseline
    (fails the gate);
  * the budget of PERFORMANCE §1: over budget is reported, and fails the gate only with
    `--strict-budget` (budgets hold on the reference machine; CI runners differ).

`--record` writes RESULTS as the new baseline (run on the CI runner class, after review).
"""

from __future__ import annotations

import json
import sys
from pathlib import Path

TOLERANCE = 1.25


def load(path: Path) -> dict[str, dict]:
    rows = {}
    for line in path.read_text().splitlines():
        line = line.strip()
        if line:
            r = json.loads(line)
            rows[r["name"]] = r
    return rows


def measured(r: dict) -> float:
    return r["p99_us"] if r["stat"] == "p99" else r["median_us"]


def main(argv: list[str]) -> int:
    if len(argv) < 2:
        print(__doc__)
        return 2
    results = Path(argv[1])
    baseline = Path(__file__).resolve().parent / "ci" / "bench-baseline.jsonl"
    if "--baseline" in argv:
        baseline = Path(argv[argv.index("--baseline") + 1])
    rows = load(results)
    if not all(r.get("optimized") for r in rows.values()):
        print("bench-gate: results come from a debug build; run `cargo bench --profile perf`")
        return 2
    if "--record" in argv:
        baseline.write_text("".join(json.dumps(r) + "\n" for r in rows.values()))
        print(f"bench-gate: recorded {len(rows)} rows in {baseline}")
        return 0
    base = load(baseline) if baseline.exists() else {}
    failed = 0
    print(f"{'row':<28} {'measured':>12} {'budget':>12} {'baseline':>12}  verdict")
    for name, r in sorted(rows.items()):
        m = measured(r)
        verdict = []
        if m > r["budget_us"]:
            verdict.append("over budget")
            if "--strict-budget" in argv:
                failed += 1
        b = base.get(name)
        if b is not None and m > measured(b) * TOLERANCE:
            verdict.append(f"REGRESSION (> {TOLERANCE:.2f} x baseline)")
            failed += 1
        print(
            f"{name:<28} {m:>10.1f}µs {r['budget_us']:>10.1f}µs "
            f"{(f'{measured(b):.1f}µs' if b else '-'):>12}  {', '.join(verdict) or 'ok'}"
        )
    if not base:
        print("bench-gate: no baseline recorded yet; regressions are not checked")
    return 1 if failed else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
