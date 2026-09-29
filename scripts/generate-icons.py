#!/usr/bin/env python3
"""Export the generated launcher master and draw the symbolic tray icons."""
from pathlib import Path
import math
from PIL import Image, ImageDraw

ROOT = Path(__file__).resolve().parents[1]
ICON_DIR = ROOT / "src-tauri" / "icons"
SIZES = (16, 24, 32, 48, 64, 128, 256, 512, 1024)


def tray_icon(pending: bool, size: int = 64) -> Image.Image:
    side = size * 4
    image = Image.new("RGBA", (side, side), (0, 0, 0, 0))
    draw = ImageDraw.Draw(image)
    color = "#dce5f0"
    center = side / 2
    radius = side * .30
    width = round(side * .085)
    for start in (35, 215):
        end = start + 145
        points = [
            (center + radius * math.cos(math.radians(angle)),
             center + radius * math.sin(math.radians(angle)))
            for angle in range(start, end + 1)
        ]
        draw.line(points, fill=color, width=width, joint="curve")
        angle = math.radians(end)
        x, y = points[-1]
        tx, ty = -math.sin(angle), math.cos(angle)
        nx, ny = math.cos(angle), math.sin(angle)
        length = side * .14
        draw.polygon([
            (x + tx * length * .45, y + ty * length * .45),
            (x - tx * length + nx * length * .7, y - ty * length + ny * length * .7),
            (x - tx * length - nx * length * .7, y - ty * length - ny * length * .7),
        ], fill=color)
    if pending:
        radius = round(side * .14)
        x, y = round(side * .79), round(side * .20)
        draw.ellipse((x - radius, y - radius, x + radius, y + radius),
                     fill="#f2b45e", outline="#1a202a", width=round(side * .035))
    return image.resize((size, size), Image.Resampling.LANCZOS)


def main() -> None:
    master = Image.open(ICON_DIR / "app-icon-master.png").convert("RGBA")
    if master.width != master.height:
        raise ValueError("The launcher master must be square")
    for size in SIZES:
        icon = master.resize((size, size), Image.Resampling.LANCZOS)
        icon.save(ICON_DIR / f"{size}x{size}.png", optimize=True)
        if size == 1024:
            icon.save(ICON_DIR / "app-icon.png", optimize=True)
    tray_icon(False).save(ICON_DIR / "tray-normal.png", optimize=True)
    tray_icon(True).save(ICON_DIR / "tray-pending.png", optimize=True)


if __name__ == "__main__":
    main()
