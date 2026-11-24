"""Generate Kit's icon set and the example sprite (run: python3 packaging/make_icons.py)."""
import math
from pathlib import Path
from PIL import Image, ImageDraw, ImageFilter

ROOT = Path(__file__).resolve().parent.parent
ACCENT = (193, 169, 255)
BG = (16, 16, 23)


def hexagon(cx, cy, r):
    return [(cx + r * math.cos(math.pi / 3 * i - math.pi / 2), cy + r * math.sin(math.pi / 3 * i - math.pi / 2)) for i in range(6)]


def logo(size, background=True):
    s = size * 4  # supersample
    img = Image.new("RGBA", (s, s), (0, 0, 0, 0))
    d = ImageDraw.Draw(img)
    if background:
        d.rounded_rectangle([0, 0, s - 1, s - 1], radius=s * 0.22, fill=BG + (255,))
        glow = Image.new("RGBA", (s, s), (0, 0, 0, 0))
        ImageDraw.Draw(glow).ellipse([s * 0.18, s * 0.12, s * 0.82, s * 0.76], fill=(64, 52, 91, 255))
        img = Image.alpha_composite(img, glow.filter(ImageFilter.GaussianBlur(s * 0.12)))
        mask = Image.new("L", (s, s), 0)
        ImageDraw.Draw(mask).rounded_rectangle([0, 0, s - 1, s - 1], radius=s * 0.22, fill=255)
        img.putalpha(mask)
        d = ImageDraw.Draw(img)
    c = (s / 2, s / 2)
    r = s * (0.33 if background else 0.46)
    h = hexagon(c[0], c[1], r)
    w = max(2, int(s * 0.035))
    d.polygon([h[0], h[1], c, h[5]], fill=ACCENT + (90,))
    d.polygon([h[1], h[2], h[3], c], fill=ACCENT + (40,))
    d.line(h + [h[0]], fill=ACCENT + (255,), width=w, joint="curve")
    for p in (h[1], h[5], h[3]):
        d.line([c, p], fill=ACCENT + (255,), width=w)
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
