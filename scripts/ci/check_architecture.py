#!/usr/bin/env python3
"""Architecture checks (ARCH-006, ARCHITECTURE §3-4 and §13, TESTING_STRATEGY §11).

Fails (exit 1) when, in the workspace at ROOT (default: the repository):

  core-ui-dep      easyplay-core depends on egui, eframe, winit, wgpu, rfd or another
                   windowing crate (§3 rule 1);
  ui-import        easyplay-ui uses a core module outside the allowed set, or anything
                   under a forbidden path (§3 rule 2, see UI_ALLOWED / UI_FORBIDDEN);
  core-layer       a core module imports a module of a higher layer, or a layer-5 module
                   imports a layer-5 module other than the allowed pairs, or `model`
                   imports more than its vocabulary (§3 rule 3, §4);
  init-deps        easyplay-init depends on anything but libc (§3 rule 4);
  unsafe           `unsafe` appears outside the allowed files (§3 rule 5);
  async            an async runtime is used outside `storage` (§3 rule 6);
  colour-literal   a colour literal appears in easyplay-ui outside `theme/` (§13);
  screen-string    a user-facing sentence is a literal in `easyplay-ui/src/screens`
                   instead of `strings` (heuristic, §13).

Test modules (`#[cfg(test)]` to the end of a file) and comments are not checked.
`--self-test` runs the checker on the seeded violations under scripts/ci/fixtures/arch/
and fails unless each one is reported.
"""

from __future__ import annotations

import re
import sys
from pathlib import Path

# ---- Tables ---------------------------------------------------------------------------------

UI_CRATES = ("egui", "eframe", "egui_extras", "egui_kittest", "winit", "wgpu", "glow", "rfd",
             "gilrs", "accesskit", "accesskit_winit", "raw-window-handle", "glutin")

# Core layers (ARCHITECTURE §4); `ui_thread` is layer-1 vocabulary (PERF-002).
LAYERS = {
    "consts": 1, "ids": 1, "clock": 1, "paths": 1, "error": 1, "model": 1, "ui_thread": 1,
    "jobs": 2, "logging": 2, "fsx": 2, "procfs": 2,
    "storage": 3, "config": 3, "host": 3,
    "identity": 4, "sandbox": 4, "umu": 4,
    "runtime": 5, "launch": 5, "session": 5, "library": 5, "stats": 5, "artwork": 5,
    "desktop": 5, "diagnostics": 5,
    "service": 6,
}
# Allowed imports between layer-5 modules (§4).
LAYER5_ALLOWED = {
    ("launch", "runtime"), ("launch", "session"),
    ("session", "stats"), ("session", "runtime"),
    ("library", "artwork"),
    ("diagnostics", "runtime"),
}
# `model` imports nothing but these (§3 rule 3).
MODEL_ALLOWED = ("ids", "consts", "clock::Timestamp", "error::ErrorCode", "error::Error", "model")
# Item-level exceptions to the layer rules, each with its reason.
LAYER_EXCEPTIONS = {
    # The launch plan is `launch`'s output and the supervisor's input (PROCESS_LIFECYCLE §2);
    # its types live in `launch` although `launch` also depends on `session`. Moving them to
    # `model` would remove the exception.
    ("session", "launch"): ("LaunchPlan", "plan_file", "PlanSandbox", "PlanFd", "FdKind",
                            "PrefixLocks"),
}

# What easyplay-ui may name in easyplay_core (§3 rule 2 as enforced): vocabulary and the
# pure planning/query functions, the service façade, and the state types it displays.
UI_ALLOWED = ("model", "ids", "clock", "consts", "error", "config", "service", "library",
              "sandbox", "umu", "launch", "stats", "artwork", "jobs", "desktop", "host",
              "ui_thread", "identity")
