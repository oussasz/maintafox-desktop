#!/usr/bin/env python3
"""Generate the Windows installer / uninstaller artwork for Maintafox.

Outputs (all deterministic, regenerate any time with `pnpm installer:art`):

  src-tauri/windows/nsis/sidebar.bmp   164x314  welcome / finish pages (installer + uninstaller)
  src-tauri/windows/nsis/header.bmp    150x57   top strip of every other wizard page
  docs/engineering/installer-preview/  PNG previews of the two bitmaps (for review only)

NSIS (Modern UI 2) only reads 24-bit BMP files, so the artwork is composed at
SCALE x the final size and reduced with LANCZOS so that text and edges are
anti-aliased. Brand colours and text live in the constants below.

Requires: Pillow (pip install pillow). Uses Segoe UI from C:\\Windows\\Fonts when
available and falls back to other common sans-serif fonts, then to Pillow's
bundled font (the fallback is logged so the output is never silently degraded).
"""

from __future__ import annotations

import sys
from pathlib import Path

from PIL import Image, ImageDraw, ImageFont

ROOT = Path(__file__).resolve().parent.parent
LOGO_COLOR = ROOT / "src" / "assets" / "logo" / "maintafox-logo-color.png"
LOGO_WHITE = ROOT / "src" / "assets" / "logo" / "maintafox-logo-white.png"
OUT_DIR = ROOT / "src-tauri" / "windows" / "nsis"
PREVIEW_DIR = ROOT / "docs" / "engineering" / "installer-preview"

# ---- Brand palette (keep in sync with src-tauri/windows/nsis/branding.nsh) ----
NAVY = (0x0B, 0x1F, 0x3A)
BRAND_BLUE = (0x00, 0x49, 0xAC)
ICY_BLUE = (0xE4, 0xEE, 0xFB)
WHITE = (0xFF, 0xFF, 0xFF)
ORANGE = (0xF6, 0x87, 0x14)

# ---- Text -----------------------------------------------------------------
WORDMARK = "Maintafox"
TAGLINE = "GMAO / CMMS industrielle"

# ---- Geometry (final pixels, as required by NSIS Modern UI) ---------------
SIDEBAR_SIZE = (164, 314)
HEADER_SIZE = (150, 57)
SCALE = 4  # supersampling factor

FONT_CANDIDATES = {
    "semibold": ["seguisb.ttf", "segoeuib.ttf", "arialbd.ttf", "DejaVuSans-Bold.ttf"],
    "regular": ["segoeui.ttf", "arial.ttf", "DejaVuSans.ttf"],
}
FONT_DIRS = [Path("C:/Windows/Fonts"), Path("/usr/share/fonts/truetype/dejavu")]


def load_font(kind: str, size_px: float) -> ImageFont.FreeTypeFont | ImageFont.ImageFont:
    size = max(1, round(size_px * SCALE))
    for name in FONT_CANDIDATES[kind]:
        for directory in FONT_DIRS:
            path = directory / name
            if path.is_file():
                if name != FONT_CANDIDATES[kind][0]:
                    print(f"warning: preferred font missing, using {name}", file=sys.stderr)
                return ImageFont.truetype(str(path), size)
    print("warning: no TrueType font found, using Pillow default font", file=sys.stderr)
    return ImageFont.load_default(size)


def load_logo(path: Path) -> Image.Image:
    """Logo cropped to its visible bounds."""
    logo = Image.open(path).convert("RGBA")
    bbox = logo.getchannel("A").getbbox()
    if bbox is None:
        raise SystemExit(f"logo {path} is fully transparent")
    return logo.crop(bbox)


def fit_logo(logo: Image.Image, max_w: float, max_h: float) -> Image.Image:
    """Resize to fit a box given in final pixels (rendered at SCALE)."""
    ratio = min(max_w * SCALE / logo.width, max_h * SCALE / logo.height)
    return logo.resize((round(logo.width * ratio), round(logo.height * ratio)), Image.LANCZOS)


def tint_white(logo: Image.Image) -> Image.Image:
    """Pure-white silhouette from the logo alpha (the shipped white PNG is off-white)."""
    solid = Image.new("RGBA", logo.size, WHITE + (255,))
    solid.putalpha(logo.getchannel("A"))
    return solid


def vertical_gradient(size: tuple[int, int], top: tuple, bottom: tuple) -> Image.Image:
    w, h = size
    img = Image.new("RGB", size)
    px = img.load()
    for y in range(h):
        t = y / (h - 1)
        # ease so that the navy dominates the upper two thirds
        t = t * t * (3 - 2 * t)
        row = tuple(round(top[i] + (bottom[i] - top[i]) * t) for i in range(3))
        for x in range(w):
            px[x, y] = row
    return img


