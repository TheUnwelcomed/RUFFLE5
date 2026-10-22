# Ruffle on the PS5 — plan (revised 2026-10-20: a standalone Games-row title; PS5_RetroArch's platform layer, the Vita ports' GL renderer)

Goal: [Ruffle](https://github.com/ruffle-rs/ruffle) (the Flash Player emulator, Rust) as a native
title on a jailbroken PS5: a tile under **Games**, launched from the home screen, built with as much
of [PS5_RetroArch](https://github.com/mihawk-99/PS5_RetroArch), [Jelly5](https://github.com/02dnot/jelly5)
and their common foundations as can be reused. Everything below was read out of the trees and the
SDK's symbol tables; the things not yet verified on hardware are marked.

## 1. The decision

A **standalone title**, not a libretro core. PS5_RetroArch is the template, not Jelly5, because:
- it is already a game-category title (`param.json`: `applicationCategoryType: 0`, `attribute: 0`;
  Makefile `APP_CATEGORY ?= game`) — Jelly5's `65536` + `attribute 536870912` is what puts a tile
  under Media;
- it already renders through **Vulkan on the PS5's GPU** (RADV, linked into the title) with audio,
  four controllers and the touchpad — the exact integration Ruffle's wgpu renderer needs, which was the
  open risk of the first plan;
- both are built on the same ps5-payload-dev SDK and the same BlackBearReloaded native-title tooling
  (ProsperoLight / ps5-native-app-boilerplate), so nothing from the Jelly5 side is lost: the Rust
  toolchain kit (milestone 1) applies unchanged.

Licensing: PS5_RetroArch, ProsperoLight, Jelly5 and PS5_Vulkan's own code are GPL-3.0-or-later;
Ruffle is MIT/Apache-2.0; Mesa/RADV is MIT. The title is GPL-3.0-or-later.

## 2. What is reused, from where

| piece | taken from | what it is |
|---|---|---|
| compiler, sysroot, loader stubs | ps5-payload-dev SDK v0.43 (+ pacbrew where needed) | `prospero-clang` (`-target x86_64-sie-ps5`), `libc.a`, the SCE `.so` stubs, `elf_x86_64.x` |
| Rust toolchain for the target | **this project** (`ruffle-ps5-m1`) | `x86_64-sie-ps5.json`, cargo config, the C shim, the linker script, the import check |
| title build tail | PS5_RetroArch `tooling/native/` (= BlackBearReloaded boilerplate; Jelly5 vendors the same) | PIE → PS5 module, fake-signed SELF, PRX import stubs, `runtime/libc.prx`, `ps5-pie.ld`; the Makefile with `APP_CATEGORY=game`, `TITLE_ID`, FTP deploy (`tools/build-title.sh`) |
| Vulkan driver | PS5_Vulkan via PS5_RetroArch | RADV built by PS5_Vulkan's `tools/build-radv.sh release` (`.deps/native/radv-release`, "what titles ship"), linked into the title by its `tools/radv-link.sh`; `src/radv_icd_ps5.c` exposes `vkGetInstanceProcAddr` as an ordinary symbol ("the Vulkan driver is linked, not loaded": a title cannot dlopen) |
| display | PS5_RetroArch `src/display.cpp`, `src/display_modes_ps5.cpp` (835 lines) | VideoOut open/flip/vblank, Vulkan instance + display modes |
| input | PS5_RetroArch `src/input_ps5.cpp` (1632 lines) | `scePad` with the hardware-verified 120-byte sample layout, four controllers, touchpad, vibration |
| audio | PS5_RetroArch `src/audio_ps5.cpp` (437 lines) | `sceAudioOut` output |
| memory, crash reports, libc shims | PS5_RetroArch `src/memory_ps5.cpp`, `crash_report.cpp`, `libc_shims.c` | direct-memory allocator, crash dumps, the title's libc gap fillers |
| picker UI | PS5_RetroArch `frontends/picker/picker.cpp` + BlackBearReloaded's Vulkan UI kit (`hui::ui`, pulled by its build) | the frontend chooser, drawn with Vulkan, cards + hints — becomes the SWF list |
| WebUI (optional, later) | PS5_RetroArch `daemon/webui_daemon.cpp`, `webui/` | local HTTP service on :6769 for uploading content from a phone/PC |
| the player | Ruffle `core`, `render/wgpu`, `video/software`, `swf` | the emulator; `exporter/` (608 lines) is the model for a frontend without winit/egui |

Not reused: Jelly5's AGC rendering engine (Vulkan replaces it), Ruffle's desktop frontend (winit,
egui, cpal, tokio, rfd, webbrowser).

## 2b. Ruffle already runs on a console — what the Vita ports give us

Two existing projects put Ruffle on the PS Vita (found via KiddRwxSsj/ruffleloader, a Lua launcher
for them). Both are Rust frontends over SDL2 with a **GL renderer**, built with std-from-source for a
tier-3 target — the same shape as this plan.

| project | what it is | licence | reuse |
|---|---|---|---|
| `ruffle_render_glow` (inside both) | a Ruffle `RenderBackend` on OpenGL ES 2/3 through `glow` — Ruffle's WebGL renderer made native (2004 lines + shaders, Context3D stubbed) | MIT OR Apache-2.0 (authors: Ruffle LLC) | **take as is**: a GL path that needs no wgpu/Vulkan adoption |
| [Fancy2209/ruffle4consoles](https://github.com/Fancy2209/ruffle4consoles) | the original Vita/Switch glue: 751-line `main.rs` (SDL2 window, GLES 2.0 context, game controllers, event loop), small audio/storage/ui/log backends | **no licence file** | design reference; ask before copying |
| [ibrahim-dogan/vita-flash-emulator](https://github.com/ibrahim-dogan/vita-flash-emulator) (RuffleVita) | a complete product on top of it: library with thumbnails, Explore/download, per-game button mapping to keys/mouse, pause menu, settings, touch as mouse, background loading, local fallback for portal loaders, AOT-precompiled Box2D; 10.8k lines, `cfg(target_os = "vita")` isolated in `platform.rs`, "a desktop development build runs the same code" | **no licence file** ("Mixed" on gamebrew) | the UX blueprint for milestone 4; its code only with the author's permission |

What this changes: the PS5 has both GL and Vulkan now. pacbrew's Mesa gives EGL + OpenGL/GLES via
**zink over RADV**, and pacbrew's SDL2 is patched to open GL (and Vulkan) windows on it; inside a
title, PS5_RetroArch's own SDL2 frontend links BlackBearReloaded's **ps5-opengl** (GL 3.3 core on
AGC, `libPS5OpenGLCore33.a`). So the glow renderer has two ways onto the PS5's GPU, both with
existing code, and the Vita precedent says the frontend over SDL2 + glow is enough for a playable
Flash player. wgpu-on-Vulkan stays as the later upgrade (filters, blend modes and Stage3D are where
the glow renderer falls short, per RuffleVita's notes).

## 3. Architecture

```
PPSA-xxxxx/eboot.bin  (category: game)
├── C++ shell (PS5_RetroArch's platform layer, trimmed)
│   main → VideoOut + RADV instance/device/VK_KHR_display swapchain (display*.cpp)
│        → scePad polling (input_ps5.cpp) → mapping → Ruffle mouse/keyboard events
│        → sceAudioOut thread (audio_ps5.cpp) pulling PCM from the Rust mixer
│        → picker (picker.cpp + UI kit) listing /data/ruffle/*.swf
│        → libruffle_ps5.a  (C ABI)
└── Rust staticlib `ruffle_ps5` (built with the milestone-1 target)
    init(vkGetInstanceProcAddr, instance, physical device, device, queue family)
        → wgpu adopts them: wgpu_hal::vulkan::Instance::from_raw / Adapter::device_from_raw
    load(path) → PlayerBuilder (as exporter does): WgpuRenderBackend::for_offscreen,
        AudioBackend = Ruffle's mixer → ring buffer the shell drains,
        NavigatorBackend = local files (+ sockets later), StorageBackend = /data/ruffle/save,
        UiBackend = the shell's events, VideoBackend = ruffle_video_software
    frame(swapchain VkImage, dt) → renders into the shell's image wrapped as a wgpu texture
        (Device::texture_from_raw); the shell presents it
```

Renderer: milestone 2 and 3 use `ruffle_render_glow` on a GL context (see §2b); the wgpu/Vulkan
layout below is the later upgrade path and is kept here for that reason.

Why the shell owns Vulkan and presentation: that code exists and runs on the console today
(RetroArch's display path, RADV's VK_KHR_display on VideoOut); the Rust side then needs no new
Vulkan code beyond adopting the handles and wrapping an image, which are wgpu-hal's documented entry
points for exactly this. The one point to verify in milestone 2 is that wgpu 30 (Ruffle's pin)
accepts RADV-on-PS5's device as an adapter and renders into the wrapped image.

Imports: a title binds its imports to the system PRXs through the tail's generated stubs (how
Jelly5 gets pthreads, sockets and mmap), plus `runtime/libc.prx` for libc. The milestone-1 payload's
95 imports are all SDK-exported; the title variant gets the same import check against the title's
stub set.

## 4. Milestones

1. **Rust std on the console** — built (see §5), run on the console pending.
2. **Flash on the TV as a payload** (fast, nothing to package): a ruffle4consoles-shaped frontend
   — SDL2 (pacbrew's, PS5 video/audio/pad driver) + `ruffle_render_glow` on SDL's GL context (EGL +
   zink over RADV, loaded at run time as pacbrew apps do) + Ruffle core — built with the
   milestone-1 target, sent with `nc` like the hello payload. First SWF running; the renderer,
   audio, pad and the Ruffle build for this target all proven before any title work.
3. **The title**: the same Rust code inside PS5_RetroArch's game-category tail with its platform
   layer; GL inside a title via ps5-opengl (what RetroArch's SDL2 frontend links) or zink + RADV
   linked statically (what RetroArch links for Vulkan) — decided by what links and runs; a tile under
   Games.
4. **The product** (RuffleVita's blueprint): library with thumbnails, per-SWF button mapping to
   keys/mouse, pause menu, settings (scaling, letterbox, audio delay), touchpad as pointer, WebUI
   upload from a phone, local fallback for portal loaders. Later: wgpu on RADV for the effects the
   glow renderer lacks.

## 5. Milestone 1 status (2026-10-20)

Built on the Bytesized host (Ubuntu 22.04, no root): LLVM 18.1.8 tarball in `~/llvm18`, rustup
nightly 1.101 (2026-10-19) + rust-src, SDK v0.43 in `~/ps5-payload-sdk`. Four things the kit did not
have on the first try, all now in it: cargo's `-Zjson-target-spec` (new gate for `.json` targets),
no `_comment` key in a target spec (rustc rejects unknown keys), one cast in std's `readdir`
(`d_fileno as u64` — std assumes FreeBSD 12's 64-bit inode; the libc crate's freebsd11 ABI has 32;
`build.sh` re-applies it after a rustup update), empty `libkvm/libmemstat/libprocstat/libdevstat`
archives (the libc crate names them; nothing calls into them), and a `dl_iterate_phdr` stub (std's
backtrace symbolizer; no objects to walk). Result: `ps5-rust-hello`, 6.3 MB, `DYN`, DT_NEEDED
`libkernel_web.sprx`, `libSceLibcInternal.sprx`, `libSceNet.sprx`, 0 TLS sections, **import check:
all 95 imports exported by the SDK's libraries**. Not yet run on the console (the server and the PS5
are on different networks; the ELF goes home by scp and in by `nc`/a payload sender/websrv from USB).

Facts established on the way (all checked, none assumed): the SDK's `libkernel`/`libSceLibcInternal`/
`libSceNet` stubs and `libc.a` export 4803 symbols; of the 227 libc functions std's unix/FreeBSD code
calls, 7 are missing (4 matter, in the shim); the SDK's `<sys/stat.h>` is the FreeBSD-11 layout, so
the libc crate is built with `--cfg libc_unstable_freebsd_version="11"` and its versioned names
(`fstat@FBSD_1.0` …, 16 of them) are `.symver` wrappers in the shim over the console's plain
functions — verified with the SDK's lld to bind correctly and call through the GOT; every math.h
function is exported by `libSceLibcInternal`, so `libm` needs no pacbrew.

## 5c. Title packaging — verified on hardware (2026-10-20)

`ps5-rust-title` (kit): Jelly5's vendored BlackBearReloaded tail (converter + signer built static
here, `ps5-pie.ld`, CRT, `malloc_shim`, `evo_dns`, clean-room `libc.prx`), Jelly5's exact link line,
`param.json` with `applicationCategoryType 0` / `attribute 0`, title `PPSA99610` "Rust Hello".
The C stand-in build (no Rust: the library replaced by C that toasts and writes the report) was
installed from `/data/homebrew/PPSA99610` and **ran on the console: both toasts shown** ("Rust Hello
(stand-in) is running on this PS5", "… 1 ok, 0 failed (report written)"). So: the game-category
param, the tile, the loader profile, the CRT/allocator/resolver runtime and `sceKernelSendNotification`
all work for a title that is not Jelly5. Two fixes made on the way, both in the kit: the generated
import table must use weak externs (the compiler folds `&f ? …` away otherwise, and the converter
then refuses an image without `.data.rel.ro`); `evo_dns.c` needs an `evo_boot_log` definition.
Next: the same title with `libps5_rust_hello.a` (built on the Bytesized host) in the library's place.

## 5d. Milestone 1 reached on the console (2026-10-20, 18:34 MDT)

`PPSA99610` "Rust Hello" with `libps5_rust_hello.a` inside, launched from the Games row:
**"Rust on PS5: 8 ok, 2 failed, 0.2 s"**, report at `/app0/ps5-rust-hello/report.txt`
(= `/data/homebrew/PPSA99610/ps5-rust-hello/` over FTP). What passed, in a title: std's threads
(4 × 10,000 under a Mutex + Condvar = `_umtx_op`), clocks (`clock_nanosleep` → `nanosleep`,
100.16 ms), HashMap's `arc4random` seed, a 64 MB `Vec`, file write/metadata/read/remove with
**`metadata().len()` = 1234 = the console's own `st_size`** (the FreeBSD-11 `struct stat` choice is
right), `read_dir` through the shim's `getdirentries` reader, `current_dir` = `/app0`,
`parallelism = 16`. `current_exe` is `NotFound` (sysctl `KERN_PROC_PATHNAME` is not answered —
harmless). The two failures: the C-side `readdir` count in the test helper read 0 where Rust read 1
(test helper, under investigation), and `connect 1.1.1.1:443` = **EACCES**: BSD sockets in a title
need `sceNetInit()` first (PS5_RetroArch's scraper does the same) — the shell now calls it.

What a title needs beyond a payload, all found on this console and all in the kits now:
1. No raw `syscall` instructions (`SYSTEM_ILLEGAL_FUNCTION_CALL` 0xa002030a): the SDK's `libc.a`
   has them in `mman.o`, `mount.o`, `syscalls.o`; the title links `libc.a` without those members and
   the shim's `PS5_TITLE` section gives libkernel bodies to everything std reaches (`clock_nanosleep`,
   `openat`, `unlinkat`, `pipe2`, `fstatat`, `futimens`, `utimensat`, …), ENOSYS to the rest.
   `build-title.sh` disassembles the image and refuses any `syscall`.
2. The console's own libc misbehaves for a title (as PS5_RetroArch's `platform_wraps.c` records):
   `opendir` refused → shim reads with `getdirentries`; `getcwd` jumps to address 0 (needs the
   system-process `__getcwd`) → shim answers `/app0`; `realpath` EPERM (std's `canonicalize` errors).
3. Imports may only come from the libraries the title links (`libkernel`, `libSceLibcInternal`,
   `libSceNet`): weak references count too (std's `posix_spawn_file_actions_addchdir*`), and the
   converter refuses anything else.
4. `sceNetInit()` before the first socket.
5. Link details from Jelly5's line: `libpthread.a` for libunwind's dependent-library note, the
   import table's externs weak (else folded away and the image lacks `.data.rel.ro`), `evo_boot_log`
   defined for `evo_dns.c`, `killpg` for std's `Child::kill`.

## 5e. v7 on the console (2026-10-20, 18:43 MDT): 9 ok, 1 failed — and what the two leftovers are

v7 (eboot `6533fd69…`, `sceNetInit()` in the shell) ran clean through every check but `net`. Facts
from `report.txt` plus the sources it was checked against:

- **fs-readdir** passed the relaxed check but told the real story: `Rust read_dir()=1 []`, `C readdir=0`.
  One iterator item with no name is one `Err` — this nightly's std uses `readdir()` and reports an
  error when it returns NULL with errno set (`library/std/src/sys/fs/unix.rs`, `ReadDir::next`). So
  the shim's reader failed on its first `getdirentries(fd, buf, 16384, &base)` on `/app0/ps5-rust-hello`.
  PS5_RetroArch's title reads directories with `getdents(fd, buf, 64 KiB)` — "mounted title directories
  need a larger read than the synthetic root" (`platform/src/directory.c` in Mihawk's payload-SDK fork,
  `github.com/mihawk-99/PS5_PayloadSDK`, revision b5efad52). **v8 reads the same way.**
- **net** still `Permission denied (os error 13)` with `sceNetInit()` done. std creates every socket
  as `socket(family, type | SOCK_CLOEXEC, 0)` on FreeBSD, then `SO_NOSIGPIPE`, `ioctl(FIONBIO)`,
  `connect`, `poll` (`sys/net/connection/socket/unix.rs`). PS5_RetroArch's title creates its sockets
  with `socket(AF_INET, SOCK_STREAM, 0)` (`src/webui_link.cpp`, `src/scraper_http.cpp`) and they work;
  its `param.json` attributes are the same as ours (0 / 0 / 524352). Which of the four calls the
  console refuses is not yet in any log, so **v8 does two things**: `--wrap=socket`/`socketpair` with the
  flags taken off and applied by `fcntl` (shim PS5_RUST_SHIM_V2, title mode), and a C probe
  (`ps5_c_net_probe`) that runs each call on its own and writes its errno into the report. A second
  probe (`ps5_c_dir_probe`) does the same for `open(O_DIRECTORY)`, `getdirentries(16K)` and
  `getdents(64K)`. Both lines start with `info  C side:` in `report.txt`.

## 5f. v8 on the console (2026-10-20, 19:01 MDT): the probes answered both questions

`report.txt`, v8 (eboot `8c5b02ef…`):

- `open(O_DIRECTORY)=18 getdirentries(16K)=-1(errno 22) getdents(64K)=65536 names: . .. report.txt`
  — **`getdirentries` with 16 KiB is EINVAL on a mounted title directory; `getdents` with 64 KiB
  works** (it hands back the whole buffer, the records after the real ones carry inode 0 and are
  skipped). Rust's `read_dir` now lists `report.txt`: fs-readdir is a real pass. Mihawk's note was the
  whole story.
- `socket(plain)=18 socket(|SOCK_CLOEXEC)=19 SO_NOSIGPIPE=0 FIONBIO=-1(errno 13) connect=0 poll=1
  revents=0x4 SO_ERROR=0 => connected` — **a title's `ioctl(FIONBIO)` is EACCES; the socket with
  `SOCK_CLOEXEC`, a blocking connect and poll all work.** So v7's "Permission denied" was std's
  `set_nonblocking`, the first thing `connect_timeout` does. v8's `--wrap=socket` was therefore
  unnecessary, and it made things worse: the Rust error became EINVAL — the only new call on that
  path was the wrap's `fcntl(F_SETFD, FD_CLOEXEC)`; v9's probe records that call to close it out.
- **v9**: socket wrap removed; `--wrap=ioctl` turns `FIONBIO` into `fcntl(F_SETFL, O_NONBLOCK)` (how
  PS5_RetroArch's title sets non-blocking: libretro-common's `socket_nonblock`, used by its scraper)
  and forwards every other request to libkernel's `ioctl`. The probe now also does `F_GETFD`,
  `F_SETFD`, `F_GETFL`, `F_SETFL(O_NONBLOCK)`, a non-blocking `connect` (EINPROGRESS expected), and
  `F_SETFL` back.

Title-specific requirement 6, then: **no `ioctl(FIONBIO)` in a title; use `fcntl(O_NONBLOCK)`.**
Requirement 7: **directory reads need `getdents` with a 64 KiB buffer.** Both are in the shim.

## 5g. v9 on the console (2026-10-20, 19:11 MDT): the sandbox, not the call

The v9 probe line: `F_GETFD=-1(errno 22) F_SETFD(CLOEXEC)=-1(errno 22) F_GETFL=0x2 ioctl(FIONBIO)=-1(errno 13)
wrapped-FIONBIO=-1(errno 13) F_SETFL(O_NONBLOCK)=-1(errno 13) F_GETFL=0x6 connect=0 poll=1 … SO_ERROR=0`.

- `fcntl(F_SETFL, O_NONBLOCK)` is EACCES too, and `F_GETFL` afterwards reads `0x6` (O_NONBLOCK set):
  FreeBSD's `F_SETFL` sets the descriptor's flag first and only then calls the socket's `FIONBIO`,
  which the sandbox refuses — so the flag is on paper only, the socket stays blocking, and
  `connect` returns 0 instead of EINPROGRESS. The v9 wrap (FIONBIO → fcntl) could not work.
- `F_GETFD`/`F_SETFD` are EINVAL in a title: that was v8's EINVAL exactly.
- This is not new to the console: Mihawk measured the same thing on 13.40 for PS5_RetroArch
  (`evidence/network-elevation/README.txt`: "F_SETFL(flags | O_NONBLOCK) returned -1 / EACCES …
  Earlier logging around the whole connection helper misidentified this as a failed connection";
  after Lapy's owned-root elevation the same call passes). It is why RetroArch's online features
  need the Lapy daemon, and why its scraper "connects" only once elevated.

What a retail game — sandboxed like us — uses instead: libSceNet's `SCE_NET_SO_NBIO`, a *socket
option* (`0x1200` at `SOL_SOCKET`; shadPS4's reverse-engineered `net_types.h`), not an ioctl. **v10**
turns `FIONBIO` into `setsockopt(SOL_SOCKET, 0x1200)` and measures four things on fresh sockets:
A `setsockopt(SO_NBIO)` raw + non-blocking connect (EINPROGRESS is the tell), B the same through
`sceNetSetsockopt`, C the BSD calls for the record, D `kqueue`/`kevent` (what mio/tokio would need).
If A and B both fail, non-blocking sockets are off the table for an unelevated title, and Ruffle's
loader runs blocking sockets on threads (`TcpStream::connect` without a timeout uses none of this;
`SO_RCVTIMEO`/`SO_SNDTIMEO` work — the platform layer's FTP server uses them in a title).

## 5h. v10 on the console (2026-10-20, 19:27 MDT): 10 ok, 0 failed — milestone 1 complete

`net: connected to 1.1.1.1:443 (peer Some(1.1.1.1:443))`, through std's own `connect_timeout`.
The probe line, which is now the reference for every socket decision in this project:

```
A socket=18 setsockopt(SO_NBIO,1)=0 getsockopt(SO_NBIO)=0->256 connect=-1(errno 36) poll=1 SO_ERROR=0 setsockopt(SO_NBIO,0)=0
B socket=18 sceNetSetsockopt(SO_NBIO,1)=0 connect=-1(errno 36) poll=1 SO_ERROR=0
C socket=18 F_GETFD=-1(errno 22) F_SETFD=-1(errno 22) ioctl(FIONBIO)=-1(errno 13) F_GETFL=0x6 F_SETFL(O_NONBLOCK)=-1(errno 13) F_GETFL=0x6 wrapped-FIONBIO=0 connect=-1(errno 36) poll=1 SO_ERROR=0
D kqueue=19 kevent(EVFILT_WRITE)=1(ident=18 filter=-2)
```

- **A**: `setsockopt(SOL_SOCKET, 0x1200)` through libkernel works in a sandboxed title, and the
  non-blocking connect then returns EINPROGRESS (36) as it should. `getsockopt` reads back 256 =
  `SS_NBIO` (0x0100), the kernel's own socket-state bit — the option is implemented in the kernel,
  not translated in libSceNet.
- **B**: `sceNetSetsockopt` does the same; either entry point will do (A needs no extra import).
- **C**: the BSD calls stay refused (EACCES for `FIONBIO`/`F_SETFL`, EINVAL for `F_GETFD`/`F_SETFD`),
  and the wrapped `FIONBIO` (→ SO_NBIO) is 0. Note `F_GETFL` is already `0x6` right after the raw
  `ioctl(FIONBIO)`: FreeBSD's `sys_ioctl` sets `FNONBLOCK` on the descriptor before the socket's own
  ioctl refuses, so the refusal is in the socket layer (`soo_ioctl`), not at the syscall entry.
- **D**: `kqueue` and `kevent(EVFILT_WRITE)` work in a title — an async runtime (mio/tokio on
  kqueue) is not ruled out by the sandbox; `EVFILT_USER` (mio's waker) is untested.

Title-specific requirement 6, final form: **`ioctl(FIONBIO)` and `fcntl(O_NONBLOCK)` are refused in a
sandboxed title; non-blocking is `setsockopt(SOL_SOCKET, 0x1200 /* SCE_NET_SO_NBIO */)`.** The shim's
`--wrap=ioctl` does that for std, so `TcpStream::connect_timeout`, `set_nonblocking` and everything
built on them (an HTTP client, a websocket, mio) work unchanged.

Milestone 1 is therefore complete on hardware: Rust's std on the PS5 as a Games-row title, with
threads, clocks, files, directories, 64 MB allocations, random, and TCP in and out of the sandbox.

## 5i. Milestone 2, step 1 (2026-10-20, 19:40 MDT): the GL probe payload

The GPU path for the payload build is pacbrew v0.41.1 (ps5-payload-dev/pacbrew-repo, released
2026-10-10): Mesa 26.2.0 with RADV on the PS5's AGC (`-Dradv-winsys=ps5`), zink for OpenGL, EGL's
"videoout" platform, built as shared objects (`libEGL.so.1`, `libGLESv2.so.2`, `libGL.so.1`,
`libgallium-26.2.0.so`, `libvulkan.so.1`; 45 MB), plus an SDL2 2.30.12 whose PS5 video driver dlopen()s
them (`SDL2/0001-ps5-opengl-opengles-and-vulkan-through-mesa.patch`: "OpenGL 4.6 and OpenGL ES 3.2 on
Mesa"). The payload SDK's loader looks for `.so` files in `LD_LIBRARY_PATH`, then `/user/homebrew/lib`,
then the cwd (`crt/rtld.c`); RADV's shader cache goes to `/data/radv-shader-cache` for a payload
(Mesa patch 0028). The whole SDK+libs tarball (217 MB) downloaded here, so C for the payload can be
built and checked in this environment; only the Rust side still needs the server.

`ps5-gl-probe` (PS5_GL_PROBE_V1, C, 2.6 MB ELF, NEEDED = sprx only): SDL_Init → ES 3.0 window (fallback
2.0) → GL strings and limits → Ruffle's GLSL ES 1.00 dialect compiled → a spinning triangle for 6 s with
vsync → frame statistics → toast. Log at `/data/ruffle/gl-probe.log`. Delivered as `ps5-gl-probe-v1.zip`
with `ruffle-ps5-mesa-v0.41.1.zip` (the five libraries for `/data/ruffle/lib`).

Also noted, from the user's pointer to LightningMods/Itemzflow: the public repository is the PS4
(OrbisDev) app — it renders with Sony's own OpenGL ES 2.0 module, Piglet (`libScePigletv2VSH.sprx`,
precompiled ESSL shaders because the shader compiler is a separate Sony module), and runs with a
system-level AUTH_INFO and `libkernel_sys`. Nothing in it replaces SDL2/Mesa for the payload or
PS5_RetroArch's platform layer for the title. The one idea worth a probe later, for the title: whether
the PS5 has a loadable Piglet that a sandboxed title may start (`sceKernelLoadStartModule`) — a GLES 2
provider needing no Mesa. Unverified; one call in the title shell will tell when milestone 3 starts.

Next (the Rust side, on the server): `ruffle-ps5` payload crate = the milestone-1 target + shim (plus
`getrandom`/`getentropy` over `arc4random_buf`, which the console lacks and Ruffle's `rand` needs),
the `sdl2` crate (static, through `prospero-pkg-config`), glow from `SDL_GL_GetProcAddress`,
`ruffle_render_glow` (GLSL ES 1.00, Ruffle LLC MIT/Apache, from the Vita port), `ruffle_core` with the
null audio/navigator/ui backends first, playing a SWF from `/data/ruffle/`.

## 5j. GL probe v2/v3 (2026-10-20, 20:00–20:21 MDT): the GPU is fine, the launching process is not

- v2: RADV initialises in the payload (`[AgcDriver] Info - Initialized`, instance 1.4.354, "PlayStation 5
  GPU (RADV NAVI21)", one `VK_KHR_display` display); zink's `vkCreateDevice` returns
  `VK_ERROR_OUT_OF_DEVICE_MEMORY`, which RADV's ps5 winsys gives when `sceKernelAllocateDirectMemory`
  /`sceKernelMapDirectMemory` fails; `RADV_DEBUG=info` shows `vram_size = 0 MB` (the pool it measured).
- v3, measured the way RADV allocates (type 12, 2 MiB aligned, CPU+GPU): `sceKernelGetDirectMemorySize()`
  = 0, 16 MB allocates and maps (also at RADV's region 0x4000000000), **64 MB fails with 0x80020023
  (EAGAIN)**. `budget_set(0)` (syscall 0x23b) returns 0 at run time but changes nothing. The process was
  pid 149, uid 1, euid 0, started from the payload manager overlay.
- Why pacbrew's GL/SDL homebrew works for its author: websrv's *Homebrew* page does not run an ELF as a
  payload. `src/ps5/hbldr.c`: it launches a title process (`sceSystemServiceLaunchApp("FAKE00000")`, a
  "Homebrew Loader" app with its own param.json), raises its privileges, calls budget_set(0) through
  ptrace **before** the ELF runs, and replaces the process image with the ELF — a big app, with a game's
  direct memory and the display. The payload page (`/elfldr`, port 9021) and the user's payload manager
  spawn a daemon-class process (SceSpZeroConf) whose direct memory stops between 16 and 64 MB.

Rule for milestone 2, then: **the Ruffle payload is "homebrew" in websrv's sense — `/data/homebrew/<name>/eboot.elf`,
launched from the Homebrew page as a big app.** Delivered: `GLProbe-homebrew.zip` (the v3 probe as
`/data/homebrew/GLProbe/eboot.elf` with an icon, plus pacbrew v0.41.1's `websrv.elf`/`elfldr.elf`).

## 5k. GL probe v3 as websrv homebrew (2026-10-20, 20:39 MDT): memory solved, AGC refuses the big-app process

Launched from websrv's Homebrew page: pid 152, uid 0, euid 0, **direct memory pool 12288 MB**, 16 MB and
64 MB allocate and map — the budget question is closed. The new stop is one level down:

```
[AgcDriver] Info - Initialized, submit.mode= 1.
[Agc] FS Table offset has shifted. This is not survivable.
radv/ps5: sceAgcInit(8) failed: 0x8a6c002f
radv/ps5: the platform did not initialise; no device
MESA: error: ZINK: failed to choose pdev
```

and `vkEnumeratePhysicalDevices` = 0 devices. libSceAgc's own check: the thread-local table it set up
at load is not where it expects it at `sceAgcInit`. The big-app loader starts a real title process
(`FAKE00000`) and then replaces its image with the ELF (`bigapp_replace`); the sprx modules and their
TLS layout come from that process's original start, and the payload SDK's emulated TLS runs on top. In
the daemon-class process of v2/v3 (`SceSpZeroConf` exec'd, the ELF injected before any module ran)
`sceAgcInit` succeeded and RADV enumerated the GPU — that process only lacked the budget.

So the three launch paths measured today:

| launch | process | direct memory | AGC/RADV |
|---|---|---|---|
| payload manager overlay | daemon-class, uid 1 | pool 0, ≤ 64 MB | OK (device found) |
| websrv Homebrew page (big app) | title image replaced, uid 0 | 12 GB | **sceAgcInit fails** |
| John's elfldr spawn: `budget_set(0)` **before** exec into SceSpZeroConf (websrv's payload page `/elfldr`, or `elfldr.elf` on port 9021) | daemon-class, uid 0 | budget 0 (12 GB expected) | expected OK — the next run |

## 5l. Milestone 3 begins: Ruffle as a title on static RADV (2026-10-20, 21:15 MDT)

The payload route is set aside: both launcher processes that have the direct-memory budget are
refused by `libSceAgc` (`FS Table offset has shifted`, `sceAgcInit = 0x8a6c002f`), and the one that
passes `sceAgcInit` has no budget. A **title** has both — it is the process type the system loader
builds, the one PS5_RetroArch and the milestone-1 Rust tile already run in. So the renderer moves
into a title, and because a title cannot `dlopen`, everything is static. The static GPU path on this
console is **Vulkan through a RADV archive** (PS5_RetroArch links exactly this), so Ruffle's renderer
becomes `ruffle_render_wgpu` on RADV, not glow/GL.

The stack: `ruffle_core → ruffle_render_wgpu → wgpu 30 → wgpu-hal (vulkan) → RADV (static archive)`,
presenting through `VK_KHR_display` on VideoOut (RADV flips). The one piece with no Rust prior art is
adopting the statically linked RADV into wgpu: the C shell exports RADV's `vk_icdGetInstanceProcAddr`
(as PS5_RetroArch's `src/radv_icd_ps5.c`), `src/gpu.rs` builds `ash::Entry` from that static symbol
(no loader), creates the instance + a `VK_KHR_display` surface with plain Vulkan, and hands them to
wgpu through its `*_from_hal` seams. The ash/Vulkan half is ordinary; the wgpu-hal argument lists are
version-sensitive and get pinned against the compiler on the server, as the m1 shim was.

Built this turn (`ruffle-ps5-title/`, in the repo): the crate (`Cargo.toml` pinning ruffle_core /
ruffle_render_wgpu / wgpu 30 / ash 0.38 to a Ruffle rev), `src/gpu.rs`, `src/lib.rs` (3a clear-screen,
3b player), the C shell (compiles clean against the SDK; `sceAgcInit(8)` from the executable), and a
`build-title.sh` skeleton that reuses ps5-rust-title's converter/signer and PS5_Vulkan's
`radv-link.sh`. Phases: **3a** — wgpu on RADV clears the display (the first server build, exercises
only the GPU seam); **3b** (`--features ruffle`) — play a SWF.

**The gating dependency:** a title links RADV as `libvulkan_radeon.ps5.a`, built by PS5_Vulkan from
its Mesa fork (`tools/build-radv.sh release`). PS5_Vulkan + PS5_Mesa must be building before this
links — the largest single setup step in the project.

## 6. Risks, in order

1. The Rust `sdl2` crate against pacbrew's patched SDL2 (static link, PS5 video driver): version
   match and the GL context creation through EGL/zink in payload mode. The Vita ports do exactly
   this against vitaGL, so the frontend code shape is known; the PS5 SDL2 patch is the new part.
1b. wgpu adopting the title's RADV device (`from_raw`/`device_from_raw`/`texture_from_raw`), later:
   the APIs exist; RADV-on-PS5 has only been driven by C/C++ titles so far.
2. Something in std's FreeBSD init the console refuses at runtime (signal handlers for the
   stack-overflow guard, `/dev/null` reopen of closed stdio, `sysctl KERN_PROC_PATHNAME`). Each is one
   line to stub; the milestone-1 test prints which check failed.
3. Build complexity: the title needs PS5_Vulkan's RADV release archive (built locally with its
   `tools/build-radv.sh release`; the repository publishes no binaries) and PS5_RetroArch's tail —
   two sibling checkouts with their own bootstrap. Worth it for what they bring, but it is the
   largest single setup step.
4. Size and memory: Ruffle core + wgpu + RADV in one eboot is tens of MB; a title's flexible memory
   budget (Jelly5 asks for 1 GB in `param.json`; RetroArch's loader notes ~403 MB free at start)
   is ample for Flash content.
5. CPU-heavy AVM2 content: interpreted on both PC and PS5; the Zen 2 cores are fast. Unknown until
   it runs.

## 5m–5o. 2026-10-21: the RADV archive builds on the server; phase 3a builds and runs to the adapter

Continued in [`findings-2026-10-21.md`](findings-2026-10-21.md): what the server build of
`libvulkan_radeon.ps5.a` took (LLVM 18 for 22.04's libstdc++ from LLVM's own packages, after the
tarball's GCC-7 `std::optional` ABI crashed mesa_clc; a job cap after 16 clang jobs took the shared box
down), the phase-3a title pipeline (`ruffle-ps5-title/build-title.sh`: m1 target + ps5-rust-title tail +
PS5_Vulkan's `radv-link.sh`; wgpu 30 patched to adopt an application-made `VkSurfaceKHR`), its first
build (four one-line fixes; Mesa's weak entry points need `--no-dynamic-linker`), and its first run on
the console (RADV_PS5 reports conformance 0.0.0.0; wgpu-hal needs the non-compliant-adapter opt-in).

## 5p. 2026-10-21: first real game; a drawn pointer, per-game controls, a 21 ms audio grain

See `docs/findings-2026-10-21.md` §7–8. Ruffle plays on the console; the first real game moved with
the left stick and took touchpad clicks, but showed no pointer and did not get its action key. V3 of
the title draws the pointer over Ruffle's frame, makes the mapping a per-game file (`controls.txt`),
and gives the audio thread a 1024-frame grain plus a 10-second score line in the log.
