"""Export PandaGen's icons (GFX-094).

Each source in kernel_bootstrap/assets/icons/src/<name>.png is a square
picture that fills its canvas edge to edge (they were generated with an
image model from the prompts in PROMPTS.md). This masks each to a
superellipse -- the "squircle" every icon shares -- with an antialiased
edge, and writes straight-alpha RGBA at the sizes the desk draws:

    kernel_bootstrap/assets/icons/<name>_<size>.rgba   (size*size*4 bytes)

The kernel includes these bytes as they are; the compositor blends them.

    python3 tools/art/icons.py
"""
import os
from PIL import Image

ROOT = os.path.join(os.path.dirname(__file__), "..", "..")
ICONS = os.path.join(ROOT, "kernel_bootstrap", "assets", "icons")
NAMES = ["notepad", "files", "terminal", "look", "calculator", "calendar",
         "timer", "tiles", "tasks", "sketch", "panda"]
SIZES = [64, 40, 32, 20, 16]


def squircle_mask(size, n=5.0, ss=8):
    big = size * ss
    m = Image.new("L", (big, big), 0)
    px = m.load()
    r = big / 2.0
    for y in range(big):
        dy = abs((y + 0.5 - r) / r)
        rem = 1.0 - dy ** n
        if rem <= 0:
            continue
        dx = rem ** (1.0 / n) * r
        for x in range(max(0, int(round(r - dx))), min(big, int(round(r + dx)))):
            px[x, y] = 255
    return m.resize((size, size), Image.LANCZOS)


def export():
    for name in NAMES:
        src = Image.open(os.path.join(ICONS, "src", name + ".png")).convert("RGB")
        for size in SIZES:
            im = src.resize((size, size), Image.LANCZOS).convert("RGBA")
            im.putalpha(squircle_mask(size))
            with open(os.path.join(ICONS, f"{name}_{size}.rgba"), "wb") as f:
                f.write(im.tobytes())
    print("exported", len(NAMES) * len(SIZES), "icons")


if __name__ == "__main__":
    export()
