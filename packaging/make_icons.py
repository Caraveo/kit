"""Generate Kit's icon set and the example sprite (run: python3 packaging/make_icons.py)."""
from pathlib import Path
from PIL import Image, ImageDraw

ROOT = Path(__file__).resolve().parent.parent


# The Kit mark: four chevron blades around a diamond, on a deep purple tile.
# Coordinates are on a 1254-unit canvas (the master artwork).
MARK = (208, 168, 250)
TILE = (34, 17, 50)
CANVAS = 1254
TILE_BOX = (75, 82, 1178, 1182)
TILE_RADIUS = 150
# Upper-left blade; the others are mirror images around the center.
BLADE = [(612, 236), (497, 350), (497, 495), (349, 495), (234, 610), (557, 610), (612, 555)]
CENTER = (627, 626)


def blades():
    cx, cy = CENTER
    out = []
    for mx in (1, -1):
        for my in (1, -1):
            out.append([(cx + (x - cx) * mx, cy + (y - cy) * my) for x, y in BLADE])
    return out


def logo(size, background=True):
    s = size * 4  # supersample
    k = s / CANVAS
    img = Image.new("RGBA", (s, s), (0, 0, 0, 0))
    d = ImageDraw.Draw(img)
    if background:
        x0, y0, x1, y1 = (v * k for v in TILE_BOX)
        d.rounded_rectangle([x0, y0, x1, y1], radius=TILE_RADIUS * k, fill=TILE + (255,))
        scale, offset = k, 0.0
    else:
        # Mark only, filling the image.
        scale = s / (1020 - 234) * 0.96
        offset = s / 2 - CENTER[0] * scale
    for b in blades():
        d.polygon([(x * scale + offset, y * scale + offset) for x, y in b], fill=MARK + (255,))
    return img.resize((size, size), Image.LANCZOS)


def main():
    icons = ROOT / "packaging" / "icons"
    icons.mkdir(parents=True, exist_ok=True)
    big = logo(1024)
    for n in (16, 32, 48, 64, 128, 256, 512):
        big.resize((n, n), Image.LANCZOS).save(icons / f"kit-{n}.png")
    big.save(icons / "kit.png")
    big.save(icons / "kit.ico", sizes=[(16, 16), (24, 24), (32, 32), (48, 48), (64, 64), (128, 128), (256, 256)])
    big.save(icons / "kit.icns")
    logo(512, background=False).save(icons / "kit-mark.png")
    if "--examples" not in __import__("sys").argv:
        print("icons written to", icons)
        return
    logo(256, background=False).save(ROOT / "examples" / "hello" / "sprites" / "kit.png")
    # A small checker texture for models.
    t = Image.new("RGBA", (128, 128))
    td = ImageDraw.Draw(t)
    for y in range(8):
        for x in range(8):
            col = (230, 222, 255, 255) if (x + y) % 2 == 0 else (150, 128, 220, 255)
            td.rectangle([x * 16, y * 16, x * 16 + 15, y * 16 + 15], fill=col)
    t.save(ROOT / "examples" / "hello" / "sprites" / "checker.png")
    print("icons written to", icons)


if __name__ == "__main__":
    main()
