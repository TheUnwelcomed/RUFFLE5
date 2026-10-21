# ruffle-ps5 — Rust (and, in time, Ruffle) on a jailbroken PlayStation 5

An experiment in bringing [Ruffle](https://github.com/ruffle-rs/ruffle), the Flash Player emulator
written in Rust, to the PS5 as a native homebrew title — the way [Jelly5](https://github.com/02dnot/jelly5)
brought Jellyfin and [PS5_RetroArch](https://github.com/mihawk-99/PS5_RetroArch) brought RetroArch.
The plan, the decisions and every measurement taken on the console are in
[`docs/ruffle-ps5-plan.md`](docs/ruffle-ps5-plan.md).

## Status (2026-10-20)

| milestone | state |
|---|---|
| 1 — Rust's standard library on the console, as a Games-row title | **done, 10/10 checks on hardware** (threads, clocks, files, directories, 64 MB allocations, random, TCP in and out of the sandbox) |
| 2 — Flash on the TV as a payload (SDL2 + Mesa/zink on RADV + Ruffle core) | in progress: the GL probe runs RADV on the GPU; direct-memory budget of the launching process under investigation |
| 3 — the standalone title | not started |

## What is here

| directory | what |
|---|---|
| `ruffle-ps5-m1/` | the Rust toolchain for the PS5: a custom target spec (`x86_64-sie-ps5.json`, FreeBSD-11 ABI, no native TLS), `build-std`, a C shim that gives std what the console lacks or refuses (`shim/ps5_shim.c`), and the milestone-1 test program (`src/lib.rs`). `build.sh` builds it as an elfldr payload. |
| `ps5-rust-title/` | packages the same Rust code as a PPSA title for `/data/homebrew` (ShadowMount+): a C shell, the PIE→module converter and FSELF signer from Jelly5's toolkit (`native-app/`, GPL-3.0, BlackBearReloaded), the title-mode shim (no raw syscalls), and the checks that catch what a sandboxed title cannot do. `build-title.sh` does it all. |
| `ps5-gl-probe/` | milestone 2, step 1: a C payload that drives SDL2 → EGL → zink → RADV on the console and logs every step (`/data/ruffle/gl-probe.log`). Needs pacbrew ≥ v0.41.1 and its Mesa shared objects on the console. |
| `docs/` | the plan and the findings, section by section, as they were made. |

## Building

- The Rust side needs the [ps5-payload-dev SDK](https://github.com/ps5-payload-dev/sdk), LLVM 18 and a
  Rust nightly with `rust-src` (`ruffle-ps5-m1/build.sh --setup` fetches what it can). A stable toolchain
  works with `RUSTC_BOOTSTRAP=1`.
- The title needs, in addition, a C++20 compiler for the converter (built on first use).
- The GL probe needs the [pacbrew](https://github.com/ps5-payload-dev/pacbrew-repo) SDK tarball
  (v0.41.1 or later), which carries Mesa 26.2 for the PS5 and the SDL2 that loads it.

## What a sandboxed PS5 title cannot do (and what this repository does instead)

Measured on hardware, firmware with the usual homebrew setup; details and the probe output are in the plan:

1. No raw `syscall` instructions — the console kills the process (0xa002030a). The SDK's `libc.a` has
   three members that use them; the title links without them and the shim supplies the calls.
2. `opendir` is refused; directories are read with `getdents` into a 64 KiB buffer (`getdirentries`
   with 16 KiB is EINVAL on a mounted title folder). `getcwd` jumps to address 0; `realpath` is EPERM.
3. Imports resolve only against the libraries a title loads; a weak import of something else is a
   silent NULL. The build refuses any.
4. `sceNetInit()` before the first socket.
5. `ioctl(FIONBIO)` and `fcntl(O_NONBLOCK)` are EACCES; non-blocking is `setsockopt(SOL_SOCKET, 0x1200)`
   (`SCE_NET_SO_NBIO`), which the shim does for std behind `--wrap=ioctl`. `kqueue` works.
6. The `dlopen` of the payload SDK cannot be used in a title (its loader makes raw syscalls), so Mesa
   has to be linked statically there — hence milestone 2 as a payload first.

## Licence

GPL-3.0-or-later (see `LICENSE`). `ps5-rust-title/native-app/` is from Jelly5's toolkit (GPL-3.0,
BlackBearReloaded); the Vita ports' `ruffle_render_glow`, when it joins, is MIT OR Apache-2.0 (Ruffle LLC).
