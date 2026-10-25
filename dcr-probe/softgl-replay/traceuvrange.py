#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-or-later
"""traceuvrange.py <trace.jsonl> <draw> [...]: per draw of the traced frame, the range of every
float vertex attribute over all vertices in its buffers, and how many components are negative."""
import json, sys, base64, struct
import numpy as np
path = sys.argv[1]
want = set(int(x) for x in sys.argv[2:])
buffers = {}
vaos = {0: {'attr': {}, 'elem': None}}
cur_vao = 0
array_buffer = None
frame = 0
lines = open(path).read().splitlines()
target = None
for l in reversed(lines):
    if l.startswith('["frame"'):
        target = json.loads(l)[1]
        break
model = None
di = 0
for l in lines:
    a = json.loads(l)
    n = a[0]
    if n == 'frame':
        frame = a[1]
    elif n == 'note' and a[1].startswith('model '):
        model = a[1]
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
    elif n == 'enableVertexAttribArray':
        vaos[cur_vao]['attr'].setdefault(a[1], {})['on'] = True
    elif n in ('drawElements', 'drawArrays') and frame == target:
        if di in want:
            v = vaos[cur_vao]
            print(f'draw {di}: {model}')
            for i in (2, 3):
                at = v['attr'].get(i)
                if not at or not at.get('on'):
                    print(f'  attrib {i}: off')
                    continue
                d = np.frombuffer(buffers[at['buf']], dtype='<f4').reshape(-1, at['size'])
                print(f'  attrib {i}: {len(d)} vertices, min {d.min(axis=0).round(4)}, max {d.max(axis=0).round(4)}, negative components {(d < -0.001).sum(axis=0)}')
            if n == 'drawElements':
                eb = np.frombuffer(buffers[v['elem']], dtype='<u4')[a[4] // 4: a[4] // 4 + a[2]]
                ref = sorted(set(eb.tolist()))
                at = v['attr'].get(2)
                d = np.frombuffer(buffers[at['buf']], dtype='<f4').reshape(-1, at['size'])
                bad = [i for i in range(len(d)) if np.abs(d[i]).max() > 2.0]
                print(f'  referenced vertices {len(ref)} (max index {max(ref)}); |uv1| > 2 at {len(bad)} vertices, of them referenced: {len(set(bad) & set(ref))}')
                pos = np.frombuffer(buffers[v['attr'][0]['buf']], dtype='<f4').reshape(-1, 3)
                for i in bad[:4]:
                    print(f'    v{i}: uv1 {d[i]} pos {pos[i]}')
                for k in (2, 3):
                    at = v['attr'].get(k)
                    if not at or not at.get('on'):
                        continue
                    dd = np.frombuffer(buffers[at['buf']], dtype='<f4').reshape(-1, at['size'])[ref]
                    print(f'  referenced attrib {k}: min {dd.min(axis=0).round(4)}, max {dd.max(axis=0).round(4)}, negative {(dd < -0.001).sum(axis=0)}')
            if v['attr'].get(2, {}).get('on') and v['attr'].get(3, {}).get('on'):
                d2 = np.frombuffer(buffers[v['attr'][2]['buf']], dtype='<f4')
                d3 = np.frombuffer(buffers[v['attr'][3]['buf']], dtype='<f4')
                print(f'  uv1 == uv2: {np.array_equal(d2, d3)}')
        di += 1
