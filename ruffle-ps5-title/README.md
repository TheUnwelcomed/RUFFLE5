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
| `src/menu.rs` | the menu: Games / Favorites / Recent / Badges / Flash Browser / Settings; stage, shelf, search, screensaver |
| `src/online.rs` | the Flash Browser tab (*Online* before 1.000.029): lists, pictures and fetching games from the Internet Archive and Flashpoint (its Flash and Shockwave games; Flash Museum until 1.000.032), on threads of its own |
| `net/` | `r5net`, its own crate: HTTPS on the console's sockets (rustls + RustCrypto, Mozilla's roots), the three sources' APIs and pages, a zip reader for Flashpoint's game data; `examples/online-check.rs`, `examples/fetch.rs` |
| `src/game.rs` | one game: loading screen, the player and its options, quick menu, overlay, the frame-time log |
| `src/dir3d.rs` | which Director games have Shockwave 3D (the menu's categories, 1.000.033) |
| `src/navigator.rs` | where a Flash game's files come from: beside the movie, and (a Flashpoint game) by web address from its game data's `content/` (1.000.033), or from Flashpoint's server (1.000.034) |
| `src/fpfetch.rs` | a Flashpoint game's files that are not on the console: asked of Flashpoint's server when the game asks for them, and kept (1.000.034) |
| `src/pretend.rs` | Pretend to be on for Flash and Shockwave games, and its Automatic choice: the site the game's own file names (1.000.035) |
| `src/fsutil.rs` | writing files safely; removing a game's folder with the calls the console allows a title (no `lstat`, 1.000.034), or moving it to `games/.ruffle5-trash` when it refuses (1.000.032) |
| `src/watchdog.rs` | names the step of a game's loop that takes 5 s or more, in ruffle.log (1.000.032) |
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
| `shell/main.c` | the C title shell: `sceAgcInit`, the ICD entry, toasts, the folders, `sceNetInit` for Flash Browser, the resolver's log lines |
| `shell/pad.c` | scePad / AudioOut |
| `Custom Mappings/` | presets shipped in the title's Custom Mappings folder (`dadnme` so far) |
| `controls.txt` | the mapping file's format and the defaults, shipped as a commented example |
| `assets/` | `ruffle-mark.png` (the menu's logo), `logo-anim.swf` (the background) |
| `art/` | `make-art.py`: draws `assets/ruffle-mark.png` and the launch icon `sce_sys/icon0.png` from Ruffle's logo |
| `fonts/` | Inter Regular/SemiBold, Montserrat Medium (SIL OFL 1.1), embedded; `liberation/`: Liberation Sans, Serif and Mono (SIL OFL 1.1) for Shockwave text |
| `build-title.sh` | links the Rust staticlib + RADV archive + runtime into a PPSA title (needs PS5_Vulkan) |
| `tools/online-check.sh` | asks the online sources from the build server, with the same code, and packs the answers |

## On the console

| where | what |
|---|---|
| `/data/homebrew/PPSA99620/games/` | your games (`*.swf`, or one game per sub-folder); also `ruffle5/` on a USB drive; games from Flash Browser, each in its folder with `cover.png` and `online.txt` (a Flashpoint game: its game data's `content/` too) |
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
checked by `dcr-probe --blit-selftest` and `DCR_PROBE_COMPARE=1`, 3 to 4 times faster). Two
film-loop fixes (1.000.026): a film loop assigned to a sprite by Lingo gets the loop's own size when
the sprite is not stretched, as bitmaps already did (DirPlayer kept the score's box of the member
before, so Battle Blitz's opponent portrait drew 2.5 times too large over the fighter grid); and a
blended sprite inside a film loop is laid over the loop's offscreen with its coverage in the alpha
channel, so where the loop is empty the stage shows through it (before, a 30% layer over the cleared
offscreen became an opaque dark box: the selection highlights hid the chosen fighters' portraits).
1.000.032: a film loop on the stage is drawn at the size DirPlayer's WebGL2 renderer gives it (the
loop's own rect, from the cast member); the size worked out from its score's records can be the
whole stage (Trick or Treat Beat!'s Keys_Loop says 432x330 for a 93x50 loop), and the loop was
squeezed by that much: every power in that game (spell, smash, keys, swim) showed as a few dots and
the character vanished while it played; Battle Blitz's fighters and names on the select screen drew
narrowed. Checked with `DCR_PROBE_COMPARE=3` (each stage drawn both ways): only film-loop stages
differ. And a sprite whose foreColor/backColor are set but left at black and white takes the
upright path with a 32-bit picture, as those colours tint nothing (Supersonic RC's whole stage: the
blit went from 11–13 ms to 2 ms a frame on a PC, the same pixels in 630 race frames).
1.000.034: `mouseEnter` / `mouseLeave` / `mouseWithin` go to the front-most sprite under the
pointer that has a script (a behaviour, or its member's script: Director's "active sprite"), not
to the front-most sprite of any kind. Teen Titans Battle Blitz's select screen shows a fighter's
highlight (a sprite with no script) over the fighter's tile on mouseEnter and hides it on
mouseLeave; with the highlight in front, the tile was left on the next frame, the highlight
hidden, the tile entered again — the screen flickered every frame (1.000.032's video). ScummVM's
Director engine also gives these events to one sprite, the front-most that responds to the mouse.
And `the idleHandlerPeriod`, `idleLoadMode`, `idleLoadPeriod`, `idleLoadTag` and
`idleReadChunkSize` (Director 5 movie properties) are kept as a movie sets them and read back
(Virus Raider's startMovie stopped at "Symbol 'idleHandlerPeriod' is not a built-in symbol");
nothing is paced by them.
1.000.028, in the patch: text in a font the movie does not carry (Arial, Times New Roman, Verdana…)
no longer stops the game — DirPlayer asked a browser's canvas for it; off the web `native_text.rs`
lays it out and draws it with the Liberation fonts (`fonts/liberation/`, the advance widths of
Arial, Times New Roman and Courier New), measuring as Chromium's canvas measures (checked against
Chromium: kerning inside words, the "top" baseline, the font box); DirPlayer's own system font
(`charmap-system.png`) is loaded as the web front end loads it; the event loop gets its turn after
each frame, so `mouseEnter` / `mouseLeave` / `mouseWithin` reach the movie; a `go` told to a
film-loop sprite (`tell sprite(n)` … `go 1`) moves that loop's playhead, not the movie's (Battle
Blitz's START button); what a
movie asks of a browser — a message box, `gotoNetPage`, the Flash bridge —
is answered as no browser here can (a notice shown in the game, a refusal the movie sees) instead of
stopping the title; `setPref` / `getPref` keep their files in the game's
saves folder; a file a movie asks for is found whatever the case of its name, as on Windows
(1.000.034: one that is not there goes to the host's fetcher — Flashpoint's server for a
Flashpoint game — and a request that fails is a `net: … not found` line in ruffle.log). A game
from Flashpoint starts as Flashpoint starts it: its launch command is parsed as DirPlayer's web front
end parses it (`parse_launch_command`, a port of its `launchCommand.js`) — the projector's
LeechProtectionRemovalHelp flags and `--do` become Lingo run when the movie loads, `--doBefore`
before, `--go N` after, `--setExternalParam` sets an external param — `the moviePath` is the address
it was published at (DirPlayer's movie path override, so a sitelocked movie runs and the files it
asks for there come from its folder), and every other web address it asks for is read from its
game data (`http://host/path` → `content/host/path`, `set_web_root`); nothing else is fetched.
`dcr-probe/` (at the repository root) runs a movie in DirPlayer on a PC or server and saves the
stage as PNGs — for checking a game before it goes on the console (`DCR_PROBE_LAUNCH`,
`DCR_PROBE_ADDRESS` and `DCR_PROBE_WEBROOT` start it as a Flashpoint game).

### Shockwave 3D (1.000.030)

DirPlayer is pinned to its `cham/w3d-fullscreen` branch (b9b9466: its main plus the Shockwave 3D
work — W3D scenes, cameras, lightmaps, motions, Havok). On the web DirPlayer draws 3D cast members
with WebGL2; the patch adds `rendering_gpu/softgl`, a software WebGL2, and compiles the same
renderer (`webgl2/scene3d.rs`, `mesh3d.rs`, `context.rs`, unchanged but for their imports) a
second time against it:

* `gl.rs` is the part of WebGL2 the renderer calls, with web_sys' names and signatures; `state.rs`
  its objects and state (buffers, vertex arrays, 2D textures with mipmaps, framebuffers with depth
  renderbuffers, depth test, culling, scissor, blending, colour mask, polygon offset, clears, blits,
  `copyTexSubImage2D`); `raster.rs` the pipeline (clipping in homogeneous coordinates, top-left
  fill rule, perspective-correct varyings, depth, blending); `texture.rs` sampling as GL ES 3.0
  §3.8 defines it (level of detail from the coordinates' derivatives, the mipmap filters, the
  wrap modes);
* GLSL is not compiled: the renderer's four programs (the model program, particles, post-process,
  outlines) are recognised by their source and run by Rust ports (`programs.rs`); each source's
  fingerprint is kept, so a DirPlayer update that changes one is reported in the log; the model
  program lights per vertex, as Director's own (IFX) renderer did;
* the CPU compositor (`rendering.rs`) puts each 3D sprite where DirPlayer's WebGL compositor does:
  a pass per camera in the sprite's camera list, overlays, the sprite's ink and blend,
  directToStage sprites last, `world.image` when a script asks for it;
* a draw is set up on the game's thread (vertex shading, clipping, triangle setup) and its pixels
  written later, with the draws before it, in row bands by up to 8 threads (`pool.rs`: the work
  CPUs the CPU layout leaves the game, `src/cpu.rs`); every band keeps the draws' order.

It was checked by recording a game's WebGL calls and replaying them in Chromium's WebGL2 with
DirPlayer's GLSL (`dcr-probe/softgl-replay/`): on a Supersonic RC frame, with depth rounded as the
browser's 16-bit depth buffer rounds it, 99.95% of the pixels are within 32 levels of the browser's
and the mean difference is 0.15 levels (the rest: per-vertex against per-pixel lighting on curved
surfaces, and edge pixels). The software keeps depth as f32, so it does not show the z-fighting
the 16-bit buffer gives the web build (staircase-edged black shapes at a distance).

One DirPlayer fix in the patch, found that way: a lightmap behind an empty first texture slot
(Director's lightmap-only layout) is a lightmap, also when it is named "...LightingMap" — Supersonic
RC's ceiling slabs had their lightmap atlas taken for the diffuse texture and sampled far outside
its islands (black slabs, on the web too) — and whether a mesh's second UV set is pre-centered is
decided by the coordinates its faces use, not by vertices no face uses.

ruffle.log gets a "Shockwave 3D" line every 5 s: 3D sprite frames, triangles and pixels per frame,
and the milliseconds a frame takes (in all, setting up the draws, writing the pixels) on how many
threads.

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
| L2 | search (on-screen keyboard, or type on a USB keyboard); the keyboard shows what is typed (1.000.033) |
| R2 | category (1.000.033): All · Flash · Shockwave · Shockwave 3D · Animations, each with its count |
| L1 / R1 | Games · Favorites · Recent · Badges · Flash Browser · Settings |
| OPTIONS | delete the focused game (1.000.028): Cross deletes, Circle keeps; its saves, cover and play time stay |
| Circle, held | close Ruffle5 |

**Categories** (1.000.033, R2 on Games, Favorites and Recent; All at each start): every game is in
one of Flash, Shockwave, Shockwave 3D and Animations. A Director game is *Shockwave 3D* when one of
its files has a Shockwave 3D cast member — an Xtra member whose record holds the type
`shockwave3d`, as DirPlayer itself tells them (`src/dir3d.rs`; a game in a folder of its own counts
every Director file beside its movie, so Paul Blart's loader counts its 3D movie). The movie's Xtra
list is not used: a loader can list the 3D Xtra and have no 3D member. Each file is read once; the
answers are kept in `/data/ruffle5/shockwave3d.txt` by size and time. *Animations* are the movies
Flashpoint calls animations (its Animations library; `kind = Animation` in the game's online.txt
from 1.000.033 on) and every movie or folder in a folder named `Animations` in the games folder or a
USB drive's `ruffle5` folder. The stage says which (*Shockwave 3D game*, *Flash animation*).

**Flash Browser** (1.000.027 as *Online*, Flashpoint 1.000.028, renamed and moved last before
Settings 1.000.029): games and movies from three lists, R2 switching
between them — the Internet Archive's Flash collection (`softwarelibrary_flash`, the movies
archive.org itself plays with Ruffle: found through its search API, most downloaded first, its
sub-collections left out), Flashpoint Archive's Flash games (1.000.033, `platform:Flash`, A to Z)
and its Shockwave games (`platform:Shockwave`, A to Z), through flashpointarchive.org's search API.
A Flash game Flashpoint's own launcher plays with Ruffle (its `ruffleSupport`) is marked *Ruffle*.
A Flashpoint game comes as its game data (a zip, checked against its SHA-256) when it has one —
`content/<host>/<path>`, the files at the addresses it was published at — and plays as if from its
address; the files it loads by web address are read from `content/` (`src/navigator.rs`), as
Flashpoint's own proxy serves them. One without game data comes as its movie alone, from
Flashpoint's legacy server; the files it loads that are not on the console are asked of that
server when the game asks for them (1.000.034, `src/fpfetch.rs`; before, they were not fetched),
as Flashpoint's own game server does (FlashpointGameServer: the game data, then its `htdocs`, then
`infinity.flashpointarchive.org/Flashpoint/Legacy/htdocs/<host>/<path>`, never a server script),
and kept where they were looked for, so the next time they are on the console. The game waits
while a file comes; each one is a `net:` line in ruffle.log with its size and seconds. Flash Museum was a
source until 1.000.032: its file server refused the movies on the console (403 Forbidden), so it
was taken out; games fetched from it before still play. L2 searches the current list.
Flashpoint's search (FPA-Rust, the archive's search library) looks for each word in the title, an
alternate title, the developer, the publisher and the series, in one platform per list; a search
that finds nothing in its list is asked again without the platform (1.000.033), and the page names
what the archive has and on which platform — Cross goes to the other list when it is Flash or
Shockwave (1.000.032's single Flashpoint list was Shockwave only: no Pyongyang Racer, a Flash
game).
Cross fetches the focused one and plays it, Square only fetches it; it lands in the games folder in a
folder of its own — the movie (and, from the Internet Archive, the small files beside it in the
item: levels, sounds, XML), its picture as `cover.png`, and `online.txt`, which names the source,
the movie to play and the title to show — so it is an ordinary game from then on. Circle cancels a
download. Nothing touches the network until the tab is opened.

A Flashpoint entry is fetched as the archive's own launcher fetches it: its game data — a zip of the
files at the addresses the game was published at, `content/<host>/<path>` — from
download.flashpointarchive.org (download.unstable.life when that fails), checked against the
SHA-256 the archive lists, and unpacked into the game's folder; an entry without game data gets its
movie from the archive's legacy server, with the external casts the movie names. `online.txt` also
keeps how the archive starts the game — its Shockwave Projector command (`launch`: the movie's
address and flags such as `--forceTheExitLock 0` or `--do "…"` Lingo), the address (`url`) and the
folder that holds its files by address (`webroot = content`) — and Ruffle5 hands DirPlayer all
three (see *Shockwave*).

A game stays in the games folder after it is played; Settings › Library › *Keep online games* Off
deletes a game fetched in this run of Ruffle5 when it is left (its saves stay; 1.000.035 — before,
any game from Flash Browser, one fetched in an earlier run too), and OPTIONS on any game's tile
deletes it there and then.

**Pretend to be on** (Settings › Player; 1.000.035 for Shockwave games too): the address a game is
told it was loaded from — a Flash game's `_url` / `loaderInfo.url`, a Director movie's `the
moviePath` and `the movieName` (the files under that address are still read from the game's
folder) — on a chosen site (Newgrounds, Kongregate, Armor Games, Miniclip, Addicting Games, Y8,
Shockwave.com), or **Automatic** (`src/pretend.rs`): the movie is read — a Flash movie decompressed,
a Director movie's resources — and the site whose domains it names most often (as whole names:
"y8.com" in "www.y8.com", not in "heavy8.com") is the one the game is told it runs on; a movie that
names none is told nothing. A game whose source gave it an address (a Flashpoint game's online.txt
`url`) keeps that. Read on a PC: Paul Blart's gameloader.dcr names Miniclip 10 times, its
paul_blart.dcr Shockwave.com 12 times; Trick or Treat Beat!, Battle Blitz and Supersonic RC name
none of the sites. A Director movie is given the http:// form of the address (Miniclip's game
loader tests `the moviePath contains "http:"`). With an address, a movie the first one goes to keeps
its own name in that folder (DirPlayer had kept the first movie's name: Battle Blitz, whose
wrapper goes to game.dcr, stopped at its title); `getStreamStatus` and `netDone` by address find
what `preloadNetThing` asked for.

Removing on the console (1.000.034). The console refuses `lstat` to Ruffle5 in the games folder
and in `/data/ruffle5` ("Operation not permitted"), while `stat`, listing a folder, `unlink`,
`rmdir` and `rename` work (1.000.032's `files: removing in …` lines). Rust's `remove_dir_all` asks
`lstat` first, and 1.000.032 and 1.000.033 told a folder from a file, and whether a removal had
worked, with `lstat` too, so a refused `lstat` read as "gone": Delete and Keep online games Off said
`menu: deleted …` and left the game there, and nothing went to the trash. A game's folder is now
removed by listing it — an entry is a folder when the listing says so, or when it can be listed —
with `unlink` for each file and `rmdir` for each folder, deepest first, and it is gone only when
`stat` no longer finds it (`src/fsutil.rs`). What the console still refuses moves into
`games/.ruffle5-trash/` (1.000.032): the game leaves the menu (a folder whose name starts with a dot
is never listed), the files stay on the disk until they can be removed. Each start tries to empty
it and says in ruffle.log what is left and why; you can also delete the folder over FTP. The log's
`files: removing in …` lines say what removing a file, an empty folder and a folder of files
answers in the games folder and in `/data/ruffle5`, whether renaming works, and whether `lstat`
is refused.

HTTPS is `net/`'s: rustls 0.23 with RustCrypto's algorithms (no C, no assembly) and Mozilla's root
certificates compiled in, on the console's own sockets and resolver; the title calls `sceNetInit`
first (a title's sockets answer EACCES before it). Names are looked up by the kit's `evo_dns.c`
(`getaddrinfo` over `sceNetResolver*`, the DNS the console's network settings name), compiled into
the eboot; its `dns:` lines go to ruffle.log. 1.000.027 and 1.000.028 linked the platform layer's
`ps5_getaddrinfo` in its place (PS5_Vulkan's link recipe binds it with `--defsym`), which answers
every lookup "Non-recoverable failure in name resolution"; `build-title.sh` V10 takes that binding
out and checks the image for the resolver. Requests say `User-Agent: Ruffle5/<version>
(PlayStation 5 Flash player; +https://github.com/TheUnwelcomed/RUFFLE5)`. `tools/online-check.sh`
runs the same code on the build server against the three sites and packs what they answered.

**Settings**: *Display* — Resolution (4K / 1080p), quality, scale mode, letterbox, alignment;
*Player* — frame rate, Flash Player version, runtime, load behaviour, script time limit, dummy
ExternalInterface, a pretend site for sitelocked games; *Controls & Sound* — controls for all
games (the editor), the on-screen keyboard (its size and place for every game), keyboard for text
boxes, USB mouse speed, game volume, menu sounds; *App* — animated
background, screensaver, FPS counter, automatic covers, badges; *Library* — rescan, clear recent,
keep online games; *About*.

## In a game

| pad | |
|---|---|
| OPTIONS, held (or Esc on a USB keyboard) | the quick menu: resume, restart, volume, controls, keyboard, take a cover picture, FPS counter, back to games |
| L2 (by default) | the on-screen keyboard (a text box in the game opens it too) |
| on the keyboard | d-pad picks a key, Cross types it, Square deletes, Triangle types a space, L1 moves it on round the screen, R1 across it, Circle closes it |

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

### The on-screen keyboard (1.000.031)

Its own settings, per game or for every game, like the rest of the controls: **Settings › Controls
& Sound › On-screen keyboard** (every game), **Keyboard for this game** in the quick menu, or the
**On-screen keyboard ›** row at the end of either controls editor.

| setting | |
|---|---|
| Made for | ready-made settings for kinds of game (below); "Your own" when the rows say something else |
| Size | Large (804 x 594 of the 1920 x 1080 screen, as before), Medium (659 x 496), Small (518 x 396), Mini (384 x 240: no title, no button hints) |
| Opens at | bottom, bottom left, left, top left, top, top right, right, bottom right |
| The game | under the keyboard (it keeps the whole screen) or beside it (while the keyboard is open the game is shown smaller in the part of the screen the keyboard leaves free, none of it hidden) |
| Background | solid, 80, 60 or 40% (the game shows through where it is under the keyboard) |

| made for | size, place, the game, background |
|---|---|
| Typing a name or a code | Large, bottom, under, solid (the keyboard as before) |
| RPG and turn-based | Small, bottom right, beside, solid: the map, the menus and the question stay in view |
| Typing games | Medium, bottom, beside, solid: the words to type above the keys |
| A key now and then | Mini, top right, under, 60%: one key from time to time, the game keeps the screen |

The page shows a map of the screen with the keyboard and the game where they will be; **Square**
shows the keyboard at full size where it will open (in a game, the paused game moves out of its way
as it will), where L1 / R1 move it, Left / Right size it, Up / Down set its background and Triangle
puts the game under it or beside it. With the game beside it, a Flash game's stage is shown without
Ruffle's letterbox around it, so it gets as much of the free part as its shape allows; a Director
stage is fitted the same way.

The menu's search keyboard (1.000.033) has settings of its own, Settings › Controls & Sound ›
Search keyboard: its size, where it opens and its background, starting from the keyboard for all
games (so a change there reaches it until it is given its own), kept in `Custom Mappings/_Menu
Search.keyboard.txt`. It shows what is typed in its head, with a caret (Mini, which has no head: in
a box above or below it).

The files are layered, generic first: the defaults; `controls.txt` in the title folder (all
comments); `Custom Mappings/_All Games.controls.txt`; the game's own
`Custom Mappings/<movie>.controls.txt` (or a shipped preset such as `dadnme.controls.txt` — Dad 'n
Me: Cross/Triangle = A, Square/Circle = S, R1 = Q); then a `<movie>.controls.txt` beside the SWF.
`ruffle.log` lists every file read and every line applied.

## The log

`/data/ruffle5/ruffle.log` (FTP). Since 1.000.032 it keeps the runs before (each starts with a
`==== Ruffle5 started <date> ====` line); past 1 MB the oldest are cut, keeping the last 512 KB.
A step of a game's loop that takes 5 s or more is named (`watchdog: <game>: still in <step> after
<n> s`, then how long it took): a game that hangs says where. While a game plays, every 5 s, in
Ruffle-Flash-PS5's format so the same SWF can be compared in both players:

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
