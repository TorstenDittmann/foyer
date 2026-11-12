"""Draws the Foyer app icon and writes every format the installers need.

    python3 crates/foyer-desktop/packaging/icon.py

Needs Pillow. Writes icon.png (1024 px), icon-512.png (Linux), icon.ico
(Windows, 16-256 px) and icon.icns (macOS) next to this script. The design matches the logo mark in
the client's sidebar: a light "F" on the Midnight canvas, with a coral dot
for the support colour.
"""

import io
import struct
from pathlib import Path

from PIL import Image, ImageDraw, ImageFont

HERE = Path(__file__).resolve().parent
FONT = HERE.parent / "assets" / "fonts" / "Onest-SemiBold.ttf"

CANVAS = (0x0B, 0x0F, 0x1A, 255)
RAISED = (0x1A, 0x22, 0x38, 255)
INK = (0xE8, 0xEC, 0xF6, 255)
CORAL = (0xFF, 0x6B, 0x5A, 255)


def draw(size: int = 1024) -> Image.Image:
    # Draw at 4x and scale down for smooth edges.
    s = size * 4
    img = Image.new("RGBA", (s, s), (0, 0, 0, 0))
    d = ImageDraw.Draw(img)
    # macOS-style grid: the tile fills ~80% of the canvas.
    inset = round(s * 0.098)
    radius = round(s * 0.18)
    box = (inset, inset, s - inset, s - inset)
    d.rounded_rectangle(box, radius=radius, fill=CANVAS, outline=RAISED, width=round(s * 0.008))

    font = ImageFont.truetype(str(FONT), round(s * 0.56))
    left, top, right, bottom = d.textbbox((0, 0), "F", font=font)
    w, h = right - left, bottom - top
    x = (s - w) / 2 - left - s * 0.035
    y = (s - h) / 2 - top
    d.text((x, y), "F", font=font, fill=INK)

    r = s * 0.052
    cx, cy = x + left + w + s * 0.075, y + top + h - r
    d.ellipse((cx - r, cy - r, cx + r, cy + r), fill=CORAL)
    return img.resize((size, size), Image.LANCZOS)


def png_bytes(img: Image.Image, size: int) -> bytes:
    buf = io.BytesIO()
    img.resize((size, size), Image.LANCZOS).save(buf, format="PNG")
    return buf.getvalue()


def write_icns(img: Image.Image, path: Path) -> None:
    # PNG-backed entries, supported since macOS 10.7.
    types = [
        (b"icp4", 16),
        (b"icp5", 32),
        (b"icp6", 64),
        (b"ic07", 128),
        (b"ic08", 256),
        (b"ic09", 512),
        (b"ic10", 1024),
        (b"ic11", 32),
        (b"ic12", 64),
        (b"ic13", 256),
        (b"ic14", 512),
    ]
    body = b"".join(
        kind + struct.pack(">I", 8 + len(data)) + data
        for kind, data in ((k, png_bytes(img, n)) for k, n in types)
    )
    path.write_bytes(b"icns" + struct.pack(">I", 8 + len(body)) + body)


def main() -> None:
    img = draw()
    img.save(HERE / "icon.png")
    img.resize((512, 512), Image.LANCZOS).save(HERE / "icon-512.png")
    img.save(HERE / "icon.ico", sizes=[(n, n) for n in (16, 24, 32, 48, 64, 128, 256)])
    write_icns(img, HERE / "icon.icns")


if __name__ == "__main__":
    main()
