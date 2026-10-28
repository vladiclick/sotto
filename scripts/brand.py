#!/usr/bin/env python3
"""Generate Sotto's brand assets as SVG (rendered to PNG with `resvg`).

The mark: a decrescendo hairpin — music notation for "getting quieter",
i.e. sotto voce — that fades into a text cursor. Your voice drops to a
whisper and becomes text. Everything is plain geometry so it stays crisp at
16 px and editable by hand.

    python3 scripts/brand.py            # writes assets/brand/*.svg + *.png

Requires `resvg` (brew install resvg) for the PNG renders.
"""

import math
import shutil
import subprocess
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
OUT = ROOT / "assets" / "brand"
FONTS = ROOT / "ui" / "public" / "fonts"

INK_TOP = "#25211D"
INK_BOTTOM = "#0D0B0A"
PAPER = "#F3EFE7"
SIGNAL = "#FF5A1F"


def squircle(cx: float, cy: float, r: float, n: float = 5.0, steps: int = 720) -> str:
    """Superellipse |x|^n + |y|^n = r^n — the continuous-curvature tile shape
    macOS icons use (a rounded rect has visible corner kinks)."""
    pts = []
    for i in range(steps):
        t = 2 * math.pi * i / steps
        c, s = math.cos(t), math.sin(t)
        x = cx + r * math.copysign(abs(c) ** (2 / n), c)
        y = cy + r * math.copysign(abs(s) ** (2 / n), s)
        pts.append(f"{x:.2f},{y:.2f}")
    return "M" + " L".join(pts) + " Z"


def mark(cx: float, cy: float, scale: float, paper: str, signal: str) -> str:
    """Hairpin + caret centred on (cx, cy). `scale` 1.0 = icon size."""
    w = 44 * scale  # caret weight: the cursor is the bold, focal stroke
    hair = 26 * scale  # hairpin weight: a hairline next to the cursor
    x0, x1 = cx - 251 * scale, cx + 164 * scale  # hairpin mouth → tip
    half = 68 * scale  # half-opening: ~6:1, the proportion of a real hairpin
    caret_x = cx + 242 * scale
    caret_h = 300 * scale
    return (
        f'<path d="M{x0:.2f} {cy - half:.2f} L{x1:.2f} {cy:.2f} L{x0:.2f} {cy + half:.2f}" '
        f'fill="none" stroke="{paper}" stroke-width="{hair:.2f}" '
        f'stroke-linecap="round" stroke-linejoin="round"/>'
        f'<rect x="{caret_x - w / 2:.2f}" y="{cy - caret_h / 2:.2f}" width="{w:.2f}" '
        f'height="{caret_h:.2f}" rx="{w / 2:.2f}" fill="{signal}"/>'
    )


def staff(x0: float, x1: float, cy: float, gap: float, width: float, opacity: float) -> str:
    """Five-line music staff, very faint."""
    lines = []
    for k in range(-2, 3):
        y = cy + k * gap
        lines.append(
            f'<line x1="{x0:.1f}" y1="{y:.1f}" x2="{x1:.1f}" y2="{y:.1f}" '
            f'stroke="{PAPER}" stroke-opacity="{opacity}" stroke-width="{width}"/>'
        )
    return "".join(lines)


