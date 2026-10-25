#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-or-later
"""tracedraws.py <trace.jsonl> [draw ...]: the draws of the traced (last) frame with the model
and texture layers the renderer noted before each, the textures bound (by name) and key uniforms."""
import json, sys
path = sys.argv[1]
want = set(int(x) for x in sys.argv[2:])
lines = open(path).read().splitlines()
target = None
for l in reversed(lines):
    if l.startswith('["frame"'):
        target = json.loads(l)[1]
        break
texname = {}
unit = 0
bound = {}
uni = {}
model = layers = None
frame = 0
di = 0
KEYS = ('u_has_texture', 'u_has_lightmap', 'u_layer2_blend', 'u_texture_unlit', 'u_opacity', 'u_alpha_threshold', 'u_diffuse_color', 'u_emissive_color', 'u_has_texcoord2', 'u_texcoord2_direct', 'u_skinning_enabled')
for l in lines:
    a = json.loads(l)
    n = a[0]
    if n == 'frame':
        frame = a[1]
    elif n == 'note':
        t = a[1]
        if t.startswith('texture '):
            nm, _, i = t[8:].rpartition(' = ')
            texname[int(i)] = nm
        elif t.startswith('model '):
            model = t
            layers = None
        elif t.startswith('layers '):
            layers = t
    elif n == 'activeTexture':
        unit = a[1] - 0x84C0
    elif n == 'bindTexture':
        bound[unit] = a[2]
    elif n.startswith('uniform'):
        uni[a[2]] = a[3:] if len(a) > 4 else a[3]
    elif n in ('drawElements', 'drawArrays') and frame == target:
        if not want or di in want:
            tex = {u: (t, texname.get(t, '?')) for u, t in sorted(bound.items()) if t is not None}
            print(f'draw {di} {a[1:]}: {model}')
            print(f'    {layers}')
            print(f'    textures {tex}')
            print('    ' + ', '.join(f'{k[2:]}={uni.get(k)}' for k in KEYS))
        di += 1
