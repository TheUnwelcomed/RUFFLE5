#!/usr/bin/env python3
"""make-art.py — Ruffle5's brand pictures, drawn from Ruffle's own vector mark.

The mark is the "r" of Ruffle's app icon (ruffle-rs/ruffle desktop/packages/linux/rs.ruffle.Ruffle.svg,
MIT / Apache-2.0 — the same drawing as ruffle.rs's favicon), copied below as its two path strings.
Writes, beside the repository's other assets:
  assets/ruffle-mark.png   the mark alone, transparent (the menu's top bar, next to RUFFLE5)
  sce_sys/icon0.png        the launch icon, 512x512: the mark on Ruffle blue with a "5" in the
                           mark's own ribbon style, tucked into the r's open corner
Run: python3 art/make-art.py   (Pillow only)
"""
import math
import os
import sys

from PIL import Image, ImageDraw, ImageFilter

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
from svgpath import mask  # noqa: E402

ROOT = os.path.dirname(HERE)

# Ruffle's colours (ruffle.rs src/theme.ts: "this is ruffle orange", "this is ruffle blue").
ORANGE = (0xFF, 0xAD, 0x33)
FOLD = (0x96, 0x62, 0x14)        # the mark's own fold shade
BLUE = (0x37, 0x52, 0x8C)
BLUE_LIGHT = (0x52, 0x71, 0xB6)  # ruffle-blue-6
BLUE_DEEP = (0x2C, 0x49, 0x80)   # ruffle-blue-9
CREAM = (0xFF, 0xF6, 0xE0)       # ruffle-orange-0

# rs.ruffle.Ruffle.svg (128x128 units): the r and its fold.
R_PATH = "m 53.191406 62.109375 l -5.726562 40.652344 h -18.605469 l 6.699219 -47.636719 l 0.34375 -1.085938 l 0.113281 -0.117187 l 0.171875 -0.457031 l 0.285156 -0.457032 l 0.746094 -0.972656 l 0.972656 -0.917968 l 1.085938 -0.800782 l 1.089844 -0.515625 l 1.144531 -0.402343 l 1.144531 -0.171876 h 10.878906 l -6.699218 -1.316406 v -0.34375 h -0.058594 l 1.890625 -13.738281 l 0.859375 -2.519531 l 1.03125 -1.660156 l 1.832031 -1.832032 c 1.144531 -0.957031 2.347656 -1.640625 3.605469 -2.0625 l 2.976562 -0.515625 h 31.777344 c 1.945312 0 3.664062 0.535157 5.152344 1.605469 l 0.515625 0.398438 l 0.628906 0.574218 l 0.859375 0.914063 l 0.800781 1.375 c 0.421875 0.992187 0.535157 2.136719 0.34375 3.433593 l -0.914062 6.527344 h 0.058593 l -2.636718 18.609375 h -18.894532 l 2.347657 -16.546875 h -12.765625 l -2.003906 14.085938 l -0.34375 1.199218 l -0.171876 0.117188 l -0.285156 0.285156 l -0.117187 0.230469 l -0.285157 0.113281 l -0.914062 0.058594 l -1.03125 -0.574219 l -0.632812 -0.34375 l -0.683594 -0.6875 l -4.867188 -7.386719 z"
FOLD_PATH = "m 63.898438 57.414062 l -0.859376 1.375 l -1.085937 1.144532 l -1.203125 0.917968 c -0.613281 0.382813 -1.242188 0.667969 -1.890625 0.859376 l -1.71875 0.398437 h -3.949219 l 1.71875 -12.882813 l 4.867188 7.386719 l 0.683594 0.6875 l 0.632812 0.34375 l 1.03125 0.574219 l 0.914062 -0.058594 l 0.285157 -0.113281 l 0.117187 -0.230469 l 0.285156 -0.285156 z"
# The mark's extent in those units (measured from the paths), and its slant: its stems lean
# 6.70 units left over 47.64 down.
MARK_BOX = (28.86, 24.83, 99.37, 102.77)
SLANT = 6.699219 / 47.636719


