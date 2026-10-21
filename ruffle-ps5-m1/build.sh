#!/usr/bin/env bash
# PS5_RUST_BUILD_V1 — build ps5-rust-hello for the PS5 (milestone 1 of Ruffle-on-PS5).
# SPDX-License-Identifier: GPL-3.0-or-later
#
# Needs: the ps5-payload-dev SDK (PS5_PAYLOAD_SDK, or a Jelly5 checkout whose
# scripts/setup-toolchain.sh has run — JELLY5_ROOT, default ~/jelly5), clang+lld 18 or newer
# on PATH or via LLVM_CONFIG, and rustup with a toolchain that can build std from source:
#   rustup toolchain install nightly --component rust-src        (preferred)
#   rustup component add rust-src                                 (stable: RUSTC_BOOTSTRAP=1 is used)
#
#   ./build.sh            release build → target/x86_64-sie-ps5/release/ps5-rust-hello
#   ./build.sh --debug    debug build
#   ./build.sh --clean    remove lib/ and target/ first
#   ./build.sh --get-sdk  download SDK v0.43 into ~/ps5-payload-sdk (no Jelly5 checkout needed;
#                         the pacbrew sysroot is not needed for this milestone)
#   ./build.sh --get-llvm download clang+lld 18.1.8 into ~/llvm18 (Linux x86-64, no root needed)
#   ./build.sh --get-rust install rustup + nightly + rust-src under ~/.cargo and ~/.rustup
#   ./build.sh --setup    all three, then build
set -euo pipefail
KIT="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "${KIT}"
PROFILE=release; CLEAN=0; GETSDK=0; GETLLVM=0; GETRUST=0
for a in "$@"; do case "$a" in --debug) PROFILE=debug ;; --clean) CLEAN=1 ;; --get-sdk) GETSDK=1 ;; --get-llvm) GETLLVM=1 ;; --get-rust) GETRUST=1 ;; --setup) GETSDK=1; GETLLVM=1; GETRUST=1 ;; -h|--help) sed -n '2,18p' "$0"; exit 0 ;; *) echo "unknown option $a" >&2; exit 2 ;; esac; done
log() { echo "==> $*"; }
die() { echo "error: $*" >&2; exit 1; }
sha256() { (sha256sum "$1" 2>/dev/null || shasum -a 256 "$1") | awk '{print $1}'; }

# --- 0. LLVM (clang + lld 18+) without root: an official release tarball under ~/llvm18 ----------
LLVM_URL="https://github.com/llvm/llvm-project/releases/download/llvmorg-18.1.8/clang+llvm-18.1.8-x86_64-linux-gnu-ubuntu-18.04.tar.xz"
if [[ $GETLLVM == 1 && ! -x "$HOME/llvm18/bin/llvm-config" ]]; then
    [[ "$(uname -s)" == "Linux" && "$(uname -m)" == "x86_64" ]] || die "--get-llvm is for Linux x86-64; on macOS: brew install llvm lld"
    log "downloading LLVM 18.1.8 (about 1 GB, unpacks to ~2.5 GB)"
    curl -fL -o "$HOME/llvm18.tar.xz" "$LLVM_URL"
    mkdir -p "$HOME/llvm18"
    tar -xJf "$HOME/llvm18.tar.xz" -C "$HOME/llvm18" --strip-components=1
    rm -f "$HOME/llvm18.tar.xz"
    log "LLVM at ~/llvm18 ($("$HOME/llvm18/bin/clang" --version | head -1))"
fi
if [[ -z "${LLVM_CONFIG:-}" && -x "$HOME/llvm18/bin/llvm-config" ]]; then
    export LLVM_CONFIG="$HOME/llvm18/bin/llvm-config"      # the SDK's wrappers honour LLVM_CONFIG
    export PATH="$HOME/llvm18/bin:${PATH}"
fi

# --- 0b. Rust without root: rustup under $HOME, nightly with rust-src -------------------------
[[ -f "$HOME/.cargo/env" ]] && source "$HOME/.cargo/env"
if [[ $GETRUST == 1 ]]; then
    if ! command -v rustup >/dev/null 2>&1; then
        log "installing rustup (https://sh.rustup.rs) with nightly + rust-src"
        curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --no-modify-path --profile minimal --default-toolchain nightly --component rust-src
        source "$HOME/.cargo/env"
    elif ! rustup run nightly cargo --version >/dev/null 2>&1; then
        rustup toolchain install nightly --profile minimal --component rust-src
    fi
fi