# Never, anywhere in the UI (outside tests): storage, filesystem and process mechanisms,
# planners and renderers, the supervisor, umu's command builder.
UI_FORBIDDEN = (
    r"storage", r"fsx", r"procfs", r"session", r"runtime", r"logging", r"paths",
    r"sandbox::(bwrap|planner|plan\b|render|seccomp|fd|sections|context)",
    r"sandbox::(plan|render)\s*\(",
    r"umu::(command|locate)", r"host::(runner|RealRunner|probe|LiveHostProbe|bwrap_probe|exec)",
    r"jobs::(JobPool|pool)", r"launch::(orchestrator|plan_file|planner|trusted|preflight|shell)",
    r"library::(service|LibraryService|remove|relink|prefix_ops|export|installer|missing)",
    r"stats::(rebuild|recorder|apply)", r"identity::(pe|fingerprint|PeInspector|lnk)",
    r"artwork::(import|http|steam|store|icon_extract)", r"desktop::(mime|shortcuts|instance|open_uri|notify)",
)

UNSAFE_ALLOWED = (
    "crates/easyplay-init/",
    "crates/easyplay-core/src/session/sys.rs",
    "crates/easyplay-core/src/sandbox/fd.rs",
    # Test programs run inside the sandbox or in place of bwrap (PROC-013, SEC-002).
    "crates/easyplay-testkit/src/bin/",
    "fuzz/",
)

ASYNC_RUNTIMES = ("tokio", "async-std", "smol", "futures-executor")

# ---- Helpers --------------------------------------------------------------------------------


def strip_tests_and_comments(text: str) -> list[tuple[int, str]]:
    """Lines before the first `#[cfg(test)]`, without `//` comments (strings kept)."""
    out = []
    for no, line in enumerate(text.splitlines(), 1):
        if line.strip().startswith("#[cfg(test)]") or line.strip().startswith("#[cfg(all(test"):
            break
        stripped = line.lstrip()
        if stripped.startswith("//"):
            continue
        # Drop a trailing comment that is not inside a string literal.
        code, in_str, i = [], False, 0
        while i < len(line):
            c = line[i]
            if c == '"' and (i == 0 or line[i - 1] != "\\"):
                in_str = not in_str
            if not in_str and line.startswith("//", i):
                break
            code.append(c)
            i += 1
        out.append((no, "".join(code)))
    return out


def dependencies(cargo_toml: Path) -> dict[str, list[str]]:
    """Dependency names per section ([dependencies], [dev-dependencies], ...)."""
    sections: dict[str, list[str]] = {}
    current = None
    for line in cargo_toml.read_text().splitlines():
        m = re.match(r"^\[(.+)\]\s*$", line.strip())
        if m:
            current = m.group(1)
            continue
        if current and "dependencies" in current:
            m = re.match(r"^([A-Za-z0-9_-]+)\s*(\.workspace)?\s*=", line.strip())
            if m:
                sections.setdefault(current, []).append(m.group(1))
    return sections


def split_top(body: str) -> list[str]:
    depth, item, items = 0, "", []
    for c in body:
        if c == "{":
            depth += 1
        elif c == "}":
            depth -= 1
        if c == "," and depth == 0:
            items.append(item)
            item = ""
        else:
            item += c
    items.append(item)
    return [re.sub(r"\s+", "", i) for i in items if i.strip()]


def expand(prefix: str, body: str) -> list[str]:
    out = []
    for it in split_top(body):
        m = re.match(r"^([A-Za-z0-9_:]*?)(?:::)?\{(.*)\}$", it)
        if m:
            inner = (prefix + "::" + m.group(1)).strip(":") if m.group(1) else prefix
            out.extend(expand(inner, m.group(2)))
        else:
            out.append((prefix + "::" + it).strip(":") if prefix else it)
    return out


def use_paths(lines: list[tuple[int, str]], crate: str) -> list[tuple[int, str]]:
    """Every `crate::a::b` path, with grouped `use crate::a::{b, c::{d}}` imports expanded."""
    text = "\n".join(f"\x01{no}\x02{line}" for no, line in lines)
    out = []
    for m in re.finditer(rf"\b{crate}::((?:[A-Za-z_][A-Za-z0-9_]*::)*)(\{{|[A-Za-z_][A-Za-z0-9_]*)", text):
        no = int(text[: m.start()].rsplit("\x01", 1)[-1].split("\x02", 1)[0])
        prefix = m.group(1).rstrip(":")
        if m.group(2) != "{":
            out.append((no, (prefix + "::" + m.group(2)).strip(":")))
            continue
        depth, i = 1, m.end()
        while i < len(text) and depth:
            depth += {"{": 1, "}": -1}.get(text[i], 0)
            i += 1
        body = re.sub(r"\x01\d+\x02", "", text[m.end(): i - 1])
        out.extend((no, p) for p in expand(prefix, body))
    return out


