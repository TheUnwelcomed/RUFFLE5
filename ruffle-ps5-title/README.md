# ruffle-ps5-title — Ruffle as a native PS5 title (milestone 3)

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
| `src/lib.rs` | `ruffle_ps5_main`: 3a clear-screen, 3b Ruffle player |
| `shell/main.c` | the C title shell: `sceAgcInit(8)`, exports the ICD entry, toasts + log |
| `Cargo.toml` | ruffle_core / ruffle_render_wgpu / wgpu 30 / ash 0.38, Ruffle rev pinned |
| `build-title.sh` | links the Rust staticlib + RADV archive + runtime into a PPSA title (needs PS5_Vulkan) |

## Progress

On the TV: toasts. On disk: `/data/ruffle/ruffle.log` (FTP). 3a's success line is
`ok   wgpu on RADV: 3840x2160 …` then `RADV present OK`.