# --- 1. the SDK ------------------------------------------------------------------------------
SDK_URL="https://github.com/ps5-payload-dev/sdk/releases/download/v0.43/ps5-payload-sdk.zip"
SDK_SHA="a9cc9929f21b2b2c5d5b309f3bab4997067c45281c0622cf4838b1aecba66fcb"   # same release Jelly5 uses
if [[ $GETSDK == 1 ]]; then
    D="${PS5_PAYLOAD_SDK:-$HOME/ps5-payload-sdk}"
    if [[ ! -x "$D/bin/prospero-clang" ]]; then
        log "downloading ps5-payload-sdk v0.43"
        curl -fL -o "$HOME/ps5-payload-sdk.zip" "$SDK_URL"
        got="$(sha256 "$HOME/ps5-payload-sdk.zip")"
        [[ "$got" == "$SDK_SHA" ]] || die "SDK zip checksum mismatch: $got"
        rm -rf "$HOME/ps5-payload-sdk.tmp"; mkdir -p "$HOME/ps5-payload-sdk.tmp"
        unzip -q "$HOME/ps5-payload-sdk.zip" -d "$HOME/ps5-payload-sdk.tmp"
        rm -rf "$D"; mv "$HOME/ps5-payload-sdk.tmp/ps5-payload-sdk" "$D"; rm -rf "$HOME/ps5-payload-sdk.tmp" "$HOME/ps5-payload-sdk.zip"
    fi
    export PS5_PAYLOAD_SDK="$D"
    log "SDK at $D — add to your shell:  export PS5_PAYLOAD_SDK=$D"
fi
if [[ -z "${PS5_PAYLOAD_SDK:-}" ]]; then
    if [[ -x "$HOME/ps5-payload-sdk/bin/prospero-clang" ]]; then
        export PS5_PAYLOAD_SDK="$HOME/ps5-payload-sdk"
    else
        J="${JELLY5_ROOT:-$HOME/jelly5}"
        [[ -x "$J/scripts/setup-toolchain.sh" ]] || die "no SDK: run  ./build.sh --get-sdk  (downloads it to ~/ps5-payload-sdk), or export PS5_PAYLOAD_SDK, or JELLY5_ROOT=<Jelly5 checkout>"
        eval "$("$J/scripts/setup-toolchain.sh" --env)"
    fi
fi
export PS5_PAYLOAD_SDK
# the SDK's wrappers find clang/lld through llvm-config (LLVM_CONFIG, or llvm-config[-NN] on PATH)
LLVM_MAJOR="$("${PS5_PAYLOAD_SDK}/bin/prospero-llvm-config" --version 2>/dev/null | cut -d. -f1 || true)"
if [[ -n "${LLVM_MAJOR}" && "${LLVM_MAJOR}" -lt 17 ]]; then
    die "LLVM ${LLVM_MAJOR} found, 17+ needed (Ubuntu 22.04 ships 14): run  ./build.sh --get-llvm"
fi
if [[ -z "${LLVM_MAJOR}" ]]; then
    if [[ "$(uname -s)" == "Darwin" ]] && command -v brew >/dev/null 2>&1 && [[ -x "$(brew --prefix llvm 2>/dev/null)/bin/llvm-config" ]]; then
        export LLVM_CONFIG="$(brew --prefix llvm)/bin/llvm-config"
        export PATH="$(brew --prefix llvm)/bin:$(brew --prefix lld 2>/dev/null)/bin:${PATH}"
    else
        die "clang/lld 18+ not found: run  ./build.sh --get-llvm  (Linux x86-64, no root), or macOS: brew install llvm lld, or export LLVM_CONFIG=<llvm>/bin/llvm-config"
    fi
fi
[[ -x "${PS5_PAYLOAD_SDK}/bin/prospero-clang" ]] || die "no prospero-clang under PS5_PAYLOAD_SDK=${PS5_PAYLOAD_SDK}"
export PATH="${PS5_PAYLOAD_SDK}/bin:${PATH}"
CC="${PS5_PAYLOAD_SDK}/bin/prospero-clang"
AR="${PS5_PAYLOAD_SDK}/bin/llvm-ar"; [[ -x "$AR" ]] || AR="$(command -v llvm-ar || true)"; [[ -n "$AR" ]] || die "llvm-ar not found"
log "SDK: ${PS5_PAYLOAD_SDK} ($("$CC" --version | head -1))"

# --- 2. the Rust toolchain --------------------------------------------------------------------
command -v rustup >/dev/null || die "rustup not found: run  ./build.sh --get-rust  (installs under ~/.cargo, no root)"
CARGO=(cargo)
if rustup run nightly cargo --version >/dev/null 2>&1; then
    CARGO=(cargo +nightly); TC=nightly
