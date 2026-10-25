#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-or-later
"""traceuv.py <trace.jsonl> <draw>: a draw of the traced frame: its uniforms, its textures' sizes,
and per vertex a_position / a_texcoord / a_texcoord2 and the v_texcoord the vertex shader makes."""
import json, sys, base64, struct
import numpy as np

path, want = sys.argv[1], int(sys.argv[2])
buffers = {}
vaos = {0: {'attr': {}, 'elem': None}}
cur_vao = 0
array_buffer = None
uni = {}
unit = 0
bound = {}
texinfo = {}
texparams = {}
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
    elif n == 'enableVertexAttribArray':
        vaos[cur_vao]['attr'].setdefault(a[1], {})['on'] = True
    elif n == 'disableVertexAttribArray':
        vaos[cur_vao]['attr'].setdefault(a[1], {})['on'] = False
    elif n.startswith('uniform'):
        uni[a[2]] = a[-1] if n in ('uniform1iv', 'uniform1fv', 'uniform3fv', 'uniform4fv', 'uniformMatrix4fv') else a[3:]
    elif n == 'activeTexture':
        unit = a[1] - 0x84C0
    elif n == 'bindTexture':
        bound[unit] = a[2]
    elif n == 'texImage2D':
        if a[2] == 0:
            texinfo[bound.get(unit)] = (a[4], a[5])
    elif n == 'texParameteri':
        texparams.setdefault(bound.get(unit), {})[a[2]] = a[3]
    elif n in ('drawElements', 'drawArrays') and frame == target:
        if di == want:
            v = vaos[cur_vao]
            print(f'draw {di} {a}')
            for k in sorted(uni):
                val = uni[k]
                if k in ('u_bone_matrices',):
                    continue
                print(f'  {k} = {val}')
            for u_, t in sorted(bound.items()):
                print(f'  unit {u_}: texture {t} size {texinfo.get(t)} params {texparams.get(t)}')
            for idx_, at in sorted(v['attr'].items()):
                print(f'  attrib {idx_}: {at}')
            if n == 'drawElements':
                mode, count, typ, off = a[1:5]
                eb = buffers[v['elem']]
                fmt = {0x1405: ('<I', 4), 0x1403: ('<H', 2), 0x1401: ('<B', 1)}[typ]
                idx = [struct.unpack_from(fmt[0], eb, off + k * fmt[1])[0] for k in range(count)]
            else:
                mode, first, count = a[1:4]
                idx = list(range(first, first + count))
            T = np.array(uni.get('u_tex_transform', list(np.eye(4).flatten())), dtype=np.float64).reshape(4, 4).T
            def attr(i, k):
                at = v['attr'].get(i)
                if not at or not at.get('on'):
                    return None
                d = buffers[at['buf']]
                st = at['stride'] or at['size'] * 4
                return struct.unpack_from('<%df' % at['size'], d, at['off'] + k * st)
            for k in sorted(set(idx)):
                p = attr(0, k); t = attr(2, k); t2 = attr(3, k)
                vt = None
                if t is not None:
                    du = np.array([t[0] + 0.5, t[1] + 0.5, 0.0, 1.0])
                    tt = T @ du
                    vt = (round(tt[0], 4), round(1 - tt[1], 4))
                print(f'  v{k}: pos {tuple(round(x, 2) for x in p)} uv {t and tuple(round(x, 4) for x in t)} uv2 {t2 and tuple(round(x, 4) for x in t2)} -> v_texcoord {vt}')
            break
        di += 1
