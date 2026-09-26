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
         "timer", "tiles", "tasks", "sketch", "panda",
         # Drawn by tools/art/drawn_icons.py (GFX-107).
         "notices", "now", "shortcuts", "bin",
         # Drawn too (WEB-001).
         "web"]
SIZES = [64, 48, 40, 32, 20, 16]


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


THUMBS = {
    "picture": "wallpaper.png",
    "aurora": "wallpapers/aurora.jpg",
    "bamboo": "wallpapers/bamboo.jpg",
    "nebula": "wallpapers/nebula.jpg",
}
THUMB_SIZE = (100, 62)


def rounded_mask(size, radius, ss=4):
    from PIL import ImageDraw
    w, h = size
    m = Image.new("L", (w * ss, h * ss), 0)
    ImageDraw.Draw(m).rounded_rectangle((0, 0, w * ss - 1, h * ss - 1), radius * ss, fill=255)
    return m.resize(size, Image.LANCZOS)


def thumbnails():
    # Look's wallpaper thumbnails (GFX-100): each wallpaper cropped to
    # 16:10, 100x62, corners rounded.
    assets = os.path.join(ROOT, "kernel_bootstrap", "assets")
    for name, src in THUMBS.items():
        im = Image.open(os.path.join(assets, src)).convert("RGB")
        w, h = im.size
        th = w * 10 // 16
        top = max(0, (h - th) // 2)
        im = im.crop((0, top, w, min(h, top + th))).resize(THUMB_SIZE, Image.LANCZOS).convert("RGBA")
        im.putalpha(rounded_mask(THUMB_SIZE, 6))
        with open(os.path.join(ICONS, f"thumb_{name}.rgba"), "wb") as f:
            f.write(im.tobytes())
    print("exported", len(THUMBS), "thumbnails")


if __name__ == "__main__":
    export()
    thumbnails()