def icon_svg() -> str:
    """1024×1024 app icon on Apple's grid: 824 px tile, 100 px margins."""
    tile = squircle(512, 512, 412)
    return f"""<svg xmlns="http://www.w3.org/2000/svg" width="1024" height="1024" viewBox="0 0 1024 1024">
  <defs>
    <linearGradient id="ink" x1="0" y1="0" x2="0" y2="1">
      <stop offset="0" stop-color="{INK_TOP}"/>
      <stop offset="1" stop-color="{INK_BOTTOM}"/>
    </linearGradient>
    <radialGradient id="glow" cx="0.5" cy="0.08" r="0.75">
      <stop offset="0" stop-color="#FFFFFF" stop-opacity="0.07"/>
      <stop offset="1" stop-color="#FFFFFF" stop-opacity="0"/>
    </radialGradient>
    <clipPath id="tile"><path d="{tile}"/></clipPath>
    <filter id="shadow" x="-20%" y="-20%" width="140%" height="140%">
      <feGaussianBlur stdDeviation="14"/>
    </filter>
  </defs>
  <path d="{tile}" transform="translate(0 10)" fill="#000" fill-opacity="0.32" filter="url(#shadow)"/>
  <path d="{tile}" fill="url(#ink)"/>
  <path d="{tile}" fill="url(#glow)"/>
  <g clip-path="url(#tile)">{staff(100, 924, 512, 46, 3, 0.055)}</g>
  <path d="{tile}" fill="none" stroke="#FFFFFF" stroke-opacity="0.08" stroke-width="3" clip-path="url(#tile)"/>
  {mark(512, 512, 1.0, PAPER, SIGNAL)}
</svg>
"""


def mark_svg(paper: str, signal: str) -> str:
    """Stand-alone mark (no tile) for docs and the menu bar reference."""
    return f"""<svg xmlns="http://www.w3.org/2000/svg" width="560" height="320" viewBox="0 0 560 320">
  {mark(280, 160, 1.0, paper, signal)}
</svg>
"""


def banner_svg() -> str:
    """1280×640 GitHub social preview / README hero."""
    return f"""<svg xmlns="http://www.w3.org/2000/svg" width="1280" height="640" viewBox="0 0 1280 640">
  <defs>
    <linearGradient id="bg" x1="0" y1="0" x2="0" y2="1">
      <stop offset="0" stop-color="#1B1815"/>
      <stop offset="1" stop-color="#0D0B0A"/>
    </linearGradient>
  </defs>
  <rect width="1280" height="640" fill="url(#bg)"/>
  {staff(0, 1280, 300, 38, 2, 0.06)}
  {mark(330, 300, 0.62, PAPER, SIGNAL)}
  <text x="560" y="352" font-family="Instrument Serif" font-style="italic" font-size="196"
        fill="{PAPER}" letter-spacing="-2">sotto</text>
  <text x="566" y="420" font-family="IBM Plex Mono" font-size="21" fill="{PAPER}"
        fill-opacity="0.62" letter-spacing="3.2">HOLD A KEY · SPEAK · IT TYPES</text>
  <text x="96" y="566" font-family="IBM Plex Mono" font-size="17" fill="{PAPER}"
        fill-opacity="0.42" letter-spacing="2.4">PRIVATE ON-DEVICE DICTATION FOR macOS — WHISPER ON METAL — 100 LANGUAGES</text>
  <rect x="96" y="586" width="44" height="4" rx="2" fill="{SIGNAL}"/>
</svg>
"""


def render(svg: Path, png: Path, width: int | None = None) -> None:
    cmd = ["resvg", "--use-fonts-dir", str(FONTS), str(svg), str(png)]
    if width:
        cmd[1:1] = ["--width", str(width)]
    subprocess.run(cmd, check=True)


def main() -> None:
    OUT.mkdir(parents=True, exist_ok=True)
    files = {
        "sotto-icon.svg": icon_svg(),
        "sotto-mark-dark.svg": mark_svg(PAPER, SIGNAL),  # for dark backgrounds
        "sotto-mark-light.svg": mark_svg("#1A1714", SIGNAL),  # for light backgrounds
        "sotto-banner.svg": banner_svg(),
    }
    for name, svg in files.items():
        (OUT / name).write_text(svg)
    if shutil.which("resvg") is None:
        raise SystemExit("resvg not found — brew install resvg")
    render(OUT / "sotto-icon.svg", OUT / "sotto-icon-1024.png")
    render(OUT / "sotto-banner.svg", OUT / "sotto-banner.png")
    print(f"wrote {len(files)} SVGs + PNG renders to {OUT.relative_to(ROOT)}")


if __name__ == "__main__":
    main()
