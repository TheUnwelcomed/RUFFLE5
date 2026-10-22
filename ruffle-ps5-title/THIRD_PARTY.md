# Third-party notices — Ruffle5 (ruffle-ps5-title)

| Work | Role | License |
| --- | --- | --- |
| [Ruffle](https://github.com/ruffle-rs/ruffle) | the Flash player (`ruffle_core`, `ruffle_render_wgpu`) | MIT / Apache-2.0 |
| [wgpu](https://github.com/gfx-rs/wgpu) v30.0.1 (+ `tools/wgpu-hal-ps5-surface.patch`) | the GPU API over RADV | MIT / Apache-2.0 |
| [PS5_Vulkan](https://github.com/BlackBearReloaded/PS5_Vulkan) | the static RADV archive, the platform layer, `radv-link.sh` | per its repository |
| [ProsperoStore](https://github.com/darkxex/ProsperoStore) | the menu's layout and design language are modelled on its home screen (`src/app/store.cpp`: the top bar, stage, shelf of 16:9 tiles, hint row, hold-to-close bar — reimplemented in Rust in `src/menu.rs`, no code copied); the three fonts below are the ones it ships | GPL-3.0-or-later |
| [Inter](https://github.com/rsms/inter) Regular, SemiBold (`fonts/`) | body text, labels | SIL OFL 1.1 (`fonts/Inter-LICENSE.txt`) |
| [Montserrat](https://github.com/JulietaUla/Montserrat) Medium (`fonts/`) | headlines | SIL OFL 1.1 (`fonts/Montserrat-LICENSE.txt`) |
| [ab_glyph](https://github.com/alexheretic/ab-glyph), [png](https://github.com/image-rs/image-png), [flate2](https://github.com/rust-lang/flate2-rs) | glyph rasterising, pictures, SWF header inflation | Apache-2.0 / MIT |

Ruffle5 itself is GPL-3.0-or-later (the repository's `LICENSE`).
