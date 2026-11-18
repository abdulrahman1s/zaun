#!/usr/bin/env python3
"""Build Zaun's bundled fonts (UI_DESIGN_SYSTEM §3, §6, §9).

Outputs (all committed; rerun only when the icon list or a source font changes):

  assets/fonts/zaun-icons.ttf          Phosphor subset, regular at U+E000+i, fill at U+E100+i
  assets/fonts/Inter-Tabular.ttf           Inter Variable digits with tabular figures baked in
  assets/fonts/Inter-Variable.ttf          copied
  assets/fonts/JetBrainsMono-Regular.ttf   copied
  assets/fonts/InstrumentSans_Condensed-Bold.ttf          the display face: Instrument Sans
                                           instanced at wdth 75, wght 700
  assets/fonts/InstrumentSans_Condensed-Bold-Tabular.ttf  its digits with tabular figures
  crates/zaun-ui/src/theme/icons_gen.rs

Usage (from the repository root); every source is optional, and only what is given is built:

  nix shell --impure --expr 'with import <nixpkgs> {}; python3.withPackages (ps: [ ps.fonttools ])' \\
    -c python3 -I assets/tools/build_fonts.py \\
       --phosphor <@phosphor-icons/web 2.x package dir/src> \\
       --inter <InterVariable.ttf 4.x> --mono <JetBrainsMono-Regular.ttf 2.304> \\
       --display <InstrumentSans[wdth,wght].ttf>

The tabular faces exist because egui 0.37 does not apply OpenType features, so `tnum` is
applied here once by remapping the digit code points to their tabular glyphs.
"""

import argparse
import re
import shutil
from pathlib import Path

from fontTools import subset
from fontTools.pens.cu2quPen import Cu2QuPen
from fontTools.pens.recordingPen import DecomposingRecordingPen
from fontTools.pens.ttGlyphPen import TTGlyphPen
from fontTools.fontBuilder import FontBuilder
from fontTools.ttLib import TTFont
from fontTools.varLib import instancer

ROOT = Path(__file__).resolve().parents[2]
FONTS = ROOT / "assets" / "fonts"
GEN_RS = ROOT / "crates" / "zaun-ui" / "src" / "theme" / "icons_gen.rs"
REGULAR_BASE = 0xE000
FILL_BASE = 0xE100


def parse_manifest():
    icons = []
    for line in (FONTS / "icons.txt").read_text().splitlines():
        line = line.split("#", 1)[0].strip()
        if not line:
            continue
        parts = line.split()
        name, source = parts[0], parts[1]
        kind = parts[2] if len(parts) > 2 else "extra"
        icons.append((name, source, kind))
    return icons


def css_codepoints(css_path, prefix):
    text = Path(css_path).read_text()
    pat = re.compile(r"\." + re.escape(prefix) + r"\.ph-([a-z0-9-]+):before\s*\{\s*content:\s*\"\\([0-9a-f]+)\"")
    return {m.group(1): int(m.group(2), 16) for m in pat.finditer(text)}


def svg(x, y):
    """Phosphor's 256-unit SVG grid to font units (1024 per em, ascent 960)."""
    return (x * 4.0, 960.0 - y * 4.0)


def draw_rect(pen, x0, y0, x1, y1):
    # Clockwise in font space (y up) so it fills inside the outline's hole.
    a, b = svg(x0, y1), svg(x1, y0)
    pen.moveTo((a[0], a[1]))
    pen.lineTo((a[0], b[1]))
    pen.lineTo((b[0], b[1]))
    pen.lineTo((b[0], a[1]))
    pen.closePath()


def draw_circle(pen, cx, cy, r):
    c = svg(cx, cy)
    rr = r * 4.0
    k = 0.5522847498 * rr
    x, y = c
    # Clockwise in font space: top, right, bottom, left.
    pen.moveTo((x, y + rr))
    pen.curveTo((x + k, y + rr), (x + rr, y + k), (x + rr, y))
    pen.curveTo((x + rr, y - k), (x + k, y - rr), (x, y - rr))
    pen.curveTo((x - k, y - rr), (x - rr, y - k), (x - rr, y))
    pen.curveTo((x - rr, y + k), (x - k, y + rr), (x, y + rr))
    pen.closePath()


