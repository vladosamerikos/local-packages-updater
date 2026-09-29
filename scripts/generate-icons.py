#!/usr/bin/env python3
from pathlib import Path

from PIL import Image, ImageDraw


ROOT = Path(__file__).resolve().parents[1]
ICON_DIR = ROOT / "src-tauri" / "icons"


def app_icon(size: int) -> Image.Image:
    scale = 2
    side = size * scale
    image = Image.new("RGBA", (side, side), (0, 0, 0, 0))
    draw = ImageDraw.Draw(image)
    margin = round(side * 0.055)
    draw.rounded_rectangle(
        (margin, margin, side - margin, side - margin),
        radius=round(side * 0.23),
        fill="#1b242e",
        outline="#34404e",
        width=max(2, round(side * 0.012)),
    )
    blue = "#9bb8ff"
    line = max(8, round(side * 0.052))
    draw.line((side * 0.5, side * 0.235, side * 0.5, side * 0.60), fill=blue, width=line)
    draw.line((side * 0.355, side * 0.475, side * 0.5, side * 0.62, side * 0.645, side * 0.475), fill=blue, width=line, joint="curve")
    draw.line((side * 0.275, side * 0.70, side * 0.725, side * 0.70), fill=blue, width=line)
    draw.line((side * 0.32, side * 0.70, side * 0.36, side * 0.77, side * 0.64, side * 0.77, side * 0.68, side * 0.70), fill=blue, width=line, joint="curve")
    return image.resize((size, size), Image.Resampling.LANCZOS)


def tray_icon(pending: bool, size: int = 64) -> Image.Image:
    scale = 3
    side = size * scale
    image = Image.new("RGBA", (side, side), (0, 0, 0, 0))
    draw = ImageDraw.Draw(image)
    color = "#cbd7e5"
    line = round(side * 0.095)
    draw.line((side * 0.5, side * 0.18, side * 0.5, side * 0.65), fill=color, width=line)
    draw.line((side * 0.31, side * 0.48, side * 0.5, side * 0.67, side * 0.69, side * 0.48), fill=color, width=line, joint="curve")
    draw.line((side * 0.23, side * 0.82, side * 0.77, side * 0.82), fill=color, width=line)
    if pending:
        radius = round(side * 0.17)
        center = (round(side * 0.81), round(side * 0.2))
        draw.ellipse((center[0] - radius, center[1] - radius, center[0] + radius, center[1] + radius), fill="#f2b45e", outline="#111820", width=max(2, round(side * 0.05)))
    return image.resize((size, size), Image.Resampling.LANCZOS)


def main() -> None:
    ICON_DIR.mkdir(parents=True, exist_ok=True)
    app_icon(1024).save(ICON_DIR / "app-icon.png", optimize=True)
    for size in (32, 128, 256):
        app_icon(size).save(ICON_DIR / f"{size}x{size}.png", optimize=True)
    tray_icon(False).save(ICON_DIR / "tray-normal.png", optimize=True)
    tray_icon(True).save(ICON_DIR / "tray-pending.png", optimize=True)


if __name__ == "__main__":
    main()
