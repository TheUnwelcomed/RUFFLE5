# Third-party notices — Ruffle5 (ruffle-ps5-title)

| Work | Role | License |
| --- | --- | --- |
| [Ruffle](https://github.com/ruffle-rs/ruffle) | the Flash player (`ruffle_core`, `ruffle_render_wgpu`) | MIT / Apache-2.0 |
| [wgpu](https://github.com/gfx-rs/wgpu) v30.0.1 (+ `tools/wgpu-hal-ps5-surface.patch`) | the GPU API over RADV | MIT / Apache-2.0 |
| [PS5_Vulkan](https://github.com/BlackBearReloaded/PS5_Vulkan) | the static RADV archive, the platform layer, `radv-link.sh` | per its repository |
| [ProsperoStore](https://github.com/darkxex/ProsperoStore) | the menu's layout and design language are modelled on its home screen (`src/app/store.cpp`: the top bar, stage, shelf of 16:9 tiles, hint row, hold-to-close bar — reimplemented in Rust in `src/menu.rs`, no code copied); the three fonts below are the ones it ships. The focused cover's breathing glow and ring follow its focus animation (reimplemented in `src/widgets.rs`) | GPL-3.0-or-later |
| [Inter](https://github.com/rsms/inter) Regular, SemiBold (`fonts/`) | body text, labels | SIL OFL 1.1 (`fonts/Inter-LICENSE.txt`) |
| [Montserrat](https://github.com/JulietaUla/Montserrat) Medium (`fonts/`) | headlines | SIL OFL 1.1 (`fonts/Montserrat-LICENSE.txt`) |
| [Ruffle-Flash-PS5](https://github.com/ZiZc3/Ruffle-Flash-PS5) by ZiZc3 | ported to Ruffle5 (rewritten, in Rust or C as noted in each file's header): the USB keyboard/mouse loading (`shell/hid.c`, `src/hid.rs`), the sound's own CPU core (`src/cpu.rs`), the on-screen keyboard (`src/keyboard.rs`), the in-game quick menu (`src/quickmenu.rs`), saves on disk (`src/saves.rs`), Ruffle's UI hooks (`src/ps5ui.rs`), cover pictures from a game's frame (`src/covers.rs`), badges (`src/badges.rs`), the library — favourites, recent, play time, session recovery (`src/library.rs`), menu sounds (`src/sounds.rs`), the Player settings it exposes, and its `[Game]` frame-time log format (`src/game.rs`) | GPL-3.0 |
| Ruffle's logo, [`rs.ruffle.Ruffle.svg`](https://github.com/ruffle-rs/ruffle/tree/master/desktop/packages/linux) | the shapes `art/make-art.py` draws `assets/ruffle-mark.png` and the launch icon `sce_sys/icon0.png` from (the icon adds a "5" in the same ribbon style); the menu's colours are Ruffle's | MIT / Apache-2.0 (the Ruffle repository) |
| [ruffle.rs](https://github.com/ruffle-rs/ruffle-rs.github.io) logo animation (`assets/logo-anim.swf`) | the menu's moving background, played by Ruffle itself; taken unchanged from the website's repository, which carries no licence file of its own — it is the Ruffle project's artwork, credited here, and is replaced or removed on the project's request | the Ruffle project's |
| [ruffle_video_software](https://github.com/ruffle-rs/ruffle) | FLV video (H.263, VP6, Screen Video) | MIT / Apache-2.0 |
| [ab_glyph](https://github.com/alexheretic/ab-glyph), [png](https://github.com/image-rs/image-png), [flate2](https://github.com/rust-lang/flate2-rs), [url](https://github.com/servo/rust-url) | glyph rasterising, pictures (reading and writing), SWF header inflation, the pretend site's URL | Apache-2.0 / MIT |

Ruffle5 itself is GPL-3.0-or-later (the repository's `LICENSE`).
