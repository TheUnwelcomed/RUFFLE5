# softgl-replay — checking Ruffle5's software 3D against a browser

Ruffle5 draws Shockwave 3D with DirPlayer's own WebGL2 renderer running on a software WebGL2
(`rendering_gpu/softgl` in `ruffle-ps5-title/tools/dirplayer-native.patch`). These tools check
its pictures against a real WebGL2 drawing the same calls with DirPlayer's GLSL.

1. Record a frame's calls with the probe:

       DIRPLAYER_SOFTGL_TRACE=50:/tmp/rc50.jsonl dcr-probe game.dcr out 26 "...inputs..."

   writes every WebGL call the 3D renderer made up to the end of its 50th 3D sprite frame, and
   that frame's software picture beside it (`/tmp/rc50.jsonl.rgba`).
2. Draw it again in software, with per-draw ownership of every pixel:

       dcr-probe --softgl-replay /tmp/rc50.jsonl /tmp/rc50-soft

3. Replay it in headless Chromium's WebGL2 (ANGLE on SwiftShader) and compare:

       python3 replay.py /tmp/rc50.jsonl /tmp/rc50 --owner --soft /tmp/rc50-soft.rgba

   prints how many pixels differ by how much and writes `-ref.png`, `-soft.png`, `-diff.png`,
   `-side.png` (and `-ref.owner`). Needs Python 3 with numpy, Pillow and Playwright's Chromium.

The browser's depth buffer is the renderer's DEPTH_COMPONENT16; the software one keeps depth as
f32. `DIRPLAYER_SOFTGL_DEPTH_BITS=16` on step 2 rounds depth as the 16-bit buffer does, for a
like-for-like comparison. The software model program lights per vertex (as Director's IFX
renderer did); DirPlayer's GLSL lights per pixel — curved surfaces differ by a few levels.

`tracedraws.py` (each draw of the traced frame: its model, texture layers, textures by name and key
uniforms), `tracepix.py` (the triangles that cover a pixel and their depths), `traceuv.py` and
`traceuvrange.py` (a draw's vertices and texture coordinates), `tracegeom.py` (its triangles in clip
and window coordinates) read the same traces.