def text_width(draw: ImageDraw.ImageDraw, text: str, font) -> float:
    return draw.textlength(text, font=font)


def centered_text(draw, cx, y, text, font, fill):
    w = text_width(draw, text, font)
    draw.text((cx - w / 2, y), text, font=font, fill=fill)


def build_sidebar() -> Image.Image:
    w, h = (SIDEBAR_SIZE[0] * SCALE, SIDEBAR_SIZE[1] * SCALE)
    img = vertical_gradient((w, h), NAVY, BRAND_BLUE)

    # Soft, low-contrast gear-like ring in the lower area for depth (restrained).
    overlay = Image.new("RGBA", (w, h), (0, 0, 0, 0))
    od = ImageDraw.Draw(overlay)
    cx, cy, r = round(w * 0.88), round(h * 0.93), round(w * 0.78)
    od.ellipse((cx - r, cy - r, cx + r, cy + r), outline=WHITE + (22,), width=SCALE * 2)
    r2 = round(r * 0.66)
    od.ellipse((cx - r2, cy - r2, cx + r2, cy + r2), outline=WHITE + (14,), width=SCALE * 2)
    img = Image.alpha_composite(img.convert("RGBA"), overlay)

    draw = ImageDraw.Draw(img)
    mid = w / 2

    logo = tint_white(fit_logo(load_logo(LOGO_WHITE), 96, 96))
    logo_y = 52 * SCALE
    img.alpha_composite(logo, (round(mid - logo.width / 2), logo_y))

    wordmark_font = load_font("semibold", 23)
    y_word = logo_y + logo.height + 18 * SCALE
    centered_text(draw, mid, y_word, WORDMARK, wordmark_font, WHITE)

    # Orange accent rule under the wordmark.
    rule_y = y_word + 36 * SCALE
    rule_w = 26 * SCALE
    draw.rounded_rectangle(
        (mid - rule_w / 2, rule_y, mid + rule_w / 2, rule_y + 3 * SCALE),
        radius=SCALE,
        fill=ORANGE,
    )

    tagline_font = load_font("regular", 10)
    centered_text(draw, mid, rule_y + 13 * SCALE, TAGLINE, tagline_font, ICY_BLUE)

    return img.convert("RGB").resize(SIDEBAR_SIZE, Image.LANCZOS)


def build_header() -> Image.Image:
    w, h = (HEADER_SIZE[0] * SCALE, HEADER_SIZE[1] * SCALE)
    # White matches MUI_BGCOLOR so the bitmap blends into the header band.
    img = Image.new("RGBA", (w, h), WHITE + (255,))
    draw = ImageDraw.Draw(img)

    logo = fit_logo(load_logo(LOGO_COLOR), 38, 38)
    font = load_font("semibold", 15)
    gap = 7 * SCALE
    text_w = text_width(draw, WORDMARK, font)
    total = logo.width + gap + text_w
    x0 = (w - total) / 2
    img.alpha_composite(logo, (round(x0), round((h - logo.height) / 2)))
    ascent, descent = font.getmetrics()
    ty = (h - (ascent + descent)) / 2 + 1 * SCALE
    draw.text((x0 + logo.width + gap, ty), WORDMARK, font=font, fill=NAVY)

    return img.convert("RGB").resize(HEADER_SIZE, Image.LANCZOS)


def save_bmp24(img: Image.Image, path: Path) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    # Pillow writes 24-bit BMP for RGB images, which is what NSIS requires.
    img.convert("RGB").save(path, format="BMP")


def main() -> None:
    for logo in (LOGO_COLOR, LOGO_WHITE):
        if not logo.is_file():
            raise SystemExit(f"missing logo asset: {logo}")

    sidebar, header = build_sidebar(), build_header()
    save_bmp24(sidebar, OUT_DIR / "sidebar.bmp")
    save_bmp24(header, OUT_DIR / "header.bmp")

    PREVIEW_DIR.mkdir(parents=True, exist_ok=True)
    sidebar.save(PREVIEW_DIR / "sidebar.png")
    header.save(PREVIEW_DIR / "header.png")
    print(f"wrote {OUT_DIR / 'sidebar.bmp'} {sidebar.size}")
    print(f"wrote {OUT_DIR / 'header.bmp'} {header.size}")


if __name__ == "__main__":
    main()
