# ruffle-ps5-title — Ruffle5: Ruffle as a native PS5 title

Flash content on the TV, rendered on the console's GPU, as a Games-row title — the real goal.

## Why a title, not a payload

The payload route (milestone 2) proved the GPU works but died on the launcher: Sony's `libSceAgc`
refuses the process websrv/elfldr hand it (`FS Table offset has shifted`, `sceAgcInit = 0x8a6c002f`),
while the payload-manager process that passed `sceAgcInit` had no direct-memory budget. A **title**
has neither problem: the system loader gives it a proper process with a game's memory and a GPU the
graphics library accepts — exactly as the Rust Hello tile (milestone 1) and PS5_RetroArch run. So the
renderer moves into a title, and everything is **static** (a title cannot `dlopen`).

## The renderer

```
ruffle_core ──► ruffle_render_wgpu ──► wgpu 30 ──► wgpu-hal (vulkan) ──► RADV (static archive)
                                                                           │
                                   VK_KHR_display on VideoOut ◄────────────┘   (RADV flips the display)
```

The one piece with no Rust prior art is handing wgpu a Vulkan device built on the **statically
linked** RADV instead of a `dlopen`'d `libvulkan.so.1`:

* the C shell exports RADV's `vk_icdGetInstanceProcAddr` (as PS5_RetroArch's `radv_icd_ps5.c` does);
* `src/gpu.rs` builds `ash::Entry` from that static symbol (no loader — a title cannot dlopen),
  creates the instance + a `VK_KHR_display` surface with plain Vulkan, and adopts them into wgpu
  through its `*_from_hal` seams;
* `ruffle_render_wgpu` then runs unchanged.

Every wgpu-hal / wgpu call in `src/gpu.rs` is written against the sources of the pinned versions
(wgpu 30.0.1, ash 0.38): `Instance::from_raw`, `expose_adapter`, `Adapter::open`, `from_hal`,
`create_adapter_from_hal`, `create_device_from_hal`. One thing wgpu 30 cannot do is adopt a
`VkSurfaceKHR` the application made itself (its only display-less route wants a Linux DRM fd), so
`build-title.sh` fetches wgpu v30.0.1 into `vendor/wgpu` and applies
`tools/wgpu-hal-ps5-surface.patch` — eleven lines in wgpu-hal's Vulkan backend that take the surface
handle through a raw-window-handle; `Cargo.toml` points `[patch.crates-io]` at that tree.

## The one dependency you must build first: the static RADV archive

A title links RADV as `libvulkan_radeon.ps5.a`, which **PS5_Vulkan** builds from its Mesa fork:

```sh
# in a PS5_Vulkan checkout (it pulls its PS5_Mesa fork):
tools/build-radv.sh release      # → .deps/native/radv-release/lib/libvulkan_radeon.ps5.a
```

`build-title.sh` here links that archive with PS5_Vulkan's own `tools/radv-link.sh` recipe (libc++,
libunwind, the SDK platform layer, the pthread/memstream wraps), the same way PS5_RetroArch's title
does. Point `RADV_ARCHIVE` / `PS5_VULKAN_DIR` at your checkout.

This is the largest single setup step in the whole project. If PS5_Vulkan + PS5_Mesa are not already
building for you, that is step one, before any of this links.

## Phases

* **3a — `./build-title.sh` (no features):** bring up wgpu on RADV and clear the display to an
  animated colour. Proves the static-RADV + VK_KHR_display + wgpu seam in a title; it exercises
  only `src/gpu.rs`.
* **3b — `./build-title.sh --ruffle`:** the product: the Ruffle5 menu, a Ruffle player per game.

Release builds are `opt-level = 3`, fat LTO, one codegen unit, for `znver2` (the PS5's Zen 2 cores:
`"cpu"` in `x86_64-sie-ps5.json`).

## The display: 4K, and what "4K output" in Settings does

The swapchain is opened at **3840x2160** (1920x1080 if the TV offers no 4K mode), once. Each game
is then drawn one of two ways, chosen by Settings › Display › Resolution:

* **4K** — Ruffle renders the game straight into the 4K swapchain. Flash is vectors, so this is a
  real 4K rendering of the shapes and text (not an enlarged picture), with no extra copy.