else
    TC="$(rustup show active-toolchain | awk '{print $1}')"
    export RUSTC_BOOTSTRAP=1       # lets stable accept -Z build-std; nightly is the clean way
    log "no nightly toolchain: using ${TC} with RUSTC_BOOTSTRAP=1"
fi
if ! rustup component list --toolchain "${TC}" --installed 2>/dev/null | grep -q '^rust-src'; then
    log "installing rust-src for ${TC} (std is built from source for a custom target)"
    rustup component add rust-src --toolchain "${TC}"
fi
log "rust: $("${CARGO[@]}" --version)"
# PS5_RUST_STDPATCH_V1 — std assumes FreeBSD 12's 64-bit d_fileno in readdir; the libc crate's
# freebsd11 ABI (the console's) has a 32-bit one. One cast, re-applied after every rustup update.
STDFS="$(rustup run "${TC}" rustc --print sysroot)/lib/rustlib/src/rust/library/std/src/sys/fs/unix.rs"
if [[ -f "$STDFS" ]] && grep -q 'd_ino: (\*entry_ptr)\.d_fileno,' "$STDFS"; then
    sed -i.ps5bak 's/d_ino: (\*entry_ptr)\.d_fileno,/d_ino: (*entry_ptr).d_fileno as u64,/' "$STDFS"
    log "patched std for the FreeBSD-11 dirent (${STDFS#$HOME/})"
fi

# --- 3. the C side: shim + the archives std expects under FreeBSD names ----------------------
[[ $CLEAN == 1 ]] && rm -rf lib target
mkdir -p lib build
"$CC" -O2 -Wall -Wextra -c shim/ps5_shim.c -o build/ps5_shim.o
"$CC" -O2 -Wall -c shim/execinfo_stub.c -o build/execinfo_stub.o
rm -f lib/*.a
"$AR" rcs lib/libps5shim.a build/ps5_shim.o
"$AR" rcs lib/libexecinfo.a build/execinfo_stub.o          # std links -lexecinfo on FreeBSD
cp "${PS5_PAYLOAD_SDK}/target/lib/libunwind.a" lib/libgcc_s.a   # the unwind crate links -lgcc_s
"$AR" rcs lib/librt.a                                       # the libc crate links -lrt, -lutil and -lm: empty here —
"$AR" rcs lib/libutil.a                                     # the console has those functions in libkernel and
"$AR" rcs lib/libm.a                                        # libSceLibcInternal (all of math.h checked); pacbrew's
                                                            # real libm.a wins when its sysroot is present
for l in kvm memstat procstat devstat; do "$AR" rcs lib/lib$l.a; done   # the libc crate names these FreeBSD
                                                            # libraries too; nothing in std calls into them
log "lib/: $(ls lib | tr '\n' ' ')"

# --- 4. cargo ---------------------------------------------------------------------------------
# .cargo/config.toml carries the target, -Zbuild-std, the linker script and the libc cfg.
ARGS=(build)
[[ $PROFILE == release ]] && ARGS+=(--release)
log "cargo ${ARGS[*]} (target x86_64-sie-ps5.json, std from source: the first build takes a few minutes)"
"${CARGO[@]}" "${ARGS[@]}"

# --- 5. check the result ----------------------------------------------------------------------
ELF="target/x86_64-sie-ps5/${PROFILE}/ps5-rust-hello"
[[ -f "$ELF" ]] || die "cargo finished but ${ELF} is missing"
log "built ${ELF} ($(du -h "$ELF" | cut -f1))"
"${KIT}/tools/import-check.sh" "$ELF"
READELF="$("${PS5_PAYLOAD_SDK}/bin/prospero-llvm-config" --bindir 2>/dev/null)/llvm-readelf"
[[ -x "$READELF" ]] || READELF=llvm-readelf
if command -v "$READELF" >/dev/null 2>&1; then
    "$READELF" -d "$ELF" | grep NEEDED | sed 's/^ */   /'
    "$READELF" -h "$ELF" | grep -E 'Type:' | sed 's/^ */   /'
    if "$READELF" -S "$ELF" | grep -q '\.tbss\|\.tdata'; then
        echo "   ⚠️  the ELF has TLS sections — the target spec's has-thread-local=false did not take"
    else
        echo "   no TLS sections (has-thread-local=false in effect)"
    fi
fi
log "send it:  ./send.sh <ps5-ip>      (elfldr on port 9021; a toast appears on the TV)"
