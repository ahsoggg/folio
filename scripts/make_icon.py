"""Generates crates/app/assets/icon.ico, icon.png and icon64.rgba (the window icon) from code.

    python scripts/make_icon.py
"""
from pathlib import Path
from PIL import Image, ImageDraw

SS = 4  # supersampling factor
OUT = Path(__file__).resolve().parent.parent / "crates" / "app" / "assets"


def draw(size: int) -> Image.Image:
    s = size * SS
    img = Image.new("RGBA", (s, s), (0, 0, 0, 0))

    # rounded square with a diagonal violet-to-blue gradient
    grad = Image.new("RGBA", (s, s))
    px = grad.load()
    top, bottom = (203, 166, 247), (80, 120, 220)
    for y in range(s):
        for x in range(s):
            t = (x + y) / (2 * (s - 1))
            px[x, y] = tuple(int(top[i] + (bottom[i] - top[i]) * t) for i in range(3)) + (255,)
    mask = Image.new("L", (s, s), 0)
    ImageDraw.Draw(mask).rounded_rectangle([0, 0, s - 1, s - 1], radius=int(s * 0.22), fill=255)
    img.paste(grad, (0, 0), mask)

    d = ImageDraw.Draw(img)
    white = (255, 255, 255, 255)
    fold = (225, 220, 250, 255)
    ink = (110, 100, 190, 255)
    u = s / 100.0

    # page with a folded top-right corner
    d.polygon([(27 * u, 16 * u), (60 * u, 16 * u), (74 * u, 30 * u), (74 * u, 84 * u), (27 * u, 84 * u)], fill=white)
    d.polygon([(60 * u, 16 * u), (74 * u, 30 * u), (60 * u, 30 * u)], fill=fold)
    # lines of text
    for i, w in enumerate([34, 34, 34, 26]):
        y = (40 + i * 10) * u
        d.rounded_rectangle([34 * u, y, (34 + w) * u, y + 4.2 * u], radius=2 * u, fill=ink)
    # bookmark ribbon
    d.polygon([(56 * u, 66 * u), (68 * u, 66 * u), (68 * u, 92 * u), (62 * u, 86 * u), (56 * u, 92 * u)],
              fill=(243, 139, 168, 255))

    return img.resize((size, size), Image.LANCZOS)


def main() -> None:
    OUT.mkdir(parents=True, exist_ok=True)
    sizes = [16, 24, 32, 48, 64, 128, 256]
    imgs = [draw(n) for n in sizes]
    imgs[-1].save(OUT / "icon.png")
    imgs[-1].save(OUT / "icon.ico", sizes=[(n, n) for n in sizes], append_images=imgs[:-1])
    (OUT / "icon64.rgba").write_bytes(draw(64).tobytes())
    print("wrote", OUT)


if __name__ == "__main__":
    main()
