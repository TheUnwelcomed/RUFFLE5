# ps5-rust-title — "Rust Hello" as a PS5 title (a tile under Games)

The milestone-1 test program packaged the way Jelly5 and PS5_RetroArch are: a `PPSA99610` folder for
`/data/homebrew`, mounted by ShadowMount+, launched from the home screen. No elfldr, no `nc`.

| file | what |
|---|---|
| `native-app/` | BlackBearReloaded's native-app tooling as Jelly5 vendors it (GPL-3.0-or-later): the PIE → PS5-module converter and FSELF signer (sources + `host/ps5-native-tool`, a static Linux x86-64 build), `ps5-pie.ld`, `app-symbols.map`, the CRT (`app_crt.cpp`), the C++ allocation runtime, the libc gap fillers (`libc_ext.c`), `getaddrinfo` on the console resolver (`evo_dns.c`), the mmap allocator (`malloc_shim.c`), and the clean-room `runtime/libc.prx` (digest pinned) |
| `sce_sys/param.json` | the title: `PPSA99610` "Rust Hello", `applicationCategoryType 0` / `attribute 0` = the Games row (PS5_RetroArch's values; Jelly5's `65536` + `536870912` is the Media row) |
| `sce_sys/icon0.png` | the tile |
| `shell/main.c` | the C entry: checks the generated import table (the loader binds an unknown import to NULL silently, so the shell toasts any), then calls the Rust library |
| `shell/boot_log.c` | `evo_boot_log` for `evo_dns.c` → `/app0/ps5-rust-hello/boot.log` (fallback `/download0`) |
| `shell/standin.c` | C in the Rust library's place, to test the packaging alone (`PS5_RUST_STANDIN=1`) |
| `build-title.sh` | builds the Rust staticlib via `../ruffle-ps5-m1/build.sh`, compiles the shell + runtime, links with Jelly5's exact `ld.lld` line, checks imports/NEEDED/TLS, converts, signs, assembles the folder and zips it |

## Build (on the machine with the milestone-1 setup)

```
unzip -o ps5-rust-title.zip     # next to ruffle-ps5-m1/ (both in the same parent folder)
cd ps5-rust-title
chmod +x build-title.sh
./build-title.sh                # → build/app/PPSA99610/ and build/app/RustHello-PPSA99610.zip
```

It finds the SDK, LLVM and Rust the same way `ruffle-ps5-m1/build.sh` does. `PS5_RUST_M1=<path>` points
at the milestone-1 kit if it is elsewhere. `PS5_RUST_STANDIN=1` builds the C stand-in instead of Rust
(what `RustHello-standin-PPSA99610.zip` is).

## Install

1. Close any running homebrew title. Copy the `PPSA99610` folder to `/data/homebrew/` on the console
   (ps5upload or FTP), so that `/data/homebrew/PPSA99610/eboot.bin` exists.
2. ShadowMount+ adds a **Rust Hello** tile under Games (if not: run the payloads again, or reboot and
   jailbreak again — the same as Jelly5's install notes).
3. Open it. Two toasts: "Rust is running on this PS5", then "Rust on PS5: N ok, M failed". The full
   per-check report is in `/app0/ps5-rust-hello/report.txt`, i.e. `/data/homebrew/PPSA99610/ps5-rust-hello/report.txt` over FTP (`/download0` is the fallback, and it is not visible over FTP).
   The stand-in build toasts "Rust Hello (stand-in) …" instead — it proves the packaging and the
   Games-row tile with no Rust in it.
4. To update: copy the new folder's files over the old ones; never delete and re-create the folder
   while ShadowMount+ has it mounted (Jelly5's README has the same warning).

## What the build checks before it signs

- the link: Jelly5's line — `ps5-pie.ld`, `--allow-multiple-definition` (evo_dns first), the version
  script, `-e _start` (the toolkit's CRT), `malloc_shim.o` first so its allocator wins, the SDK's
  `.so` stubs minus the WebKit/system kernel variants (apps import `libkernel` only), `libc.a` last;
- NEEDED must be `libkernel.sprx`, `libSceLibcInternal.sprx` (and `libSceNet.sprx` for the resolver),
  never `libkernel_web`/`libScePosixForWebKit`; no TLS segment;
- every import exported by a system library (the SDK stubs are the list); then the same table is
  compiled into the title so the shell can toast an import the loader left NULL.

Verified here with the C stand-in: 58 imports, all exported; `.data.rel.ro` present (the converter
requires it); FSELF signed, `integrity: valid`. The Rust build is the same tail with
`libps5_rust_hello.a` + the shim in the group.