def paint(img, path_or_mask, colour, transform=None):
    m = mask(path_or_mask, img.size, transform) if isinstance(path_or_mask, str) else path_or_mask
    layer = Image.new("RGBA", img.size, colour + (255,))
    layer.putalpha(m)
    img.alpha_composite(layer)


def mark_transform(scale, ox, oy):
    return lambda p: ((p[0] - MARK_BOX[0]) * scale + ox, (p[1] - MARK_BOX[1]) * scale + oy)


def five_mask(size, transform, ss=8):
    """A "5" in the mark's construction: flat ribbon strokes as thick as the r's stem (scaled), the
    same slant, a rounded outer corner where the r has its round one, and a fold where the stroke
    turns behind itself. Built in an upright box (u right, v down), then leaned like the r."""
    W, H = size[0] * ss, size[1] * ss
    m = Image.new("L", (W, H), 0)
    d = ImageDraw.Draw(m)
    # Upright design box: 30 wide, 40 tall, stroke 8.6 (the r's stem is 18.6 of its 78 height).
    w, h, t = 30.0, 40.0, 8.6
    vb = 0.0  # lean about the bottom

    def lean(u, v):
        return (u + (h - v) * SLANT, v)

    def poly(pts):
        out = []
        for u, v in pts:
            x, y = transform(lean(u, v))
            out.append((x * ss, y * ss))
        d.polygon(out, fill=255)

    def rounded_corner(cu, cv, r, a0, a1, inner=None):
        # a filled quarter-ring / quarter-disc, leaned
        pts = []
        for i in range(25):
            a = math.radians(a0 + (a1 - a0) * i / 24)
            pts.append((cu + r * math.cos(a), cv + r * math.sin(a)))
        if inner is None:
            pts.append((cu, cv))
        else:
            for i in range(24, -1, -1):
                a = math.radians(a0 + (a1 - a0) * i / 24)
                pts.append((cu + inner * math.cos(a), cv + inner * math.sin(a)))
        poly(pts)

    mid = 16.5          # where the middle stroke starts
    r = 7.0             # outer radius of the bowl's corners
    # top bar and the left stem down to the middle
    poly([(0, 0), (w, 0), (w, t), (t, t), (t, mid), (0, mid)])
    # the middle stroke, its right end rounded (outer radius r)
    poly([(0, mid), (w - r, mid), (w - r, mid + t), (0, mid + t)])
    rounded_corner(w - r, mid + r, r, 270, 360, inner=r - t if r > t else None)
    # the bowl's right side, down to the bottom corner
    poly([(w - t, mid + r), (w, mid + r), (w, h - r), (w - t, h - r)])
    rounded_corner(w - r, h - r, r, 0, 90, inner=r - t if r > t else None)
    # the bottom stroke
    poly([(0, h - t), (w - r, h - t), (w - r, h), (0, h)])
    # fill the inner corners the quarter-discs leave open when r <= t
    poly([(w - r, mid), (w - t + 0.01, mid), (w - t + 0.01, mid + t), (w - r, mid + t)])
    return m.resize(size, Image.LANCZOS), lean


def five_fold(size, transform, lean, ss=8):
    """The fold, as the r has one: where the top stroke turns down into the left stem, a shaded
    curl hangs in the inner corner, as if the ribbon bent there."""
    W, H = size[0] * ss, size[1] * ss
    m = Image.new("L", (W, H), 0)
    d = ImageDraw.Draw(m)
    t = 8.6
    k = 6.4  # the curl's size
    pts = [(t, t), (t + k, t)]
    for i in range(1, 17):
        a = math.radians(90 * i / 16)
        # a concave curve from (t+k, t) back to (t, t+k), bowing toward the corner
        pts.append((t + k - k * math.sin(a) * 0.98, t + k * (1 - math.cos(a)) * 0.98 + (k * 0.02 if i == 16 else 0)))
    pts.append((t, t + k))
    out = []
    for u, v in pts:
        x, y = transform(lean(u, v))
        out.append((x * ss, y * ss))
    d.polygon(out, fill=255)
    return m.resize(size, Image.LANCZOS)