def glyph_from(font, cp):
    gs = font.getGlyphSet()
    name = font.getBestCmap()[cp]
    rec = DecomposingRecordingPen(gs)
    gs[name].draw(rec)
    return rec, font["hmtx"][name][0]


def build_glyph(rec, extra=None):
    pen = TTGlyphPen(None)
    qpen = Cu2QuPen(pen, max_err=1.0, reverse_direction=False)
    rec.replay(qpen)
    if extra:
        extra(qpen)
    return pen.glyph()


def build_icons(phosphor_dir):
    reg_font = TTFont(Path(phosphor_dir) / "regular" / "Phosphor.ttf")
    fill_font = TTFont(Path(phosphor_dir) / "fill" / "Phosphor-Fill.ttf")
    reg_cp = css_codepoints(Path(phosphor_dir) / "regular" / "style.css", "ph")
    fill_cp = css_codepoints(Path(phosphor_dir) / "fill" / "style.css", "ph-fill")
    icons = parse_manifest()

    glyphs = {}
    metrics = {}

    def take(font, cps, src, extra=None):
        rec, adv = glyph_from(font, cps[src])
        return build_glyph(rec, extra), adv

    band = lambda p: draw_rect(p, 48, 92, 208, 120)
    dot = lambda p: draw_circle(p, 128, 128, 22)

    for i, (name, source, _kind) in enumerate(icons):
        if source == "@band":
            r = take(reg_font, reg_cp, "shield", band)
            f = take(fill_font, fill_cp, "shield")
        elif source == "@dot":
            r = take(reg_font, reg_cp, "shield", dot)
            f = r
        elif source.endswith("!fill"):
            base = source[: -len("!fill")]
            r = f = take(fill_font, fill_cp, base)
        else:
            r = take(reg_font, reg_cp, source)
            f = take(fill_font, fill_cp, source)
        for variant, (g, adv), base in (("regular", r, REGULAR_BASE), ("fill", f, FILL_BASE)):
            gname = f"{name}.{variant}"
            glyphs[gname] = (base + i, g, adv)

    order = [".notdef", "space"] + sorted(glyphs, key=lambda n: glyphs[n][0])
    empty = TTGlyphPen(None).glyph()
    glyf = {".notdef": empty, "space": TTGlyphPen(None).glyph()}
    hmtx = {".notdef": (1024, 0), "space": (512, 0)}
    cmap = {0x20: "space"}
    for gname, (cp, g, adv) in glyphs.items():
        glyf[gname] = g
        hmtx[gname] = (adv, 0)
        cmap[cp] = gname
    fb = FontBuilder(1024, isTTF=True)
    fb.setupGlyphOrder(order)
    fb.setupCharacterMap(cmap)
    fb.setupGlyf(glyf)
    for gname in order:
        g = fb.font["glyf"][gname]
        g.recalcBounds(fb.font["glyf"])
        hmtx[gname] = (hmtx[gname][0], getattr(g, "xMin", 0))
    fb.setupHorizontalMetrics(hmtx)
    fb.setupHorizontalHeader(ascent=960, descent=-64)
    fb.setupNameTable({"familyName": "Zaun Icons", "styleName": "Regular"})
    fb.setupOS2(sTypoAscender=960, sTypoDescender=-64, usWinAscent=960, usWinDescent=64)
    fb.setupPost()
    out = fb.font
    out.save(FONTS / "zaun-icons.ttf")
    return icons


