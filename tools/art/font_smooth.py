"""Bake the smooth glyph tables from the 8x16 bitmap font (GFX-095).

The desk's font is the 8x16 bitmap the text console has always used. To
draw it smoothly, each glyph is enlarged with Scale2x -- an edge-directed
pixel-art upscaler that keeps straight stems straight and turns
staircases into diagonals and corners into curves -- and then reduced
again by area. Two tables come out, next to the font data:

- font_aa_8x16.bin: 128 glyphs x 8x16 alpha bytes, from the 4x enlargement
  boxed back to 1x. Straight stems stay fully opaque; steps get soft edges.
- font_hi8_8x16.bin: 128 glyphs x 64x128 bits (8 bytes a row), the 8x
  enlargement itself, which scaled text samples for its coverage.

    python3 tools/art/font_smooth.py
"""
import os, re

HERE = os.path.dirname(__file__)
SRC = os.path.join(HERE, "..", "..", "graphics_rasterizer", "src")


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
    aa = bytearray()
    hi = bytearray()
    for rows in glyphs:
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
