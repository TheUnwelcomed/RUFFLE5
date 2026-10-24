"""A small SVG path rasteriser for the Ruffle mark (only what rs.ruffle.Ruffle.svg uses: M/m L/l H/h
V/v C/c Z/z, filled even-odd), supersampled with PIL. Used by make-art.py; no other dependencies."""
import re
from PIL import Image, ImageChops, ImageDraw

TOKEN = re.compile(r"[MmLlHhVvCcZz]|-?(?:\d+\.?\d*|\.\d+)(?:[eE][-+]?\d+)?")


def subpaths(d, steps=24):
    """The path's subpaths as point lists (curves flattened)."""
    toks = TOKEN.findall(d)
    i = 0
    out, cur = [], []
    x = y = sx = sy = 0.0
    cmd = None

    def num():
        nonlocal i
        v = float(toks[i])
        i += 1
        return v

    while i < len(toks):
        t = toks[i]
        if re.match(r"[A-Za-z]", t):
            cmd = t
            i += 1
            if cmd in "Zz":
                if cur:
                    out.append(cur)
                cur = []
                x, y = sx, sy
                continue
        rel = cmd.islower()
        c = cmd.upper()
        if c == "M":
            nx, ny = num(), num()
            if rel:
                nx += x
                ny += y
            if cur:
                out.append(cur)
            cur = [(nx, ny)]
            x, y, sx, sy = nx, ny, nx, ny
            cmd = "l" if rel else "L"  # implicit lineto after moveto
        elif c == "L":
            nx, ny = num(), num()
            if rel:
                nx += x
                ny += y
            cur.append((nx, ny))
            x, y = nx, ny
        elif c == "H":
            nx = num()
            if rel:
                nx += x
            cur.append((nx, y))
            x = nx
        elif c == "V":
            ny = num()
            if rel:
                ny += y
            cur.append((x, ny))
            y = ny
        elif c == "C":
            pts = [num() for _ in range(6)]
            if rel:
                pts = [pts[k] + (x if k % 2 == 0 else y) for k in range(6)]
            x1, y1, x2, y2, ex, ey = pts
            for s in range(1, steps + 1):
                u = s / steps
                a, b, cc, dd = (1 - u) ** 3, 3 * u * (1 - u) ** 2, 3 * u * u * (1 - u), u ** 3
                cur.append((a * x + b * x1 + cc * x2 + dd * ex, a * y + b * y1 + cc * y2 + dd * ey))
            x, y = ex, ey
        else:
            raise ValueError("unsupported command " + cmd)
    if cur:
        out.append(cur)
    return out


def mask(d, size, transform, ss=8):
    """An L-mode coverage mask `size` big; `transform` maps path units to output pixels."""
    W, H = size[0] * ss, size[1] * ss
    m = Image.new("1", (W, H), 0)
    for poly in subpaths(d):
        if len(poly) < 3:
            continue  # a bare moveto ("m 0 0" after a close)
        layer = Image.new("1", (W, H), 0)
        ImageDraw.Draw(layer).polygon([(px * ss, py * ss) for px, py in (transform(p) for p in poly)], fill=1)
        m = ImageChops.logical_xor(m, layer)  # even-odd
    return m.convert("L").resize(size, Image.LANCZOS)
