"""Bake the smooth glyph tables (GFX-095, GFX-102).

Printable ASCII is drawn from Fira Mono Medium (assets/fonts, SIL Open
Font License 1.1): a monospace face whose advance is 0.6em, so at
13.33px every glyph is exactly the 8x16 cell's 8 pixels wide and the
desk's layout, which counts cells, is unchanged. The baseline sits on
row 12, where the bitmap font's is, so descenders keep rows 13-15.

The other glyphs come from the 8x16 bitmap the text console has always
used, enlarged with Scale2x -- an edge-directed pixel-art upscaler that
keeps straight stems straight and turns staircases into diagonals and
corners into curves -- and then reduced again by area.

Two tables come out, next to the font data:

- font_aa_8x16.bin: 128 glyphs x 8x16 alpha bytes: the face's coverage
  at 1x (or the bitmap's 4x enlargement boxed back to 1x).
- font_hi8_8x16.bin: 128 glyphs x 64x128 bits (8 bytes a row): the glyph
  eight times the size, which scaled text samples 4x4 a pixel for its
  coverage.

    python3 tools/art/font_smooth.py      (needs Pillow)
"""
import os, re
from PIL import Image, ImageDraw, ImageFont

HERE = os.path.dirname(__file__)
SRC = os.path.join(HERE, "..", "..", "graphics_rasterizer", "src")
FACE = os.path.join(HERE, "..", "..", "assets", "fonts", "FiraMono-Medium.woff2")
# 8 px of advance at 0.6em; the baseline on row 12 of 16.
SIZE, BASELINE = 8 / 0.6, 12


def face_big(face, ch, shift):
    """`ch` drawn by the face eight times the cell (64x128), moved right by
    `shift` of those pixels: alpha rows."""
    im = Image.new("L", (64, 128), 0)
    ImageDraw.Draw(im).text((shift, BASELINE * 8), ch, font=face, fill=255, anchor="ls")
    px = im.load()
    return [[px[x, y] for x in range(64)] for y in range(128)]


def box(big):
    """The 8x drawing boxed down to the 8x16 cell."""
    return [
        [
            (sum(big[y * 8 + dy][x * 8 + dx] for dy in range(8) for dx in range(8)) + 32) // 64
            for x in range(8)
        ]
        for y in range(16)
    ]


def snapped(face, ch):
    """`ch` moved sideways by up to half a pixel to where its stems land on
    whole pixels (GFX-102): the shift whose 1x coverage is sharpest, as the
    sum of its squares. This is the one hint the face gets, and it is only
    ever horizontal, so every glyph keeps the baseline."""
    best = None
    for shift in range(-4, 5):
        big = face_big(face, ch, shift)
        cell = box(big)
        sharp = sum(v * v for row in cell for v in row)
        if best is None or sharp > best[0]:
            best = (sharp, cell, big)
    return best[1], best[2]


def load_font(path):
    text = re.sub(r"//[^\n]*", "", open(path).read())
    glyphs = []
    for m in re.finditer(r"\[([^\[\]]*)\]", text):
        body = m.group(1).strip()
        if ";" in body:
            v, n = body.split(";")
            glyphs.append([int(v.strip(), 0)] * int(n.strip()))
        else:
            glyphs.append([int(x.strip(), 0) for x in body.split(",") if x.strip()])
    assert len(glyphs) == 128, len(glyphs)
    return glyphs


def bitmap(rows):
    return [[(r >> (7 - c)) & 1 for c in range(8)] for r in rows]


def scale2x(g):
    h, w = len(g), len(g[0])
    at = lambda y, x: g[y][x] if 0 <= y < h and 0 <= x < w else 0
    out = [[0] * (w * 2) for _ in range(h * 2)]
    for y in range(h):
        for x in range(w):
            p = g[y][x]
            a, b, c, d = at(y - 1, x), at(y, x + 1), at(y, x - 1), at(y + 1, x)
            e1 = e2 = e3 = e4 = p
            if c == a and c != d and a != b:
                e1 = a
            if a == b and a != c and b != d:
                e2 = b
            if d == c and d != b and c != a:
                e3 = c
            if b == d and b != a and d != c:
                e4 = d
            out[2 * y][2 * x], out[2 * y][2 * x + 1] = e1, e2
            out[2 * y + 1][2 * x], out[2 * y + 1][2 * x + 1] = e3, e4
    return out


def main():
    glyphs = load_font(os.path.join(SRC, "font_data_8x16.in"))
    face8 = ImageFont.truetype(FACE, SIZE * 8)
    aa = bytearray()
    hi = bytearray()
    for index, rows in enumerate(glyphs):
        if 32 <= index < 127:
            ch = chr(index)
            cell, big = snapped(face8, ch)
            for row in cell:
                aa.extend(row)
            for row in big:
                for byte in range(8):
                    v = 0
                    for bit in range(8):
                        v = (v << 1) | (row[byte * 8 + bit] >= 128)
                    hi.append(v)
            continue
        b = bitmap(rows)
        x4 = scale2x(scale2x(b))  # 32 x 64
        for y in range(16):
            for x in range(8):
                n = sum(x4[y * 4 + dy][x * 4 + dx] for dy in range(4) for dx in range(4))
                aa.append((n * 255 + 8) // 16)
        x8 = scale2x(x4)  # 64 x 128
        for row in x8:
            for byte in range(8):
                v = 0
                for bit in range(8):
                    v = (v << 1) | row[byte * 8 + bit]
                hi.append(v)
    open(os.path.join(SRC, "font_aa_8x16.bin"), "wb").write(aa)
    open(os.path.join(SRC, "font_hi8_8x16.bin"), "wb").write(hi)
    print("wrote", len(aa), "and", len(hi), "bytes")


if __name__ == "__main__":
    main()
