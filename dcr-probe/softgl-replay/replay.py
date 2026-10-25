#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-or-later
"""replay.py <trace.jsonl> <out prefix> [--owner] [--soft FILE] [--stop-after N] [--skip-before F]

Replays a softgl call trace in headless Chromium's WebGL2 (ANGLE on SwiftShader) with the
original GLSL, then compares the browser's picture of the result texture with the software
one (<trace>.rgba, or --soft FILE: what `dcr-probe --softgl-replay` wrote). Writes <out>-ref.png,
<out>-soft.png, <out>-diff.png, <out>-side.png (row 0 at the top, as the texture's row 0 is the
sprite's top) and <out>-ref.rgba, and prints the difference statistics.
  --owner          also <out>-ref.owner: per pixel, the last draw of the traced frame that changed it
  --stop-after N   draw only the traced frame's first N draws
  --skip-before F  skip the draws of the frames before F (faster; earlier frames' pictures are lost)"""
import argparse, base64, json, os, sys
import numpy as np
from PIL import Image
from playwright.sync_api import sync_playwright

HERE = os.path.dirname(os.path.abspath(__file__))

ap = argparse.ArgumentParser()
ap.add_argument('trace')
ap.add_argument('out')
ap.add_argument('--skip-before', type=int, default=0)
ap.add_argument('--stop-after', type=int, default=None)
ap.add_argument('--owner', action='store_true')
ap.add_argument('--soft', default=None, help='software picture to compare with (default <trace>.rgba)')
args = ap.parse_args()

trace_bytes = open(args.trace, 'rb').read()
js = open(os.path.join(HERE, 'replay.js')).read()

with sync_playwright() as p:
    b = p.chromium.launch(args=['--use-angle=swiftshader', '--enable-unsafe-swiftshader', '--ignore-gpu-blocklist', '--enable-webgl'])
    page = b.new_page()
    page.route('http://replay.invalid/trace', lambda r: r.fulfill(status=200, body=trace_bytes, content_type='text/plain'))
    page.route('http://replay.invalid/', lambda r: r.fulfill(status=200, body='<!doctype html><title>replay</title>', content_type='text/html'))
    page.goto('http://replay.invalid/')
    page.add_script_tag(content=js)
    opts = {'skipBefore': args.skip_before, 'stopAfter': args.stop_after, 'owner': args.owner}
    out = page.evaluate('([u, o]) => replayTrace(u, o)', ['http://replay.invalid/trace', opts])
    b.close()

for l in out['log']:
    print('[replay]', l)
res = out['result']
if not res:
    sys.exit('no result line reached')
w, h = res['w'], res['h']
ref = np.frombuffer(base64.b64decode(res['rgba']), dtype=np.uint8).reshape(h, w, 4)
soft = np.frombuffer(open(args.soft or (args.trace + '.rgba'), 'rb').read(), dtype=np.uint8).reshape(h, w, 4)
open(args.out + '-ref.rgba', 'wb').write(ref.tobytes())
if res.get('owner'):
    open(args.out + '-ref.owner', 'wb').write(base64.b64decode(res['owner']))
print(f'[replay] frame {res["frame"]}, {w}x{h}, framebuffer status 0x{res["status"]:x}')
d = np.abs(ref.astype(np.int16) - soft.astype(np.int16))
dm = d[:, :, :3].max(axis=2)
da = d[:, :, 3]
n = w * h
for t in (0, 2, 8, 32, 96):
    print(f'[diff] rgb max-channel diff > {t:3d}: {int((dm > t).sum()):7d} px ({100.0 * (dm > t).sum() / n:6.2f}%)')
print(f'[diff] alpha diff > 0: {int((da > 0).sum())} px; mean rgb diff {d[:, :, :3].mean():.3f}')
Image.fromarray(ref, 'RGBA').save(args.out + '-ref.png')
Image.fromarray(soft, 'RGBA').save(args.out + '-soft.png')
vis = np.zeros((h, w, 3), dtype=np.uint8)
vis[:, :, 0] = np.clip(dm.astype(np.int32) * 4, 0, 255)
vis[:, :, 1] = np.where(dm > 32, 255, 0)
Image.fromarray(vis, 'RGB').save(args.out + '-diff.png')
# side by side (opaque view)
side = np.concatenate([ref[:, :, :3], soft[:, :, :3], vis], axis=1)
Image.fromarray(side, 'RGB').save(args.out + '-side.png')
print('[replay] wrote', args.out + '-{ref,soft,diff,side}.png')