* **1080p** — Ruffle renders into a 1920x1080 texture that one GPU pass scales to the screen
  (bilinear). The cheaper choice for a game that is slow at 4K.

The menu is laid out at 1920x1080 and drawn at the display's size, its text rasterised at 4K.
The `[Game]` lines in the log (below) say which a game ran at and how long each frame took, so the
two settings can be compared on the same game.

## Files

| file | what |
|---|---|
| `src/gpu.rs` | the static-RADV → wgpu bring-up (the novel piece); picks 3840x2160, else 1920x1080 |
| `src/lib.rs` | `ruffle_ps5_main`: 3a clear-screen; 3b the app — the menu loop, a player per game, back |
| `src/shell.rs` | the display: the swapchain, a game's `RenderTarget` (direct at 4K, or 1080p + GPU scale), cover capture, frame timings |
| `src/menu.rs` | the menu: Games / Favorites / Recent / Badges / Settings; stage, shelf, search, screensaver |
| `src/game.rs` | one game: loading screen, the player and its options, quick menu, overlay, the frame-time log |
| `src/background.rs` | the menu's moving background: ruffle.rs's logo animation, played by Ruffle at the display's size |
| `src/widgets.rs` | the shared look: Ruffle's colours, buttons, hints, focus halo/ring, ribbons, sparks, easing |
| `src/ui.rs` | the 2D renderer: one instanced SDF-quad pipeline (rounded rects, rings, glows, sheared ribbons, pictures), text, clipping |
| `src/editor.rs` | the controls editor (one game, or all games), in the menu's look; writes Custom Mappings |
| `src/controls.rs` | the mapping as data: defaults, the layered files, writing a file the way presets are written |
| `src/settings.rs` | Settings: rows, values, help lines, `settings.txt` |
| `src/quickmenu.rs` | the in-game menu (hold OPTIONS) |
| `src/keyboard.rs` | the on-screen keyboard |
| `src/library.rs` | favourites, recently played, play time, the last session (recovered after a crash) |
| `src/badges.rs` | 34 badges for using the app, with progress |
| `src/covers.rs` | a game's own cover picture, taken from its frame |
| `src/saves.rs` | Flash saves (SharedObjects) on disk |
| `src/ps5ui.rs` | Ruffle's UI hooks: text boxes open the keyboard, the pointer can be hidden |
| `src/hid.rs` + `shell/hid.c` | a USB keyboard and mouse (system modules loaded first thing in `main()`, functions imported directly; the log shows what the console answers) |
| `src/cpu.rs` | a CPU core of its own for the sound |
| `src/sounds.rs` | the menu's sounds, on their own audio port |
| `src/input.rs` | the DualSense (and USB devices) as keyboard and mouse; menu navigation with auto-repeat |
| `src/cursor.rs` | the pointer drawn over the game |
| `src/audio.rs` | Ruffle's mixer → AudioOut: mixed in f32, clamped to s16 |
| `src/swfinfo.rs` | a SWF's header (version, size, fps, frames) without inflating the movie |
| `src/paths.rs`, `src/fsutil.rs` | where things live; durable and background writes |
| `shell/main.c` | the C title shell: `sceAgcInit`, the ICD entry, toasts, the folders |
| `shell/pad.c` | scePad / AudioOut |
| `Custom Mappings/` | presets shipped in the title's Custom Mappings folder (`dadnme` so far) |
| `controls.txt` | the mapping file's format and the defaults, shipped as a commented example |
| `assets/` | `ruffle-mark.png` (the menu's logo), `logo-anim.swf` (the background) |
| `art/` | `make-art.py`: draws `assets/ruffle-mark.png` and the launch icon `sce_sys/icon0.png` from Ruffle's logo |
| `fonts/` | Inter Regular/SemiBold, Montserrat Medium (SIL OFL 1.1), embedded |
| `build-title.sh` | links the Rust staticlib + RADV archive + runtime into a PPSA title (needs PS5_Vulkan) |

## On the console

| where | what |
|---|---|
| `/data/homebrew/PPSA99620/games/` | your games (`*.swf`, or one game per sub-folder); also `ruffle5/` on a USB drive |
| `/data/homebrew/PPSA99620/Custom Mappings/` | every controls file: the shipped presets, the ones the editor writes, `_All Games.controls.txt` |
| `/data/ruffle5/` | `ruffle.log`, `settings.txt`, the library and badges, `saves/`, `covers/` |

Ruffle5 used `/data/ruffle` before 1.000.020; Ruffle-Flash-PS5 uses that folder too, so Ruffle5 now
keeps its own. Mapping files found in the old places (`/data/ruffle/*.controls.txt`,
`games/*.controls.txt`) are moved into Custom Mappings once, at start; the log lists each move.

## Shockwave (Director) games

Ruffle plays Flash. Macromedia Director movies — Shockwave games (`.dcr`), projector games
(`.dir`/`.dxr` with `.cct` casts and Xtras) — are a different format with their own language,
Lingo; Ruffle5 plays them with [DirPlayer](https://github.com/igorlira/dirplayer-rs), a Director
player in Rust, built into the title (`src/shockwave.rs`):

* put a `.dcr` in the games folder, or a game's whole folder (movie, `.cct` casts, `Xtras/`,
  `Projector.ini`): the menu lists the folder as one game, started by the movie its
  `Projector.ini` names (`[Movies] Movie01=`), else its only `.dcr`; its controls are
  `Custom Mappings/<folder name>.controls.txt`;
* the stage is rendered on the CPU and scaled to the TV: Settings › Display › Scale mode
  *Unscaled* = whole multiples (every pixel the same size), *Zoom to Fit*/*Crop* = as large as fits,
  *Stretch* = the whole screen; Quality Low/Medium = sharp pixels, High and up = smooth;
* keys and the mouse reach the movie as a browser's events would; sound is DirPlayer's, mixed
  without a browser (`native_audio`, MP3 through Symphonia);
* quick menu, controls editor, on-screen keyboard, covers, FPS and play time work as for Flash;
* a shipped preset for *Teen Titans: Battle Blitz* (`Custom Mappings/ttbattleblitz.controls.txt`,
  for a `games/ttbattleblitz/` folder): Square punch (Z), Cross/Circle kick (X), Triangle jump,
  d-pad run/duck, OPTIONS pause (P; hold it for the quick menu).

DirPlayer's tasks and timers run on its own small executor off the web (`native_rt.rs` in the
patch): async-std's runtime needs async-io, whose kqueue reactor does not start on the console
(1.000.021 aborted with "cannot initialize I/O event notification", EINVAL). Nothing in Ruffle5
opens a kqueue now.

Frames follow Director's model, as DirPlayer's own frame loop does: one score frame, then the stage
drawn, at most at the movie's tempo and never a burst of frames to catch up (1.000.024; before, up
to three frames ran per picture, and a game that paces itself — Battle Blitz sets `puppetTempo 999`
and waits inside each frame until 33 ms after the last — showed one frame in three). After each
frame the same steps run as on the web (`run_native_frame` in the patch): the sound channels'
tick (the end of a sound frees its channel and loops or queues the next one; without it every
channel stayed busy and a game ran out of them), the `idle` event and cue points.

`tools/fetch-dirplayer.sh` (run by `build-title.sh`) fetches DirPlayer at a pinned commit into
`vendor/dirplayer` and applies `tools/dirplayer-native.patch`: an embedding API for a native
program (`player/native.rs`), a sound mixer without Web Audio (`player/native_audio.rs`), local
files instead of `fetch()`, browser-console calls compiled out off the web, `rect.intersect()`
returning the overlap (DirPlayer returned the union, so every collision test in a fighting game
hit), its `image` dependency cut to the formats movies use (no AV1 encoder in the build), its
wasm-bindgen pins relaxed to share Ruffle's versions, an rlib-only crate type, and a native task
executor and timers (`player/native_rt.rs`) in place of async-std's runtime, the web frame loop's
per-frame steps for a native host (`run_native_frame`), and a per-row path for sprites drawn
upright onto the stage (`copy_pixels_with_params`: the same pixels as DirPlayer's per-pixel loop,
checked by `dcr-probe --blit-selftest` and `DCR_PROBE_COMPARE=1`, 3 to 4 times faster).
`dcr-probe/` (at the repository root) runs a movie in DirPlayer on a PC or server and saves the
stage as PNGs — for checking a game before it goes on the console.

## The menu

The look is Ruffle's: its blue and orange, its logo beside the name, and the ruffle.rs logo
animation playing behind everything (Settings › App can turn it off). The layout follows
ProsperoStore's home screen: a big stage for the focused game, a shelf of tiles below. The focused
cover breathes a soft glow, a ring with a running light goes round its border, and a leaning
Ruffle ribbon sits under its name; tiles lift and wobble as the focus lands, the stage crossfades
between games, and a ribbon wipe carries you into a game and back out.

| pad | in the menu |
|---|---|
| d-pad / left stick | move (held: repeats) |
| Cross | play the focused game / open a Settings row |
| Triangle | this game's controls editor |
| Square | favourite / unfavourite |
| L2 | search (on-screen keyboard, or type on a USB keyboard) |
| L1 / R1 | Games · Favorites · Recent · Badges · Settings |
| Circle, held | close Ruffle5 |

**Settings**: *Display* — Resolution (4K / 1080p), quality, scale mode, letterbox, alignment;
*Player* — frame rate, Flash Player version, runtime, load behaviour, script time limit, dummy
ExternalInterface, a pretend site for sitelocked games; *Controls & Sound* — controls for all
games (the editor), keyboard for text boxes, USB mouse speed, game volume, menu sounds; *App* — animated
background, screensaver, FPS counter, automatic covers, badges; *Library* — rescan, clear recent;
*About*.

## In a game

| pad | |
|---|---|
| OPTIONS, held (or Esc on a USB keyboard) | the quick menu: resume, restart, volume, controls, take a cover picture, FPS counter, back to games |
| L2 (by default) | the on-screen keyboard (a text box in the game opens it too) |

A game without a picture gets one from its own frame after 8 seconds (Settings › App).

## Controls

The pad does what `src/controls.rs` says, by default:

| pad | Flash |
|---|---|
| d-pad, left stick | arrow keys |
| Cross / Square / Triangle / Circle | Space / Z / X / C |
| L1 / R1 | Shift / Control |
| L3 / R3 | A / S |
| Options | Enter |
| touchpad | the mouse: touch to point, press to click |
| right stick | moves the pointer |
| R2 | left mouse button |
| L2 | the on-screen keyboard (it was the right mouse button before 1.000.020) |

Change them without editing files: **Triangle** on a game opens its controls editor, **Settings ›
Controls & Sound › Controls for all games** the one for every game. Pick a button, then what it
does (Cross sets it, Square adds or removes a key so one button can press up to four, Triangle
resets a row). Leaving saves a file to Custom Mappings written like the shipped presets: a comment,
then only the lines that differ.

The files are layered, generic first: the defaults; `controls.txt` in the title folder (all
comments); `Custom Mappings/_All Games.controls.txt`; the game's own
`Custom Mappings/<movie>.controls.txt` (or a shipped preset such as `dadnme.controls.txt` — Dad 'n
Me: Cross/Triangle = A, Square/Circle = S, R1 = Q); then a `<movie>.controls.txt` beside the SWF.
`ruffle.log` lists every file read and every line applied.

## The log

`/data/ruffle5/ruffle.log` (FTP). While a game plays, every 5 s, in Ruffle-Flash-PS5's format so
the same SWF can be compared in both players:

```
[Game] 5 s: 300 ticks, 300 frames drawn, 0 read back, 0 readbacks failed, root frame Some(42), playing true
[Game] ms per loop: game 1.2, jobs 0.0, draw 2.9 | per frame drawn: readback 0.0 | overlay 0.3, present 11.8
[Game] 3840x2160 4K: 60.0 fps drawn, 60.0 presents/s
```

*game* is Ruffle's tick (ActionScript and the timeline), *jobs* its background work, *draw*
building and submitting the frame, *readback* copying a frame to the CPU (Ruffle5 does that only
for a cover picture; Ruffle-Flash-PS5 does it every frame), *overlay* the pointer, menus and the
1080p scale pass, *present* waiting for a swapchain image and showing it (time spent waiting for
the TV's refresh lands here). 3a's success line is `ok   wgpu on RADV: 3840x2160 …` then
`RADV present OK`.