class Report:
    def __init__(self) -> None:
        self.problems: list[str] = []

    def add(self, rule: str, where: str, detail: str) -> None:
        self.problems.append(f"{rule}: {where}: {detail}")


# ---- Checks ---------------------------------------------------------------------------------


def check_core_deps(root: Path, r: Report) -> None:
    toml = root / "crates/easyplay-core/Cargo.toml"
    if not toml.exists():
        return
    for section, names in dependencies(toml).items():
        if section.startswith("dev-"):
            continue
        for n in names:
            if n in UI_CRATES:
                r.add("core-ui-dep", str(toml.relative_to(root)), f"[{section}] {n}")


def check_init_deps(root: Path, r: Report) -> None:
    toml = root / "crates/easyplay-init/Cargo.toml"
    if not toml.exists():
        return
    for section, names in dependencies(toml).items():
        if section == "dependencies":
            for n in names:
                if n != "libc":
                    r.add("init-deps", str(toml.relative_to(root)), n)


def check_ui_imports(root: Path, r: Report) -> None:
    src = root / "crates/easyplay-ui/src"
    if not src.exists():
        return
    for f in sorted(src.rglob("*.rs")):
        rel = str(f.relative_to(root))
        lines = strip_tests_and_comments(f.read_text())
        for no, path in use_paths(lines, "easyplay_core"):
            top = path.split("::", 1)[0]
            if top not in UI_ALLOWED:
                r.add("ui-import", f"{rel}:{no}", f"easyplay_core::{path} (module `{top}` not allowed)")
                continue
            for pat in UI_FORBIDDEN:
                if re.match(pat + r"(\b|$)", path):
                    r.add("ui-import", f"{rel}:{no}", f"easyplay_core::{path} (forbidden)")
                    break


def module_of(src: Path, f: Path) -> str:
    rel = f.relative_to(src)
    return rel.parts[0].removesuffix(".rs")


def check_core_layers(root: Path, r: Report) -> None:
    src = root / "crates/easyplay-core/src"
    if not src.exists():
        return
    for f in sorted(src.rglob("*.rs")):
        if f.name in ("lib.rs",):
            continue
        me = module_of(src, f)
        if me not in LAYERS:
            r.add("core-layer", str(f.relative_to(root)), f"module `{me}` missing from the layer table")
            continue
        rel = str(f.relative_to(root))
        lines = strip_tests_and_comments(f.read_text())
        for no, path in use_paths(lines, "crate"):
            dep = path.split("::", 1)[0]
            if dep == me or dep not in LAYERS:
                continue
            lm, ld = LAYERS[me], LAYERS[dep]
            if me == "model":
                if not any(path == a or path.startswith(a + "::") or path.startswith(a + "{")
                           for a in MODEL_ALLOWED):
                    r.add("core-layer", f"{rel}:{no}", f"model imports crate::{path}")
                continue
            if ld > lm:
                r.add("core-layer", f"{rel}:{no}", f"layer {lm} `{me}` imports layer {ld} `{dep}` (crate::{path})")
            elif ld == lm == 5 and (me, dep) not in LAYER5_ALLOWED:
                item = path.split("::")[1] if "::" in path else ""
                if item in LAYER_EXCEPTIONS.get((me, dep), ()):
                    continue
                r.add("core-layer", f"{rel}:{no}", f"layer-5 `{me}` imports `{dep}` (not an allowed pair)")


