#!/usr/bin/env python3
"""Build site/fonts/vt323-termlink.woff2 from upstream VT323.

VT323 has no box-drawing, block or arrow glyphs, so browsers would fall back to
another monospace font and the site's frames would drift off the character
grid. This adds those glyphs on VT323's own 400-unit cell, sized for the site's
1.2 line height, and subsets the result to what the site uses.

Requires fonttools and brotli (`python3 -m pip install fonttools brotli`).
Usage: scripts/site-font.py VT323-Regular.ttf site/fonts/vt323-termlink.woff2
Source: google/fonts ofl/vt323/VT323-Regular.ttf at
eb8781e516576b414603df8cd267c0f21c9b4ee2
(sha256 cf4de751ada78ceac033dbe16a687742939995b77bc2a052ae17a4957958594d).
"""

import hashlib
import sys

from fontTools import subset
from fontTools.pens.ttGlyphPen import TTGlyphPen
from fontTools.ttLib import TTFont

SOURCE_SHA256 = "cf4de751ada78ceac033dbe16a687742939995b77bc2a052ae17a4957958594d"
FAMILY = "VT323 Termlink"
ADVANCE = 400
# A 1.2 line box spans -300..900 around VT323's 800/-200 metrics.
LINE_TOP, LINE_BOTTOM = 910, -310
MID_Y0, MID_Y1 = 260, 340
MID_X0, MID_X1 = 160, 240
PX = 40

# Arms: (left, right, up, down) from the cell centre.
BOX = {
    0x2500: (1, 1, 0, 0),
    0x2502: (0, 0, 1, 1),
    0x250C: (0, 1, 0, 1),
    0x2510: (1, 0, 0, 1),
    0x2514: (0, 1, 1, 0),
    0x2518: (1, 0, 1, 0),
    0x251C: (0, 1, 1, 1),
    0x2524: (1, 0, 1, 1),
    0x252C: (1, 1, 0, 1),
    0x2534: (1, 1, 1, 0),
    0x253C: (1, 1, 1, 1),
}

# 10-wide pixel bitmaps on a 40-unit grid; row 0 spans y=640..680. The cell's
# visual centre (y=300) is row 9.
E = ".........."
LEFT = [E] * 6 + [
    "...#......",
    "..##......",
    ".###......",
    "##########",
    ".###......",
    "..##......",
    "...#......",
]
UP = [E, E] + [
    "....##....",
    "...####...",
    "..######..",
    ".##.##.##.",
] + ["....##...."] * 11
TRIANGLE_UP = [E] * 7 + [
    "....##....",
    "...####...",
    "..######..",
    ".########.",
    "##########",
]
BITMAPS = {
    0x2190: LEFT,
    0x2192: [row[::-1] for row in LEFT],
    0x2191: UP,
    0x2193: [E, E] + UP[2:][::-1],
    0x25B2: TRIANGLE_UP,
    0x25BC: [E] * 7 + TRIANGLE_UP[7:][::-1],
    0x25B6: [E] * 4 + [
        "#.........",
        "###.......",
        "#####.....",
        "#######...",
        "#########.",
        "#######...",
        "#####.....",
        "###.......",
        "#.........",
    ],
    0x25A0: [E] * 5 + [".########."] * 8,
    0x25C6: [E] * 4 + [
        "....##....",
        "...####...",
        "..######..",
        ".########.",
        "##########",
        ".########.",
        "..######..",
        "...####...",
        "....##....",
    ],
    0x25CF: [E] * 7 + [
        "...###....",
        "..#####...",
        "..#####...",
        "..#####...",
        "...###....",
    ],
}


def rect(pen, x0, y0, x1, y1):
    pen.moveTo((x0, y0))
    pen.lineTo((x0, y1))
    pen.lineTo((x1, y1))
    pen.lineTo((x1, y0))
    pen.closePath()


def box_glyph(arms):
    left, right, up, down = arms
    pen = TTGlyphPen(None)
    if left or right:
        rect(pen, -4 if left else MID_X0, MID_Y0, ADVANCE + 4 if right else MID_X1, MID_Y1)
    if up or down:
        rect(pen, MID_X0, LINE_BOTTOM if down else MID_Y0, MID_X1, LINE_TOP if up else MID_Y1)
    return pen.glyph()


def bitmap_glyph(rows):
    pen = TTGlyphPen(None)
    for r, row in enumerate(rows):
        for c, cell in enumerate(row):
            if cell == "#":
                y1 = 680 - r * PX
                rect(pen, c * PX, y1 - PX, (c + 1) * PX, y1)
    return pen.glyph()


def block_glyph(shade):
    pen = TTGlyphPen(None)
    if not shade:
        rect(pen, 0, -200, ADVANCE, 800)
    else:
        for r in range(25):
            for c in range(10):
                if (r + c) % 2 == 0:
                    rect(pen, c * PX, -200 + r * PX, (c + 1) * PX, -200 + (r + 1) * PX)
    return pen.glyph()


def main(source, target):
    with open(source, "rb") as handle:
        digest = hashlib.sha256(handle.read()).hexdigest()
    if digest != SOURCE_SHA256:
        sys.exit(f"unexpected source font sha256 {digest}")
    font = TTFont(source)
    glyphs = {code: box_glyph(arms) for code, arms in BOX.items()}
    glyphs.update({code: bitmap_glyph(rows) for code, rows in BITMAPS.items()})
    glyphs[0x2588] = block_glyph(False)
    glyphs[0x2591] = block_glyph(True)
    order = font.getGlyphOrder()
    for code, glyph in glyphs.items():
        name = f"uni{code:04X}"
        if name not in order:
            order.append(name)
        font["glyf"][name] = glyph
        glyph.recalcBounds(font["glyf"])
        # Renderers place outlines by the hmtx side bearing, so it must be xMin.
        font["hmtx"][name] = (ADVANCE, glyph.xMin)
        for table in font["cmap"].tables:
            if table.isUnicode():
                table.cmap[code] = name
    font.setGlyphOrder(order)
    font["maxp"].numGlyphs = len(order)
    for record in font["name"].names:
        if record.nameID in (1, 16):
            record.string = FAMILY
        elif record.nameID == 4:
            record.string = f"{FAMILY} Regular"
        elif record.nameID == 6:
            record.string = "VT323Termlink-Regular"
    unicodes = list(range(0x20, 0x7F)) + list(range(0xA0, 0x100))
    unicodes += [0x2013, 0x2014, 0x2018, 0x2019, 0x201C, 0x201D, 0x2022, 0x2026]
    unicodes += [0x2039, 0x203A] + list(glyphs)
    options = subset.Options()
    options.flavor = "woff2"
    options.name_IDs = ["*"]
    options.notdef_outline = True
    subsetter = subset.Subsetter(options)
    subsetter.populate(unicodes=unicodes)
    subsetter.subset(font)
    font.flavor = "woff2"
    font.save(target)


if __name__ == "__main__":
    if len(sys.argv) != 3:
        sys.exit(__doc__)
    main(sys.argv[1], sys.argv[2])
