#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-or-later
"""tracepix.py <trace.jsonl> <x> <y> [draw ...]: for the traced (last) frame, which triangles
of each draw (all draws if none given) cover the centre of pixel (x, y) (GL window coordinates,
row 0 = y 0), with their window depth there (unskinned geometry, culling as traced)."""
import json, sys, base64, struct
import numpy as np

path, px, py = sys.argv[1], int(sys.argv[2]), int(sys.argv[3])
want = set(int(x) for x in sys.argv[4:])
cx, cy = px + 0.5, py + 0.5
buffers = {}
vaos = {0: {'attr': {}, 'elem': None}}
cur_vao = 0
array_buffer = None
uni = {}
viewport = (0, 0, 1, 1)
caps = set()
cull = 0x405
front = 0x901
depth_func = 0x201
poff = (0.0, 0.0)
frame = 0
lines = open(path).read().splitlines()
target = None
for l in reversed(lines):
    if l.startswith('["frame"'):
        target = json.loads(l)[1]
        break
di = 0
for l in lines:
    a = json.loads(l)
    n = a[0]
    if n == 'frame':
        frame = a[1]
    elif n == 'createVertexArray':
        vaos[a[1]] = {'attr': {}, 'elem': None}
    elif n == 'bindVertexArray':
        cur_vao = a[1] if a[1] is not None else 0
    elif n == 'bindBuffer':
        if a[1] == 0x8892:
            array_buffer = a[2]
        else:
            vaos[cur_vao]['elem'] = a[2]
    elif n == 'bufferData':
        data = base64.b64decode(a[2]['b64'])
        bid = array_buffer if a[1] == 0x8892 else vaos[cur_vao]['elem']
        buffers[bid] = data
    elif n == 'vertexAttribPointer':
        idx, size, typ, norm, stride, off = a[1:7]
        vaos[cur_vao]['attr'].setdefault(idx, {}).update(buf=array_buffer, size=size, type=typ, stride=stride, off=off)
    elif n.startswith('uniform'):
        uni[a[2]] = a[-1] if n in ('uniform1iv', 'uniform1fv', 'uniform3fv', 'uniform4fv', 'uniformMatrix4fv') else a[3:]
    elif n == 'viewport':
        viewport = tuple(a[1:5])
    elif n == 'enable':
        caps.add(a[1])
    elif n == 'disable':
        caps.discard(a[1])
    elif n == 'cullFace':
        cull = a[1]
    elif n == 'frontFace':
        front = a[1]
    elif n == 'depthFunc':
        depth_func = a[1]
    elif n == 'polygonOffset':
        poff = tuple(a[1:3])
    elif n in ('drawElements', 'drawArrays') and frame == target:
        if not want or di in want:
            v = vaos[cur_vao]
            at = v['attr'].get(0, {})
            data = buffers.get(at.get('buf'))
            stride = at.get('stride') or 12
            if n == 'drawElements':
                mode, count, typ, off = a[1:5]
                eb = buffers[v['elem']]
                fmt = {0x1405: ('<I', 4), 0x1403: ('<H', 2), 0x1401: ('<B', 1)}[typ]
                idx = [struct.unpack_from(fmt[0], eb, off + k * fmt[1])[0] for k in range(count)]
            else:
                mode, first, count = a[1:4]
                idx = list(range(first, first + count))
            if mode == 4 and data is not None and uni.get('u_skinning_enabled', [0])[0] == 0:
                M = np.array(uni['u_model'], dtype=np.float64).reshape(4, 4).T
                V = np.array(uni['u_view'], dtype=np.float64).reshape(4, 4).T
                P = np.array(uni['u_projection'], dtype=np.float64).reshape(4, 4).T
                PVM = P @ V @ M
                for t in range(0, len(idx) - 2, 3):
                    cl = []
                    for k in idx[t:t + 3]:
                        p = struct.unpack_from('<3f', data, at['off'] + k * stride)
                        cl.append(PVM @ np.array([p[0], p[1], p[2], 1.0]))
                    # GL clipping: -w <= x, y, z <= w (Sutherland-Hodgman in clip space).
                    poly = cl
                    for axis in range(3):
                        for sgn in (1, -1):
                            out = []
                            for k in range(len(poly)):
                                A, B = poly[k], poly[(k + 1) % len(poly)]
                                da = A[3] - sgn * A[axis]
                                db = B[3] - sgn * B[axis]
                                if da >= 0:
                                    out.append(A)
                                if (da >= 0) != (db >= 0):
                                    tt = da / (da - db)
                                    out.append(A + (B - A) * tt)
                            poly = out
                            if not poly:
                                break
                        if not poly:
                            break
                    if len(poly) < 3:
                        continue
                    win = [(viewport[0] + (c[0] / c[3] + 1) * viewport[2] / 2, viewport[1] + (c[1] / c[3] + 1) * viewport[3] / 2, (c[2] / c[3]) * 0.5 + 0.5) for c in poly]
                    # facing from the whole polygon's signed area
                    area2 = sum(win[k][0] * win[(k + 1) % len(win)][1] - win[(k + 1) % len(win)][0] * win[k][1] for k in range(len(win)))
                    if area2 == 0:
                        continue
                    ccw = area2 > 0
                    is_front = ccw if front == 0x901 else not ccw
                    culled = 0xB44 in caps and ((cull == 0x404 and is_front) or (cull == 0x405 and not is_front) or cull == 0x408)
                    for k in range(1, len(win) - 1):
                        (x0, y0, z0), (x1, y1, z1), (x2, y2, z2) = win[0], win[k], win[k + 1]
                        area = (x1 - x0) * (y2 - y0) - (x2 - x0) * (y1 - y0)
                        if area == 0:
                            continue
                        w0 = ((x1 - cx) * (y2 - cy) - (x2 - cx) * (y1 - cy)) / area
                        w1 = ((x2 - cx) * (y0 - cy) - (x0 - cx) * (y2 - cy)) / area
                        w2 = 1 - w0 - w1
                        if min(w0, w1, w2) >= 0:
                            z = w0 * z0 + w1 * z1 + w2 * z2
                            clipped = len(poly) != 3 or any(c[3] <= 0 for c in cl)
                            print(f'draw {di} tri {t // 3}: covers, depth {z:.9f}, {"CULLED" if culled else "drawn"}{" (clipped)" if clipped else ""}, depthfunc 0x{depth_func:x}, poff {poff if 0x8037 in caps else None}, w {[round(float(c[3]), 2) for c in cl]}')
                            break
        di += 1
