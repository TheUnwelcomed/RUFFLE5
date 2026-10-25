#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-or-later
"""tracegeom.py <trace.jsonl> <draw index> [...]: for draws of the traced (last) frame, the
vertices' clip coordinates (gl_Position) and window coordinates, from the traced buffers,
vertex arrays and uniforms (unskinned: u_projection * u_view * u_model * a_position)."""
import json, sys, base64, struct
import numpy as np

path = sys.argv[1]
want = set(int(x) for x in sys.argv[2:])
buffers = {}
vaos = {0: {'attr': {}, 'elem': None}}
cur_vao = 0
array_buffer = None
uni = {}
viewport = (0, 0, 1, 1)
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
        tgt = a[1]
        data = base64.b64decode(a[2]['b64'])
        bid = array_buffer if tgt == 0x8892 else vaos[cur_vao]['elem']
        buffers[bid] = data
    elif n == 'vertexAttribPointer':
        idx, size, typ, norm, stride, off = a[1:7]
        vaos[cur_vao]['attr'].setdefault(idx, {})
        vaos[cur_vao]['attr'][idx].update(buf=array_buffer, size=size, type=typ, stride=stride, off=off)
    elif n == 'enableVertexAttribArray':
        vaos[cur_vao]['attr'].setdefault(a[1], {})['on'] = True
    elif n == 'disableVertexAttribArray':
        vaos[cur_vao]['attr'].setdefault(a[1], {})['on'] = False
    elif n.startswith('uniform'):
        uni[a[2]] = a[-1] if n in ('uniform1iv', 'uniform1fv', 'uniform3fv', 'uniform4fv', 'uniformMatrix4fv') else a[3:]
    elif n == 'viewport':
        viewport = tuple(a[1:5])
    elif n in ('drawElements', 'drawArrays') and frame == target:
        if di in want:
            v = vaos[cur_vao]
            at = v['attr'].get(0, {})
            data = buffers[at['buf']]
            stride = at['stride'] or 12
            if n == 'drawElements':
                mode, count, typ, off = a[1:5]
                eb = buffers[v['elem']]
                fmt = {0x1405: ('<I', 4), 0x1403: ('<H', 2), 0x1401: ('<B', 1)}[typ]
                idx = [struct.unpack_from(fmt[0], eb, off + k * fmt[1])[0] for k in range(count)]
            else:
                mode, first, count = a[1:4]
                idx = list(range(first, first + count))
            M = np.array(uni['u_model'], dtype=np.float64).reshape(4, 4).T
            V = np.array(uni['u_view'], dtype=np.float64).reshape(4, 4).T
            P = np.array(uni['u_projection'], dtype=np.float64).reshape(4, 4).T
            print(f'draw {di}: {n} mode {mode} {len(idx)} vertices, skinning {uni.get("u_skinning_enabled")}, viewport {viewport}')
            for t in range(0, len(idx) - 2, 3):
                row = []
                for k in idx[t:t + 3]:
                    p = struct.unpack_from('<3f', data, at['off'] + k * stride)
                    c = P @ V @ M @ np.array([p[0], p[1], p[2], 1.0])
                    if c[3] != 0:
                        nd = c[:3] / c[3]
                        wx = viewport[0] + (nd[0] + 1) * viewport[2] / 2
                        wy = viewport[1] + (nd[1] + 1) * viewport[3] / 2
                        row.append(f'clip({c[0]:9.3f},{c[1]:9.3f},{c[2]:9.3f},{c[3]:9.3f}) win({wx:8.1f},{wy:8.1f})')
                    else:
                        row.append(f'clip({c[0]:9.3f},{c[1]:9.3f},{c[2]:9.3f},{c[3]:9.3f})')
                print(f'  tri {t // 3}: ' + ' | '.join(row))
        di += 1
