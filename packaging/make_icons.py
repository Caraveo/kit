"""Generate Kit's icon set and the example sprite (run: python3 packaging/make_icons.py)."""
import sys
from pathlib import Path
from PIL import Image, ImageDraw

ROOT = Path(__file__).resolve().parent.parent


def main():
    icons = ROOT / "packaging" / "icons"
    icons.mkdir(parents=True, exist_ok=True)
    with Image.open(ROOT / "docs" / "kit-logo.png") as artwork:
        master = artwork.convert("RGBA")
    big = master.resize((1024, 1024), Image.LANCZOS)
    for n in (16, 32, 48, 64, 128, 256, 512):
        master.resize((n, n), Image.LANCZOS).save(icons / f"kit-{n}.png")
    big.save(icons / "kit.png")
    big.save(icons / "kit.ico", sizes=[(16, 16), (24, 24), (32, 32), (48, 48), (64, 64), (128, 128), (256, 256)])
    big.save(icons / "kit.icns")
    for path, size in (
        ("documentation/public/favicon.png", 64),
        ("documentation/src/assets/icon.png", 256),
        ("site/dist/assets/icon.png", 256),
    ):
        master.resize((size, size), Image.LANCZOS).save(ROOT / path)

    # The standalone mark uses the same artwork with the dark tile removed.
    red = master.getchannel("R")
    mask = red.point(lambda value: max(0, min(255, round((value - 60) * 255 / 125))))
    mark = master.copy()
    mark.putalpha(mask)
    mark = mark.crop(mask.getbbox())
    mark = mark.resize((492, 492), Image.LANCZOS)
    tile = Image.new("RGBA", (512, 512))
    tile.paste(mark, (10, 10))
    tile.save(icons / "kit-mark.png")

    if "--examples" not in sys.argv:
        print("icons written to", icons)
        return
    tile.resize((256, 256), Image.LANCZOS).save(ROOT / "examples" / "hello" / "sprites" / "kit.png")
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
