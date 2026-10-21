#!/usr/bin/env python3
"""PS5_RUFFLE_TESTSWF_V1 — write a small, original, animated SWF for phase 3b: two squares on a
dark stage, one sliding, one spinning, 90 frames at 30 fps, looping. Pure SWF 6 tags (DefineShape,
PlaceObject2, ShowFrame), no ActionScript — the first thing a working renderer should show.

    python3 tools/make-test-swf.py test-swf/bounce.swf

SPDX-License-Identifier: GPL-3.0-or-later
"""
import math
import struct
import sys

TWIP = 20


class Bits:
    """MSB-first bit writer (SWF bit fields)."""

    def __init__(self):
        self.out = bytearray()
        self.acc = 0
        self.n = 0

    def ub(self, value, nbits):
        assert 0 <= value < (1 << nbits), (value, nbits)
        for i in range(nbits - 1, -1, -1):
            self.acc = (self.acc << 1) | ((value >> i) & 1)
            self.n += 1
            if self.n == 8:
                self.out.append(self.acc)
                self.acc = 0
                self.n = 0

    def sb(self, value, nbits):
        assert -(1 << (nbits - 1)) <= value < (1 << (nbits - 1)), (value, nbits)
        self.ub(value & ((1 << nbits) - 1), nbits)

    def align(self):
        if self.n:
            self.out.append(self.acc << (8 - self.n))
            self.acc = 0
            self.n = 0
        return bytes(self.out)


def sbits(*values):
    """Minimal signed bit width that holds every value."""
    n = 1
    for v in values:
        while not (-(1 << (n - 1)) <= v < (1 << (n - 1))):
            n += 1
    return n


def rect(xmin, xmax, ymin, ymax):
    b = Bits()
    n = sbits(xmin, xmax, ymin, ymax)
    b.ub(n, 5)
    for v in (xmin, xmax, ymin, ymax):
        b.sb(v, n)
    return b.align()


def matrix(tx, ty, angle=None):
    """MATRIX: optional rotation (uniform scale 1), translation in twips."""
    b = Bits()
    if angle is None:
        b.ub(0, 1)  # HasScale
        b.ub(0, 1)  # HasRotate
    else:
        c = int(round(math.cos(angle) * 65536))
        s = int(round(math.sin(angle) * 65536))
        n = sbits(c, s, -s)
        b.ub(1, 1)  # HasScale
        b.ub(n, 5)
        b.sb(c, n)  # ScaleX
        b.sb(c, n)  # ScaleY
        b.ub(1, 1)  # HasRotate
        b.ub(n, 5)
        b.sb(s, n)  # RotateSkew0
        b.sb(-s, n)  # RotateSkew1
    n = sbits(tx, ty)
    b.ub(n, 5)
    b.sb(tx, n)
    b.sb(ty, n)
    return b.align()


def tag(code, body):
    if len(body) < 63:
        return struct.pack("<H", (code << 6) | len(body)) + body
    return struct.pack("<HI", (code << 6) | 0x3F, len(body)) + body


def define_shape(shape_id, w, h, rgb):
    """DefineShape: one solid-filled rectangle (w x h twips) with its origin at the top-left."""
    body = struct.pack("<H", shape_id) + rect(0, w, 0, h)
    # FILLSTYLEARRAY: one solid fill; LINESTYLEARRAY: none
    body += bytes([1, 0x00]) + bytes(rgb) + bytes([0])
    b = Bits()
    b.ub(1, 4)  # NumFillBits
    b.ub(0, 4)  # NumLineBits
    # StyleChangeRecord: MoveTo (0,0), FillStyle0 = 1
    b.ub(0, 1)  # TypeFlag: non-edge
    b.ub(0, 1)  # StateNewStyles
    b.ub(0, 1)  # StateLineStyle
    b.ub(0, 1)  # StateFillStyle1
    b.ub(1, 1)  # StateFillStyle0
    b.ub(1, 1)  # StateMoveTo
    b.ub(1, 5)  # MoveBits
    b.sb(0, 1)
    b.sb(0, 1)
    b.ub(1, 1)  # FillStyle0 (NumFillBits wide)
    for dx, dy in ((w, 0), (0, h), (-w, 0), (0, -h)):
        n = max(sbits(dx, dy), 2)
        b.ub(1, 1)  # TypeFlag: edge
        b.ub(1, 1)  # StraightFlag
        b.ub(n - 2, 4)  # NumBits
        b.ub(1, 1)  # GeneralLineFlag
        b.sb(dx, n)
        b.sb(dy, n)
    b.ub(0, 6)  # EndShapeRecord
    return tag(2, body + b.align())


def place(depth, character=None, m=b""):
    flags = 0x04  # HasMatrix
    flags |= 0x02 if character is not None else 0x01  # HasCharacter, else Move
    body = bytes([flags]) + struct.pack("<H", depth)
    if character is not None:
        body += struct.pack("<H", character)
    return tag(26, body + m)


def build():
    W, H = 800 * TWIP, 600 * TWIP
    fps, frames = 30, 90
    side_a, side_b = 160 * TWIP, 120 * TWIP
    tags = tag(9, bytes([0x10, 0x18, 0x30]))  # SetBackgroundColor: dark blue
    tags += define_shape(1, side_a, side_a, (0xF2, 0x8C, 0x28))  # orange
    tags += define_shape(2, side_b, side_b, (0x5A, 0xD8, 0x7A))  # green
    for f in range(frames):
        t = f / frames
        ax = int((400 + 260 * math.sin(2 * math.pi * t)) * TWIP) - side_a // 2
        ay = int((300 + 120 * math.cos(4 * math.pi * t)) * TWIP) - side_a // 2
        angle = 2 * math.pi * t
        # the spinning square turns about its centre: translate by the rotated half-diagonal
        hx, hy = side_b / 2, side_b / 2
        cx, cy = 400 * TWIP, (300 + 180 * math.sin(2 * math.pi * t)) * TWIP
        bx = int(cx - (hx * math.cos(angle) - hy * math.sin(angle)))
        by = int(cy - (hx * math.sin(angle) + hy * math.cos(angle)))
        first = f == 0
        tags += place(1, 1 if first else None, matrix(ax, ay))
        tags += place(2, 2 if first else None, matrix(bx, by, angle))
        tags += tag(1, b"")  # ShowFrame
    tags += tag(0, b"")  # End
    header = rect(0, W, 0, H) + struct.pack("<HH", fps << 8, frames)
    length = 8 + len(header) + len(tags)
    return b"FWS" + bytes([6]) + struct.pack("<I", length) + header + tags


if __name__ == "__main__":
    out = sys.argv[1] if len(sys.argv) > 1 else "bounce.swf"
    data = build()
    with open(out, "wb") as f:
        f.write(data)
    print(f"{out}: {len(data)} bytes, 90 frames at 30 fps, 800x600")
