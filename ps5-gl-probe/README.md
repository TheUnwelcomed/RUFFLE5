# ps5-gl-probe — OpenGL ES on the PS5 through Mesa, as a payload (milestone 2, step 1)

Proves the GPU path Ruffle-on-PS5 will render through — SDL2 → EGL ("videoout") → zink → RADV on the
console's AGC — before any Rust touches it. One C file, built against pacbrew's SDK (v0.41.1, the
first release with Mesa 26.2 for the PS5 and the SDL2 that loads it).

## Files

| file | what |
|---|---|
| `ps5-gl-probe.elf` | the payload (v3), built here against pacbrew v0.41.1 (SDL 2.30.12) — ready to run |
| `main.c` | PS5_GL_PROBE_V3: direct-memory check and budget_set(0) first, then every step logged, Mesa's own stderr captured into the log, every GL function fetched with `SDL_GL_GetProcAddress` (as glow does); when SDL fails, RADV and EGL are driven by hand and each call's result is logged |
| `build.sh` | rebuilds it: `PS5_PAYLOAD_SDK=<pacbrew sdk> ./build.sh` |
| `ruffle-ps5-mesa-v0.41.1.zip` (separate) | the five Mesa shared objects the console needs, from the same release |

## Run it

1. Unpack `ruffle-ps5-mesa-v0.41.1.zip` and copy its `lib/` folder to the console as `/data/ruffle/lib`
   (FTP), so `/data/ruffle/lib/libEGL.so.1` exists. 45 MB; once.
2. Launch `ps5-gl-probe.elf` the way the milestone-1 payload was launched: from websrv's homebrew page
   (`/data/homebrew/GLProbe/eboot.elf`), or `nc <ps5> 9021 < ps5-gl-probe.elf` from a machine on the
   console's network.
3. Expected on the TV: toast "GL probe: starting SDL", then a spinning red/green/blue triangle on a dark
   blue background for 6 seconds (any controller button ends it early), then a toast
   "GL probe OK: zink Vulkan 1.4(AMD …) — OpenGL ES 3.x Mesa 26.2.0 — 60 fps" (or similar).
4. Fetch `/data/ruffle/gl-probe.log` (v2 also holds Mesa's own messages: `EGL_LOG_LEVEL=debug`,
   `RADV_DEBUG=info`). It has the display mode, the ES version SDL got, the GL strings,
   MAX_TEXTURE_SIZE / MAX_SAMPLES, the first extensions, whether Ruffle's GLSL ES 1.00 shaders compiled,
   the first-frame latency and the frame-time statistics. If anything fails, the line before `FAIL`
   says which call and what SDL said.

## What it decides

- The ES version and `OES_element_index_uint` / VAO availability: what `ruffle_render_glow` gets.
- MAX_SAMPLES: the MSAA Ruffle's "high" quality asks for.
- Frame time at 3840x2160 with vsync: whether the swapchain runs at the display rate.
- Where the libraries can live: `LD_LIBRARY_PATH=/data/ruffle/lib` (set by the payload) or `/user/homebrew/lib`.

## Why a payload first

A sandboxed title cannot use the SDK's `dlopen` (its loader makes raw syscalls), so Mesa would have to
be linked statically there. The payload route needs no such work and answers the GPU questions now;
the title gets its renderer in milestone 3 (static RADV, as PS5_RetroArch links it, under wgpu or zink).

## v1 → v2

v1 on the console (2026-10-20): SDL_Init fine, then `SDL_CreateWindow: Could not initialize EGL` for ES 3.0
and ES 2.0 — libEGL.so.1 and libGLESv2.so.2 had loaded (otherwise SDL's message names the library), and
`eglInitialize` failed. Mesa prints the reason on stderr, which nobody sees when the payload comes from a
launcher; v2 writes stdout/stderr into the log, switches Mesa's debug output on, and when SDL fails it
loads libvulkan.so.1 alone (instance, devices, VK_KHR_display) and then libEGL.so.1 alone, logging each call.

## v2 → v3

v2's log settled it: RADV runs in the payload (AGC initialised, "PlayStation 5 GPU (RADV NAVI21)",
one VK_KHR_display display), but zink's `vkCreateDevice` got `VK_ERROR_OUT_OF_DEVICE_MEMORY` — what
RADV's ps5 winsys returns when a direct-memory allocation fails — and `RADV_DEBUG=info` reported
`vram_size = 0 MB`. Direct memory is budgeted per process; John Törnblom's elfldr and selfldr both call
`budget_set(0)` (syscall 0x23b) in the child before exec, and the payload manager that launched v1/v2
(pid 145, uid 1, euid 0) may not. v3 measures the pool and allocates 16 MB / 64 MB the way RADV does,
and if that fails calls `budget_set(0)` itself and measures again — before SDL starts, so a fix shows
up as the triangle in the same run. If the memory is still short, launching through websrv or elfldr
(port 9021) is the way, since those set the budget.