def make_mark(path):
    scale = 5.0
    w = int(math.ceil((MARK_BOX[2] - MARK_BOX[0]) * scale)) + 8
    h = int(math.ceil((MARK_BOX[3] - MARK_BOX[1]) * scale)) + 8
    img = Image.new("RGBA", (w, h), (0, 0, 0, 0))
    tf = mark_transform(scale, 4, 4)
    paint(img, R_PATH, ORANGE, tf)
    paint(img, FOLD_PATH, FOLD, tf)
    img.save(path, optimize=True)
    return img


def make_icon(path, size=512):
    S = size
    img = Image.new("RGBA", (S, S), BLUE + (255,))
    # Ruffle blue, lit from the upper left, deepening toward the lower right.
    px = img.load()
    for y in range(S):
        for x in range(S):
            t = min(1.0, math.hypot(x - S * 0.28, y - S * 0.22) / (S * 1.05))
            t = t * t * (3 - 2 * t)
            c = tuple(int(BLUE_LIGHT[i] + (BLUE_DEEP[i] - BLUE_LIGHT[i]) * t) for i in range(3))
            px[x, y] = c + (255,)
    # Two faint ribbons in the mark's slant behind it.
    glow = Image.new("RGBA", (S, S), (0, 0, 0, 0))
    gd = ImageDraw.Draw(glow)
    for off, a in ((-0.10, 18), (0.30, 10)):
        x0 = S * (0.55 + off)
        gd.polygon([(x0, -10), (x0 + S * 0.22, -10), (x0 + S * 0.22 - (S + 20) * SLANT, S + 10), (x0 - (S + 20) * SLANT, S + 10)], fill=(255, 255, 255, a))
    glow = glow.filter(ImageFilter.GaussianBlur(S * 0.02))
    img.alpha_composite(glow)

    # The mark, large, a little left of centre.
    scale = S * 0.80 / (MARK_BOX[3] - MARK_BOX[1])
    mw = (MARK_BOX[2] - MARK_BOX[0]) * scale
    ox = (S - mw) / 2 - S * 0.035
    oy = S * 0.10
    tf = mark_transform(scale, ox, oy)
    # a soft shadow under the mark
    sh = Image.new("RGBA", (S, S), (0, 0, 0, 0))
    shm = mask(R_PATH, (S, S), mark_transform(scale, ox + S * 0.012, oy + S * 0.02))
    shl = Image.new("RGBA", (S, S), (10, 20, 45, 120))
    shl.putalpha(shm.point(lambda v: v * 120 // 255))
    sh.alpha_composite(shl)
    img.alpha_composite(sh.filter(ImageFilter.GaussianBlur(S * 0.018)))
    paint(img, R_PATH, ORANGE, tf)
    paint(img, FOLD_PATH, FOLD, tf)

    # The 5: in the r's open lower-right corner, under its right leg.
    # Mark units: the leg ends at y 62.6; the corner spans x ~57..99.
    five_scale = scale * 0.92
    fu0, fv0 = 65.0, 67.5  # top-left of the 5's upright box, in mark units
    def ftf(p):
        u, v = p
        return (ox + (fu0 - MARK_BOX[0]) * scale + u * five_scale,
                oy + (fv0 - MARK_BOX[1]) * scale + v * five_scale)
    fm, lean = five_mask((S, S), ftf)
    sh2 = Image.new("RGBA", (S, S), (10, 20, 45, 0))
    sh2.putalpha(fm.point(lambda v: v * 110 // 255))
    img.alpha_composite(sh2.filter(ImageFilter.GaussianBlur(S * 0.014)).transform((S, S), Image.AFFINE, (1, 0, -S * 0.01, 0, 1, -S * 0.016)))
    paint(img, fm, CREAM)
    paint(img, five_fold((S, S), ftf, lean), (0xD8, 0xBE, 0x8C))

    # Round the corners as the system does anyway (the console masks the tile).
    img.save(path, optimize=True)
    return img


if __name__ == "__main__":
    os.makedirs(os.path.join(ROOT, "assets"), exist_ok=True)
    make_mark(os.path.join(ROOT, "assets", "ruffle-mark.png"))
    out = sys.argv[1] if len(sys.argv) > 1 else os.path.join(ROOT, "sce_sys", "icon0.png")
    make_icon(out)
    print("wrote assets/ruffle-mark.png and", out)