def check_unsafe(root: Path, r: Report) -> None:
    for base in ("crates", "fuzz"):
        d = root / base
        if not d.exists():
            continue
        for f in sorted(d.rglob("*.rs")):
            rel = str(f.relative_to(root))
            if "/target/" in rel or any(rel.startswith(a) for a in UNSAFE_ALLOWED):
                continue
            for no, line in strip_tests_and_comments(f.read_text()):
                code = re.sub(r'"([^"\\]|\\.)*"', '""', line)
                if re.search(r"\bunsafe\s*(\{|fn\b|impl\b|extern\b)|allow\(unsafe_code\)", code):
                    r.add("unsafe", f"{rel}:{no}", line.strip())


def check_async(root: Path, r: Report) -> None:
    for toml in sorted((root / "crates").glob("*/Cargo.toml")):
        crate = toml.parent.name
        for section, names in dependencies(toml).items():
            if section.startswith("dev-"):
                continue
            for n in names:
                if n in ASYNC_RUNTIMES and crate != "easyplay-core":
                    r.add("async", str(toml.relative_to(root)), n)
    src = root / "crates/easyplay-core/src"
    if src.exists():
        for f in sorted(src.rglob("*.rs")):
            if module_of(src, f) == "storage":
                continue
            for no, line in strip_tests_and_comments(f.read_text()):
                if re.search(r"\btokio::", line):
                    r.add("async", f"{f.relative_to(root)}:{no}", "tokio outside storage")


def check_colours(root: Path, r: Report) -> None:
    src = root / "crates/easyplay-ui/src"
    if not src.exists():
        return
    for f in sorted(src.rglob("*.rs")):
        rel = str(f.relative_to(root))
        if "/theme/" in rel:
            continue
        for no, line in strip_tests_and_comments(f.read_text()):
            if (re.search(r"Color32::from_(rgb|rgba_premultiplied|rgba_unmultiplied|gray)\(\s*\d", line)
                    or re.search(r"Color32::from_hex\(", line)
                    or re.search(r'"#[0-9A-Fa-f]{6}([0-9A-Fa-f]{2})?"', line)):
                r.add("colour-literal", f"{rel}:{no}", line.strip())


SENTENCE = re.compile(r'"([A-Z][a-z\']+(?: [A-Za-z\'(),-]+){2,}[^"]*)"')
IGNORED_MACROS = re.compile(r"\b(debug_assert\w*|assert\w*|tracing::\w+|trace|debug|info|warn|error|panic|unreachable|expect)!?\(")


def check_screen_strings(root: Path, r: Report) -> None:
    src = root / "crates/easyplay-ui/src/screens"
    if not src.exists():
        return
    for f in sorted(src.rglob("*.rs")):
        rel = str(f.relative_to(root))
        for no, line in strip_tests_and_comments(f.read_text()):
            if IGNORED_MACROS.search(line):
                continue
            for m in SENTENCE.finditer(line):
                r.add("screen-string", f"{rel}:{no}", m.group(0))


CHECKS = (check_core_deps, check_init_deps, check_ui_imports, check_core_layers, check_unsafe,
          check_async, check_colours, check_screen_strings)


def run(root: Path) -> list[str]:
    r = Report()
    for c in CHECKS:
        c(root, r)
    return r.problems


def self_test(here: Path) -> int:
    fixtures = here / "fixtures/arch"
    failed = 0
    for case in sorted(p for p in fixtures.iterdir() if p.is_dir()):
        expect = (case / "EXPECT").read_text().split()
        got = run(case)
        rules = {p.split(":", 1)[0] for p in got}
        for e in expect:
            if e == "clean":
                ok = not got
            else:
                ok = e in rules
            if not ok:
                failed += 1
                print(f"self-test {case.name}: expected {e}, got {got}")
        if "clean" not in expect and (rules - set(expect)):
            failed += 1
            print(f"self-test {case.name}: unexpected {sorted(rules - set(expect))}: {got}")
    print(f"self-test: {'ok' if not failed else f'{failed} failure(s)'}")
    return 1 if failed else 0


def main(argv: list[str]) -> int:
    here = Path(__file__).resolve().parent
    if "--self-test" in argv:
        return self_test(here)
    root = Path(argv[1]) if len(argv) > 1 else here.parent.parent
    problems = run(root)
    for p in problems:
        print(p)
    print(f"architecture: {len(problems)} problem(s)")
    return 1 if problems else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
