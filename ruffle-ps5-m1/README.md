# ps5-rust-hello — milestone 1 of Ruffle on the PS5

Rust's standard library running as a [ps5-payload-dev](https://github.com/ps5-payload-dev/sdk)
payload, built the way Jelly5 is built (the same SDK, the same clang/lld, the same loader).
Nothing in Ruffle can start before this runs, so this kit does only this, with no crates.

What's in it:

| file | what |
|---|---|
| `x86_64-sie-ps5.json` | the Rust target (PS5_RUST_TARGET_V1): rustc's `x86_64-unknown-freebsd` spec with `has-thread-local=false` (the SDK uses emulated TLS; std falls back to pthread keys), PIE, `panic=abort`, no rpath, `crt-static-respected=false`. LLVM still targets `x86_64-unknown-freebsd`; the objects are plain SysV x86-64 ELF and the SDK's clang/lld link them. rustc rejects unknown keys in a spec, so there is no comment inside the file |
| `.cargo/config.toml` | the target, `-Zbuild-std`, the linker script, and the libc crate's FreeBSD-11 ABI cfg |
| `shim/ps5_shim.c` | the versioned FreeBSD names the libc crate asks for (`fstat@FBSD_1.0` …) over the console's plain functions; `accept4`, `getpeereid`, `elf_aux_info`, `pthread_setname_np`; test helpers (toast, C-side stat/readdir) |
| `shim/execinfo_stub.c` | `backtrace`/`backtrace_symbols_fd` (std links `-lexecinfo`; never called) |
| `tools/ps5-rust-linker.sh` | what cargo calls as the linker: puts back the SDK libraries that `-nodefaultlibs` drops, adds `lib/`, hands over to `prospero-clang` |
| `tools/import-check.sh` | every import of the ELF must be exported by an SDK library (Jelly5's runtime import check) |
| `src/main.rs` | the test program: 10 checks, exit code = failures, a toast on the TV |
| `build.sh`, `send.sh` | build; send to elfldr (port 9021) and show the output |

## Setup (once)

Linux or macOS, no root needed. On the Bytesized host everything goes under `$HOME`.

1. **clang + lld 18 or newer.** Linux without root: unpack an LLVM release
   (`clang+llvm-18.1.8-x86_64-linux-gnu-ubuntu-18.04.tar.xz` from
   https://github.com/llvm/llvm-project/releases) into `~/llvm18` and
   `export LLVM_CONFIG=~/llvm18/bin/llvm-config`. macOS: `brew install llvm lld coreutils`.
2. **The SDK.** `./build.sh --get-sdk` downloads ps5-payload-dev SDK v0.43 (the release Jelly5 uses,
   checksum verified) into `~/ps5-payload-sdk`; the pacbrew sysroot is not needed for this
   milestone. Or point at one you have: `export PS5_PAYLOAD_SDK=…`, or `JELLY5_ROOT=<Jelly5 checkout>`
   where `scripts/setup-toolchain.sh` has run.
3. **Rust.** `curl https://sh.rustup.rs -sSf | sh -s -- -y` then
   `rustup toolchain install nightly --component rust-src`
   (`-Zbuild-std` needs rust-src; nightly is the supported way, stable works through
   `RUSTC_BOOTSTRAP=1`, which `build.sh` sets when there is no nightly).

## Build and run

```
chmod +x build.sh send.sh tools/*.sh
./build.sh --get-sdk       # first time: fetches the SDK, then builds
./build.sh                 # → target/x86_64-sie-ps5/release/ps5-rust-hello, plus the import check
./send.sh 192.168.x.y      # the console, jailbroken, with elfldr running (port 9021)
```

Expected on the terminal (and "Rust on PS5: 10 ok, 0 failed" as a toast on the TV):

```
Rust on the PS5 — ps5-rust-hello (PS5_RUST_HELLO_V5)
ok    basics: os=freebsd arch=x86_64 pid=… parallelism=Ok(…)
ok    shim: ps5_notify called (look for the toast on the TV)
ok    clocks: …
ok    threads: 4 threads x 10000 = 40000, joined=true
ok    hashmap: 1000 entries
info  C side: sizeof(struct stat)=120 (FreeBSD 11: 120) sizeof(struct dirent)=…
ok    fs-stat: wrote 1234 bytes: Rust metadata().len()=1234 C stat st_size=1234 read back ok=true
ok    fs-readdir: /data: Rust read_dir()=N entries, C readdir=N+2 (incl. . and ..)
ok    fs-paths: …
ok    memory: 64 MB allocated and touched
ok    net: connected to 1.1.1.1:443     (FAIL here only means no internet on the console)
Rust on PS5: 10 ok, 0 failed, 0.x s
```

`fs-stat` and `fs-readdir` are the ones that prove the struct layouts: Rust's `metadata().len()`
is read out of the `struct stat` the console filled in; if the layout were wrong the number
would be garbage, not 1234.

## Where it can fail, and what each failure means

- **`cargo` can't find rust-src / `-Z` rejected** → step 3 above.
- **Link error `undefined symbol`** → a std function the SDK does not export. The pre-check in
  the findings doc lists every libc function std calls on FreeBSD against the SDK's exports;
  the seven it lacks are in the shim. A new one goes into `shim/ps5_shim.c` the same way.
- **`import check: … NOT in any SDK library`** → the ELF links but the loader could not bind it
  on the console; same fix.
- **elfldr prints nothing / the console shows no toast** → the loader rejected the ELF. Compare
  `llvm-readelf -l` of this ELF with the SDK's `samples/hello_world/hello_world.elf` (both must be
  `DYN`, PIE, page size 0x4000, no TLS program header).
- **The toast appears, the terminal shows FAILs** → read the line: each check names the subsystem
  (clocks, threads, fs-stat, …) and the values it saw.

## What the SDK has (checked, not assumed)

The SDK's `libkernel.so`, `libSceLibcInternal.so`, `libSceNet.so` stubs and `libc.a` export
4803 symbols. Of the 227 libc functions std's unix/FreeBSD code calls, 7 are missing and
matter: `accept4`, `getpeereid`, `elf_aux_info`, `pthread_setname_np` (in the shim), and
`ttyname`, `copy_file_range`, `__cxa_thread_atexit_impl` (never reached: std uses `isatty`,
`copy_file_range` is Linux-only, and `__cxa_thread_atexit_impl` is only for native TLS).
`_umtx_op`, `pthread_*`, `sysctlbyname`, `arc4random_buf`, `cpuset_getaffinity`, `getaddrinfo`,
`clock_gettime`, `mmap`, `sigaltstack`, `__error`, `environ` are all there.
std links `-lgcc_s -lexecinfo -lpthread`; the libc crate links `-lc -lm -lrt -lpthread -ldl -lutil`.
The SDK has `libc.a`, `libpthread.a`, `libdl.a`, `libunwind.a` (= `libgcc_s` here, it needs only the
`__eh_frame_*` bounds, which the SDK's linker script provides); `librt`/`libutil`/`libm` are empty
archives here (every math.h function is exported by `libSceLibcInternal`; pacbrew's real `libm.a`
takes precedence when its sysroot is present), `libexecinfo` is the two-function stub.

Verified with the SDK's own toolchain (clang 18.1.3, lld 18): a C stand-in with rustc's exact
link line (`-nodefaultlibs`, `-pie`, `-z relro -z now`, `--gc-sections`, the eight `-l` libraries
above) links through `tools/ps5-rust-linker.sh`, the `.symver` definitions satisfy the
`fstat@FBSD_1.0` references and call the console's `fstat` through the GOT, and the import check
passes (21 imports, all exported; DT_NEEDED `libkernel_web.sprx`, `libSceLibcInternal.sprx`).
The Rust side could not be compiled where this kit was written (no access to the Rust toolchain
downloads there), so the first `cargo build` is the real test.

## After milestone 1

2. A Rust `staticlib` + wgpu on the pacbrew Mesa's **RADV** (hardware Vulkan on the PS5's GPU,
   `VK_KHR_display` on VideoOut) — a triangle on the TV.
3. Ruffle's core with a frontend modelled on its `exporter` crate: SWF from `/data`, pad →
   mouse/keyboard, an `AudioBackend` on `sceAudioOut`, a `NavigatorBackend` on the SDK's sockets.
4. Package with Jelly5's native-app tail as its own title folder under `/data/homebrew`.