def bake_tabular(font, out_path, family, ps_name):
    """Keep only the digits of `font`, with `tnum` applied, and save it as `family`."""
    gsub = font["GSUB"].table
    cmap = font.getBestCmap()
    digits = {cp: cmap[cp] for cp in range(0x30, 0x3A)}
    tnum = {}
    for fr in gsub.FeatureList.FeatureRecord:
        if fr.FeatureTag != "tnum":
            continue
        for li in fr.Feature.LookupListIndex:
            lookup = gsub.LookupList.Lookup[li]
            for st in lookup.SubTable:
                mapping = getattr(st, "mapping", None)
                if mapping is None and hasattr(st, "ExtSubTable"):
                    mapping = getattr(st.ExtSubTable, "mapping", None)
                if mapping:
                    tnum.update(mapping)
    if not any(g in tnum for g in digits.values()):
        raise SystemExit(f"no tnum glyphs in {family}")
    # Remap the digit code points to their tabular glyphs, then keep only those. A digit
    # without a tabular alternate is tabular as drawn.
    for t in font["cmap"].tables:
        if not t.isUnicode():
            continue
        for cp, g in digits.items():
            if cp in t.cmap:
                t.cmap[cp] = tnum.get(g, g)
    opts = subset.Options()
    opts.layout_features = []
    opts.name_IDs = ["*"]
    opts.notdef_outline = True
    sub = subset.Subsetter(opts)
    sub.populate(unicodes=list(digits))
    sub.subset(font)
    for rec in font["name"].names:
        if rec.nameID in (1, 4, 16):
            rec.string = family
        elif rec.nameID == 6:
            rec.string = ps_name
    font.save(out_path)


def build_tabular(inter_path):
    bake_tabular(TTFont(inter_path), FONTS / "Inter-Tabular.ttf", "Inter Tabular", "InterTabular")


def build_display(variable_path):
    """The display face (UI_DESIGN_SYSTEM §3): Instrument Sans Condensed Bold, one static
    instance of the variable font, and its tabular digits."""
    out = FONTS / "InstrumentSans_Condensed-Bold.ttf"
    static = instancer.instantiateVariableFont(
        TTFont(variable_path), {"wdth": 75, "wght": 700}, updateFontNames=True
    )
    static.save(out)
    bake_tabular(
        TTFont(out),
        FONTS / "InstrumentSans_Condensed-Bold-Tabular.ttf",
        "Instrument Sans Condensed Tabular",
        "InstrumentSansCondensed-BoldTabular",
    )


def write_rust(icons):
    lines = [
        "// @generated by assets/tools/build_fonts.py from assets/fonts/icons.txt. Do not edit.",
        "",
        "use super::Icon;",
        "",
    ]
    for i, (name, source, kind) in enumerate(icons):
        const = name.upper().replace(".", "_")
        lines.append(f"/// `{name}` (Phosphor `{source}`, {kind}).")
        lines.append(
            f"pub const {const}: Icon = Icon::new(\"{name}\", '\\u{{{REGULAR_BASE + i:04X}}}', "
            f"'\\u{{{FILL_BASE + i:04X}}}', {'true' if kind == 'doc' else 'false'});"
        )
    lines.append("")
    lines.append("/// Every icon, in manifest order.")
    lines.append("pub const ALL: &[Icon] = &[")
    for name, _s, _k in icons:
        lines.append(f"    {name.upper().replace('.', '_')},")
    lines.append("];")
    GEN_RS.write_text("\n".join(lines) + "\n")


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--phosphor")
    ap.add_argument("--inter")
    ap.add_argument("--mono")
    ap.add_argument("--display")
    args = ap.parse_args()
    FONTS.mkdir(parents=True, exist_ok=True)
    if args.phosphor:
        icons = build_icons(args.phosphor)
        write_rust(icons)
        print(f"{len(icons)} icons")
    if args.inter:
        build_tabular(args.inter)
        shutil.copyfile(args.inter, FONTS / "Inter-Variable.ttf")
    if args.mono:
        shutil.copyfile(args.mono, FONTS / "JetBrainsMono-Regular.ttf")
    if args.display:
        build_display(args.display)
        print("display face")


if __name__ == "__main__":
    main()
