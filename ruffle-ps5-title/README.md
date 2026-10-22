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

* **3a — `cargo build` with no features (default):** bring up wgpu on RADV and clear the display to
  an animated colour for 6 s. Proves the static-RADV + VK_KHR_display + wgpu seam in a title. This is
  the first server build; it exercises only `src/gpu.rs`.
* **3b — `--features ruffle`:** load `/data/ruffle/movie.swf` and play it through `ruffle_render_wgpu`
  (`run_ruffle` in `src/lib.rs`). Added once 3a shows colour on the TV.

## Files

| file | what |
|---|---|
| `src/gpu.rs` | the static-RADV → wgpu bring-up (the novel piece) |
| `src/lib.rs` | `ruffle_ps5_main`: 3a clear-screen; 3b the app — the menu, a Ruffle player per game, back |
| `src/menu.rs` | the menu: Games (stage + shelf of tiles) and Settings; design after ProsperoStore's home screen |
| `src/ui.rs` | the 2D renderer the menu draws with: one instanced SDF-quad pipeline, text from the bundled fonts |
| `src/shell.rs` | the shared display: the swapchain every player's `RenderTarget` points at; the menu presents to it directly |
| `src/swfinfo.rs` | a SWF's header (version, size, fps, frames) without inflating the movie |
| `src/input.rs` | the DualSense as keyboard and mouse: buttons → keys/clicks per the controls, touchpad and right stick → pointer |
| `src/controls.rs` | the mapping as data: defaults, `/data/ruffle/controls.txt`, `/data/ruffle/<movie>.controls.txt` |
| `src/cursor.rs` | the pointer drawn over Ruffle's frame (a `RenderTarget` wrapping `SwapChainTarget`) |
| `src/audio.rs` | Ruffle's mixer → AudioOut: mixed in f32, clamped to s16 (mixing into i16 wraps), 1024-frame grains, a 10-second score line in the log |
| `shell/pad.c` | scePad / AudioOut calls, the hardware-verified sample layout |
| `shell/libc_fixes.c` | a lexical `realpath` (the console's is EPERM in a title) |
| `controls.txt` | the mapping file's format and the defaults, shipped in the title folder as a commented example |
| `fonts/` | Inter Regular/SemiBold, Montserrat Medium (SIL OFL 1.1), embedded in the title |
| `games-README.txt` | shipped as `games/README.txt`: what goes in the games folder |
| `THIRD_PARTY.md` | what the title is built from, and the menu's design credit |
| `presets/*.controls.txt` | per-game mappings shipped in the title folder (`dadnme` so far) |
| `shell/main.c` | the C title shell: `sceAgcInit(8)`, exports the ICD entry, toasts + log |
| `Cargo.toml` | ruffle_core / ruffle_render_wgpu / wgpu 30 / ash 0.38, Ruffle rev pinned |
| `build-title.sh` | links the Rust staticlib + RADV archive + runtime into a PPSA title (needs PS5_Vulkan) |

## The menu

Ruffle5 opens on a menu of **`/data/homebrew/PPSA99620/games/`**: every `.swf` there, and the
first `.swf` in each sub-folder (a game with its own files). A `<name>.png` beside a file, or a
`cover.png` / `icon.png` in a folder, is its picture; without one a tile gets a colour from its
name and its initials. The focused game is shown big on the stage with what its header says (SWF
version, size, frame rate, frames) and whether a controls preset ships for it.

| pad | in the menu |
|---|---|
| d-pad / left stick ← → | focus a game (↑ ↓ jump by four) |
| Cross | play it |
| L1 / R1 | Games ↔ Settings |
| Circle, held | close Ruffle5 |
| **OPTIONS, held (in a game)** | back to the menu |

Settings shows the version (1.000.020) and the author. The layout, type and motion follow
ProsperoStore's home screen (see `THIRD_PARTY.md`).

## Controls

The pad does what `src/controls.rs` says, by default:

| pad | Flash |
|---|---|
| d-pad, left stick | arrow keys |
| Cross / Square / Triangle / Circle | Space / Z / X / C |
| L1 / R1 | Shift / Control |
| L3 / R3 | A / S |
| Options | Enter |
| touchpad | the mouse: touch to point (the pad's surface is the screen), press to click; a pointer is drawn, hidden 3 s after it stops |
| right stick | moves the pointer |
| R2 / L2 | left / right mouse button |

Every game wants different keys, so the mapping is layered, generic first: the defaults, then
`controls.txt` in the title folder (shipped, all comments), `/data/ruffle/controls.txt` (yours, all
games), `<movie>.controls.txt` in the title folder (a shipped preset — `presets/` here, e.g.
`dadnme.controls.txt` for Dad 'n Me: Cross/Triangle = A, Square/Circle = S, R1 = Q), then yours
for that movie: `/data/ruffle/<movie>.controls.txt` or `<movie>.controls.txt` beside it in the
games folder. A button may press several keys at once
(`cross = space, enter`) or click (`cross = click`); sticks can be `dpad`, `mouse` or `off`.
`ruffle.log` lists every file read, every line applied and the mapping in effect.

## Progress

On the TV: toasts. On disk: `/data/ruffle/ruffle.log` (FTP). 3a's success line is
`ok   wgpu on RADV: 3840x2160 …` then `RADV present OK`.
