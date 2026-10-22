#!/usr/bin/env bash
# PS5_RUFFLE_TITLE_BUILD_V4 — build the Ruffle PS5 title: a PPSA folder for /data/homebrew, a tile
# under Games. Phase 3a by default (wgpu on the static RADV clears the display); 3b with --ruffle.
# SPDX-License-Identifier: GPL-3.0-or-later
#
# Three proven pieces, joined:
#   * ruffle-ps5-m1 (milestone 1): the Rust-for-PS5 target and the C shim (title mode) that put
#     Rust's std on the console as a Games-row title.
#   * ps5-rust-title: the title tail that ran it — converter + signer, PRX stubs, libc.prx, the
#     link line, the import / raw-syscall / TLS gates.
#   * PS5_Vulkan's tools/radv-link.sh: how a title links the RADV archive (whole), libc++/libunwind
#     for ACO, Clang's builtins, and the SDK fork's platform layer (the heap in direct memory via
#     --wrap=malloc & co, the pthread/memstream wraps, the ps5_* libc stand-ins via --defsym).
#     Where the platform layer and the m1 shim both give a function a body (opendir, openat,
#     clock_nanosleep, ...), the --defsym binding wins at link time: RADV's own functions, used by
#     Mihawk's titles and PS5_RetroArch, serve Rust too.
#
# Needs (all made by build-radv-server.sh, which wrote ~/.ruffle-radv-env — source it first):
#   PS5_VULKAN_DIR   a PS5_Vulkan checkout with .deps/native/radv-release/lib/libvulkan_radeon.ps5.a
#   PS5_PAYLOAD_SDK  its SDK (.deps/native/ps5-payload-sdk: upstream v0.42 + the platform layer)
#   LLVM_CONFIG      the LLVM 18 tarball (~/llvm18); rustup nightly + rust-src (milestone 1)
#
#   source ~/.ruffle-radv-env && ./build-title.sh           → build/app/PPSA99620/{eboot.bin,...}
#   ./build-title.sh --ruffle     3b: Ruffle compiled in (--features ruffle), plays /data/ruffle/movie.swf
#   ./build-title.sh --clean      remove build/ and target/ first
#   JOBS=n                        parallel compile jobs (default 4: the build box is shared)
#   PS5_RUST_KIT=<path>           ps5-rust-title (default ../ps5-rust-title)
#   PS5_RUST_M1=<path>            ruffle-ps5-m1 (default ../ruffle-ps5-m1)
set -euo pipefail
HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "${HERE}"
log() { echo "==> $*"; }
ok()  { echo "   ✓ $*"; }
die() { echo "error: $*" >&2; exit 1; }
need() { [[ -e "$1" ]] || die "$2"; }

RUFFLE=0; CLEAN=0
for a in "$@"; do case "$a" in --ruffle) RUFFLE=1 ;; --clean) CLEAN=1 ;; -h|--help) sed -n '2,30p' "$0"; exit 0 ;; *) die "unknown option $a" ;; esac; done
JOBS="${JOBS:-4}"
NICE=(nice -n 19); command -v ionice >/dev/null 2>&1 && NICE+=(ionice -c 2 -n 7)

# --- 1. environment -------------------------------------------------------------------------------
[[ -f "$HOME/.cargo/env" ]] && source "$HOME/.cargo/env"
if [[ -z "${LLVM_CONFIG:-}" && -x "$HOME/llvm18/bin/llvm-config" ]]; then export LLVM_CONFIG="$HOME/llvm18/bin/llvm-config"; fi
[[ -n "${LLVM_CONFIG:-}" ]] || die "LLVM_CONFIG not set (source ~/.ruffle-radv-env)"
export LLVM_CONFIG
LLVM_BIN="$("$LLVM_CONFIG" --bindir)"; export PATH="$LLVM_BIN:$HOME/.local/bin:$PATH"
VK="${PS5_VULKAN_DIR:-$HOME/ps5gpu/PS5_Vulkan}"
need "$VK/tools/radv-link.sh" "PS5_VULKAN_DIR=$VK has no tools/radv-link.sh"
RADV_ARCHIVE="${RADV_ARCHIVE:-$VK/.deps/native/radv-release/lib/libvulkan_radeon.ps5.a}"
need "$RADV_ARCHIVE" "no RADV archive at $RADV_ARCHIVE — run build-radv-server.sh first"
SDK="${PS5_PAYLOAD_SDK:-$VK/.deps/native/ps5-payload-sdk}"
need "$SDK/bin/prospero-clang" "no prospero-clang under PS5_PAYLOAD_SDK=$SDK"
need "$SDK/target/lib/libps5platform.a" "$SDK has no platform layer (libps5platform.a): the title needs PS5_Vulkan's SDK (.deps/native/ps5-payload-sdk), not the plain v0.43"
export PS5_PAYLOAD_SDK="$SDK"
SYSROOT="$SDK/target"; STUBDIR="$SYSROOT/lib"
TCC="$SDK/bin/prospero-clang"; TCXX="$SDK/bin/prospero-clang++"; LLD="$SDK/bin/prospero-lld"
TBIN="$("$SDK/bin/prospero-llvm-config" --bindir 2>/dev/null || true)"; [[ -n "$TBIN" ]] || TBIN="$LLVM_BIN"
NM="$TBIN/llvm-nm"; READELF="$TBIN/llvm-readelf"; AR="$TBIN/llvm-ar"; OBJDUMP="$TBIN/llvm-objdump"
for t in "$TCC" "$TCXX" "$LLD" "$NM" "$READELF" "$AR"; do [[ -x "$t" ]] || die "missing tool: $t"; done
KIT="${PS5_RUST_KIT:-$HERE/../ps5-rust-title}"; KIT="$(cd "$KIT" 2>/dev/null && pwd || true)"
[[ -n "$KIT" && -f "$KIT/native-app/native_app_builder.cpp" ]] || die "ps5-rust-title kit not found (PS5_RUST_KIT=<path>)"
M1="${PS5_RUST_M1:-$HERE/../ruffle-ps5-m1}"; M1="$(cd "$M1" 2>/dev/null && pwd || true)"
[[ -n "$M1" && -f "$M1/shim/ps5_shim.c" ]] || die "ruffle-ps5-m1 kit not found (PS5_RUST_M1=<path>)"
NATIVE="$KIT/native-app"
TOOL="$KIT/host/ps5-native-tool"
if [[ ! -x "$TOOL" ]]; then
    log "building the host converter (clang++ + zlib)"
    ZI="${ZLIB_INCLUDE:-$VK/.deps/native/zlib/root/usr/include}"; ZA="${ZLIB_ARCHIVE:-$VK/.deps/native/zlib/root/usr/lib/libz.a}"
    [[ -f "$ZA" && -f "$ZI/zlib.h" ]] || die "static zlib not found (ZLIB_INCLUDE/ZLIB_ARCHIVE; build-radv-server.sh's setup makes one)"
    mkdir -p "$KIT/host"
    clang++ -std=c++20 -O2 -static -I "$ZI" "$NATIVE/native_app_builder.cpp" "$NATIVE/self_container.cpp" \
        "$NATIVE/elf_object.cpp" "$NATIVE/sce_module_writer.cpp" "$ZA" -o "$TOOL"
fi
LIBC_PRX="$NATIVE/runtime/libc.prx"; need "$LIBC_PRX" "native-app/runtime/libc.prx missing"
(cd "$NATIVE/runtime" && sha256sum -c libc.prx.sha256 >/dev/null) || die "libc.prx does not match its pinned digest"
ok "SDK $SDK ($("$TCC" --version | head -1)); RADV $(du -h "$RADV_ARCHIVE" | cut -f1); kit $KIT; m1 $M1; $JOBS jobs"

# Hardware-validated loader profile (Jelly5 app/scripts/build.sh, PS5_Vulkan tools/build-radv-title.sh).
MODULE_SDK=0x02000009
COMPANION_SDK=0x08050001
FSELF_MAGIC=0x1D3D154F
TFLAGS=(-fno-plt -fno-stack-protector -ffunction-sections -fdata-sections)

PARAM="$HERE/sce_sys/param.json"
TITLE_ID="$(python3 -c 'import json,sys; print(json.load(open(sys.argv[1]))["titleId"])' "$PARAM")"
BUILD="$HERE/build"; APPDIR="$BUILD/app/$TITLE_ID"
[[ $CLEAN == 1 ]] && rm -rf "$BUILD" "$HERE/target"
rm -rf "$BUILD/obj" "$BUILD/stubs"; mkdir -p "$BUILD/obj" "$BUILD/stubs" "$BUILD/app"

# --- 2. the Rust library -----------------------------------------------------------------------------
# 2a. the toolchain, as milestone 1's build.sh prepares it: nightly + rust-src, and std's one-cast
# patch for the FreeBSD-11 dirent (re-applied after every rustup update).
command -v rustup >/dev/null || die "rustup not found (milestone 1: ruffle-ps5-m1/build.sh --get-rust)"
CARGO=(cargo)
if rustup run nightly cargo --version >/dev/null 2>&1; then CARGO=(cargo +nightly); TC=nightly
else TC="$(rustup show active-toolchain | awk '{print $1}')"; export RUSTC_BOOTSTRAP=1; log "no nightly toolchain: using $TC with RUSTC_BOOTSTRAP=1"; fi
rustup component list --toolchain "$TC" --installed 2>/dev/null | grep -q '^rust-src' || rustup component add rust-src --toolchain "$TC"
STDFS="$(rustup run "$TC" rustc --print sysroot)/lib/rustlib/src/rust/library/std/src/sys/fs/unix.rs"
if [[ -f "$STDFS" ]] && grep -q 'd_ino: (\*entry_ptr)\.d_fileno,' "$STDFS"; then
    sed -i.ps5bak 's/d_ino: (\*entry_ptr)\.d_fileno,/d_ino: (*entry_ptr).d_fileno as u64,/' "$STDFS"
    ok "patched std for the FreeBSD-11 dirent"
fi
ok "rust: $("${CARGO[@]}" --version)"

# 2b. wgpu v30.0.1 for the surface patch (Cargo.toml [patch.crates-io] points at vendor/wgpu).
# Fetched by explicit tag ref (this server's git rejects tag clones) and patched once.
fetch_rev() {   # fetch_rev DIR URL REV(tag name or commit sha)
    local dir="$1" url="$2" rev="$3" ref="$3"
    [[ "$rev" =~ ^[0-9a-f]{40}$ ]] || ref="refs/tags/$rev"
    if [[ -f "$dir/.fetched-rev" && "$(<"$dir/.fetched-rev")" == "$rev" ]]; then return 0; fi
    rm -rf "$dir"; mkdir -p "$dir"
    git -C "$dir" init -q
    git -C "$dir" remote add origin "$url"
    git -C "$dir" fetch -q --no-tags --depth 1 origin "$ref" || die "git fetch $url $ref failed"
    git -C "$dir" -c advice.detachedHead=false checkout -q FETCH_HEAD || die "git checkout of $ref failed"
    printf '%s\n' "$rev" > "$dir/.fetched-rev"
}
log "wgpu v30.0.1 sources (vendor/wgpu) + the surface patch"
fetch_rev "$HERE/vendor/wgpu" https://github.com/gfx-rs/wgpu.git v30.0.1
if ! grep -q PS5_RUFFLE_WGPU_HAL_PATCH_V1 "$HERE/vendor/wgpu/wgpu-hal/src/vulkan/instance.rs"; then
    git -C "$HERE/vendor/wgpu" apply --check "$HERE/tools/wgpu-hal-ps5-surface.patch" || die "tools/wgpu-hal-ps5-surface.patch does not apply to wgpu v30.0.1"
    git -C "$HERE/vendor/wgpu" apply "$HERE/tools/wgpu-hal-ps5-surface.patch"
fi
grep -q PS5_RUFFLE_WGPU_HAL_PATCH_V1 "$HERE/vendor/wgpu/wgpu-hal/src/vulkan/instance.rs" || die "the wgpu-hal patch is not in place"
ok "wgpu-hal patched (PS5_RUFFLE_WGPU_HAL_PATCH_V1)"

# 2c. cargo: this crate to a staticlib. Same target and shim as milestone 1; std from source
# (.cargo/config.toml). -Zgit=shallow-deps: the pinned Ruffle revision is fetched shallow, not with
# its whole history. Capped at $JOBS rustc processes: the build box is shared.
CARGO_ARGS=(build --release -j "$JOBS" -Zgit=shallow-deps)
[[ $RUFFLE == 1 ]] && CARGO_ARGS+=(--features ruffle)
log "cargo ${CARGO_ARGS[*]} (first build compiles std + wgpu: several minutes at $JOBS jobs)"
"${NICE[@]}" "${CARGO[@]}" "${CARGO_ARGS[@]}"
RUST_A="$HERE/target/x86_64-sie-ps5/release/libruffle_ps5.a"
need "$RUST_A" "cargo finished but $RUST_A is missing"
# the shim in title mode (-DPS5_TITLE): libkernel-backed bodies for the POSIX calls the SDK's libc.a
# implements with raw syscalls, the FBSD_1.0 symver wrappers std binds, --wrap=ioctl's SO_NBIO body
"$TCC" -O2 -Wall -DPS5_TITLE "${TFLAGS[@]}" -c "$M1/shim/ps5_shim.c" -o "$BUILD/obj/ps5_shim_title.o"
"$TCC" -O2 -Wall "${TFLAGS[@]}" -c "$M1/shim/execinfo_stub.c" -o "$BUILD/obj/execinfo_stub.o"
"$TCC" -std=gnu11 -O2 -Wall "${TFLAGS[@]}" -c "$HERE/shell/dl_stubs.c" -o "$BUILD/obj/dl_stubs.o"
"$AR" rcs "$BUILD/obj/libps5shim_title.a" "$BUILD/obj/ps5_shim_title.o" "$BUILD/obj/execinfo_stub.o" "$BUILD/obj/dl_stubs.o"
RUST_LIBS=("$RUST_A" "$BUILD/obj/libps5shim_title.a")
ok "$(du -h "$RUST_A" | cut -f1) ${RUST_A##*/} + the shim in title mode"

# --- 3. the shell and the runtime pieces ---------------------------------------------------------------
# The CRT and C++ runtime are PS5_Vulkan's copies (tooling/native: the ones RADV titles ship with —
# the CRT sets the IEEE floating-point state); the converter, linker script, stubs and libc.prx are
# the kit's (the same BlackBearReloaded tail, hardware-validated with Rust std).
log "compiling the shell and the native-app runtime"
"$TCC" -std=gnu11 -O2 -Wall "${TFLAGS[@]}" -c "$HERE/shell/main.c" -o "$BUILD/obj/main.o"
"$TCC" -std=gnu11 -O2 -Wall "${TFLAGS[@]}" -c "$HERE/shell/pad.c" -o "$BUILD/obj/pad.o"
"$TCC" -std=gnu11 -O2 -Wall "${TFLAGS[@]}" -c "$HERE/shell/libc_fixes.c" -o "$BUILD/obj/libc_fixes.o"
"$TCC" -std=gnu11 -O2 -Wall "${TFLAGS[@]}" -c "$KIT/shell/boot_log.c" -o "$BUILD/obj/boot_log.o"
VKNATIVE="$VK/tooling/native"; [[ -f "$VKNATIVE/app_crt.cpp" ]] || VKNATIVE="$NATIVE"
"$TCXX" -std=c++20 -O2 -fno-exceptions -fno-rtti "${TFLAGS[@]}" -c "$VKNATIVE/app_crt.cpp" -o "$BUILD/obj/app_crt.o"
"$TCXX" -std=c++20 -O2 -fno-exceptions -fno-rtti "${TFLAGS[@]}" -c "$VKNATIVE/app_cpp_runtime.cpp" -o "$BUILD/obj/app_cpp_runtime.o"
# libunwind.a names "pthread" as a dependent library; the console's pthreads come from libkernel.
"$AR" rcs "$BUILD/obj/libpthread.a" "$BUILD/obj/app_cpp_runtime.o"
"$TCC" -std=gnu11 -O2 -w "${TFLAGS[@]}" -c "$NATIVE/stubs/libc_ext.c" -o "$BUILD/obj/libc_ext.o"
"$TCC" -std=gnu11 -O2 -w "${TFLAGS[@]}" -c "$NATIVE/stubs/evo_dns.c" -o "$BUILD/obj/evo_dns.o"
"$TCC" -std=gnu11 -O2 -w "${TFLAGS[@]}" -c "$NATIVE/stubs/malloc_shim.c" -o "$BUILD/obj/malloc_shim.o"
# malloc_shim first as in the kit; with the platform's --wrap=malloc it is only the __real_ fallback
RUNTIME_OBJS=("$BUILD/obj/malloc_shim.o" "$BUILD/obj/app_crt.o" "$BUILD/obj/app_cpp_runtime.o"
              "$BUILD/obj/libc_ext.o" "$BUILD/obj/evo_dns.o")
OBJS=("$BUILD/obj/main.o" "$BUILD/obj/pad.o" "$BUILD/obj/libc_fixes.o" "$BUILD/obj/boot_log.o")

# --- 4. the RADV link recipe (PS5_Vulkan tools/radv-link.sh) ----------------------------------------
export PS5_CLANG="$LLVM_BIN/clang"        # for Clang's builtins archive (__emutls_get_address)
# The recipe names the builtins by their classic path (lib/linux/libclang_rt.builtins-x86_64.a, as
# Debian's clang lays them out); the LLVM release tarball uses the per-target layout. Same archive.
RES="$("$PS5_CLANG" --print-resource-dir)"
if [[ ! -f "$RES/lib/linux/libclang_rt.builtins-x86_64.a" && -f "$RES/lib/x86_64-unknown-linux-gnu/libclang_rt.builtins.a" ]]; then
    mkdir -p "$RES/lib/linux"
    ln -sf ../x86_64-unknown-linux-gnu/libclang_rt.builtins.a "$RES/lib/linux/libclang_rt.builtins-x86_64.a"
    ok "builtins: the tarball's per-target libclang_rt.builtins.a linked under the name radv-link.sh expects"
fi
# shellcheck source=/dev/null
source "$VK/tools/radv-link.sh"
radv_link_recipe "$VK" "$SDK" "$RADV_ARCHIVE" || die "radv_link_recipe failed (missing input above)"
PLATFORM="$SDK/target/lib/libps5platform.a"
ok "RADV link: ${#radv_link_inputs[@]} inputs, ${#radv_link_flags[@]} flags (wraps + ps5_* bindings)"

# --- 5. PRX import stubs for system modules the SDK has no link stub for -----------------------------
# AGC: RADV's command processor imports (libSceAgc / libSceAgcDriver), as PS5_Vulkan's titles stub them.
UNDEF="$BUILD/obj-undef.txt"
for o in "${OBJS[@]}" "${RUNTIME_OBJS[@]}" "${RUST_LIBS[@]}" "$RADV_ARCHIVE" "$PLATFORM"; do
    "$NM" -u "$o" 2>/dev/null | grep -oE '\bsce[A-Za-z0-9_]+' || true
done | sort -u > "$UNDEF"
PRX_STUB_SOS=()
for base in libSceVideodec2 libSceAudiodec libSceAgc libSceAgcDriver libSceCommonDialog libSceWebBrowserDialog; do
    csrc="$BUILD/stubs/$base.c"; : > "$csrc"
    while read -r s; do
        case "$base" in
            libSceAgcDriver) [[ "$s" == sceAgcDriver* ]] || continue ;;
            libSceAgc)       [[ "$s" == sceAgc* && "$s" != sceAgcDriver* ]] || continue ;;
            libSceVideodec2) [[ "$s" == sceVideodec2* ]] || continue ;;
            libSceAudiodec)  [[ "$s" == sceAudiodec* ]] || continue ;;
            libSceCommonDialog) [[ "$s" == sceCommonDialog* ]] || continue ;;
            libSceWebBrowserDialog) [[ "$s" == sceWebBrowserDialog* ]] || continue ;;
        esac
        echo "void $s(void){}" >> "$csrc"
    done < "$UNDEF"
    [[ -s "$csrc" ]] || continue
    so="$BUILD/stubs/$base.so"
    "$TCC" -shared -nostdlib -nodefaultlibs -fPIC -Wl,-soname,"$base.sprx" -o "$so" "$csrc"
    PRX_STUB_SOS+=("$so")
    printf '   %-28s %s syms\n' "$base.sprx" "$(wc -l < "$csrc")"
done
ok "sce* imports: $(wc -l < "$UNDEF") ($(tr '\n' ' ' < "$UNDEF"))"

# --- 6. link (the kit's line + the RADV recipe) ---------------------------------------------------------
log "linking"
CXX_RUNTIME=("$SYSROOT/lib/libc++.a" "$SYSROOT/lib/libc++abi.a" "$SYSROOT/lib/libunwind.a")
# SDK link stubs, minus the WebKit-process and system-process kernel variants: apps import libkernel only.
SDK_STUBS=()
for so in "$STUBDIR"/*.so; do
    case "$(basename "$so")" in libkernel_web.so|libScePosixForWebKit.so|libkernel_sys.so) continue ;; esac
    SDK_STUBS+=("$so")
done
# libc.a without its raw-syscall members (a title may not execute the syscall instruction)
LIBC_TITLE="$BUILD/obj/libc_title.a"
rm -rf "$BUILD/libc-title"; mkdir -p "$BUILD/libc-title"
(cd "$BUILD/libc-title" && "$AR" x "$SYSROOT/lib/libc.a" && rm -f mman.o mount.o syscalls.o && "$AR" rcs "$LIBC_TITLE" ./*.o)
LINK_LOG="$BUILD/link.log"
printf 'int ps5_import_count(void) { return 0; }\nconst char *ps5_import_null(int i) { (void)i; return 0; }\n' > "$BUILD/obj/import_check0.c"
"$TCC" -std=gnu11 -O1 -fPIC -w "${TFLAGS[@]}" -c "$BUILD/obj/import_check0.c" -o "$BUILD/obj/import_check0.o"
link_app() {
    # The kit's line (--allow-multiple-definition for evo_dns's resolver names; --wrap=ioctl for
    # std's set_nonblocking → SO_NBIO) with the RADV recipe's wraps/bindings and inputs spliced in:
    # the archive whole, then libc++/libc++abi/libunwind + builtins + the platform layer as a group.
    # --no-dynamic-linker (+ -z nodynamic-undefined-weak, the name newer lld gives the same rule):
    # Mesa's dispatch tables name every Vulkan entry point through a weak reference, hundreds of
    # which nothing defines (vk_common_*, wsi_*); without this lld exports each as a dynamic import
    # and the converter refuses them. With it they resolve to 0 at link time, as PS5_Vulkan's and
    # PS5_RetroArch's titles link (tools/check-driver.sh, build-title.sh). std's weak references
    # (posix_spawn_file_actions_addchdir_np) resolve the same way.
    "$LLD" -T "$NATIVE/ps5-pie.ld" --eh-frame-hdr --allow-multiple-definition \
        --no-dynamic-linker -z nodynamic-undefined-weak \
        --wrap=ioctl "${radv_link_flags[@]}" \
        --version-script "$NATIVE/app-symbols.map" --exclude-libs=ALL --error-limit=0 \
        -L "$BUILD/obj" -e _start -o "$BUILD/llvm-pie.elf" \
        "${RUNTIME_OBJS[@]}" "${OBJS[@]}" "$@" "${PRX_STUB_SOS[@]}" \
        --start-group "${RUST_LIBS[@]}" --end-group "${CXX_RUNTIME[@]}" \
        "${radv_link_inputs[@]}" \
        --as-needed "${SDK_STUBS[@]}" "${PRX_STUB_SOS[@]}" \
        --start-group "$LIBC_TITLE" --end-group \
        2> "$LINK_LOG"
}
if ! link_app "$BUILD/obj/import_check0.o"; then
    grep -oE "undefined symbol: .*" "$LINK_LOG" | sed 's/^undefined symbol: //' | sort -u > "$BUILD/undefined-symbols.txt" || true
    if [[ -s "$BUILD/undefined-symbols.txt" ]]; then
        echo "   $(wc -l < "$BUILD/undefined-symbols.txt") undefined symbols (build/undefined-symbols.txt):"
        head -60 "$BUILD/undefined-symbols.txt" | sed 's/^/     /'
    else tail -30 "$LINK_LOG"; fi
    die "link failed — send me build/undefined-symbols.txt or the lines above"
fi
# Runtime import check: the loader binds an import the console does not export to NULL, silently.
# Relink with a table of every import; the shell can toast the NULL ones at start. The imports are
# what the converter sees: the undefined entries of the dynamic symbol table (not nm's static
# table, where Mesa's unresolved weak entry points still appear as 'w').
IMPORTS="$BUILD/imports.txt"
"$READELF" --dyn-syms "$BUILD/llvm-pie.elf" | awk '$7=="UND" && $8!="" {print $8}' | sed 's/@.*//' | sort -u > "$IMPORTS"
python3 - "$IMPORTS" "$BUILD/obj/import_check.c" <<'PY'
import sys
names = [n for n in open(sys.argv[1]).read().split() if n and not n.startswith("ps5_import")]
c = ["/* generated by build-title.sh: every import of the eboot */"]
c += ["extern void %s(void) __asm__(\"%s\") __attribute__((weak));" % (n.replace('@','_').replace('.','_'), n) for n in names]
c += ["static const void *const s_addr[] = {"] + ["    (const void *)&%s," % n.replace('@','_').replace('.','_') for n in names] + ["};"]
c += ["static const char *const s_name[] = {"] + ['    "%s",' % n for n in names] + ["};"]
c += ["int ps5_import_count(void) { return %d; }" % len(names),
      "const char *ps5_import_null(int i) { return s_addr[i] ? 0 : s_name[i]; }"]
open(sys.argv[2], "w").write("\n".join(c) + "\n")
PY
"$TCC" -std=gnu11 -O1 -fPIC -w "${TFLAGS[@]}" -c "$BUILD/obj/import_check.c" -o "$BUILD/obj/import_check.o"
link_app "$BUILD/obj/import_check.o" || { tail -30 "$LINK_LOG"; die "relink with the import check failed"; }
ok "linked $(stat -c %s "$BUILD/llvm-pie.elf") bytes, $(wc -l < "$IMPORTS") imports"
if "$READELF" -d "$BUILD/llvm-pie.elf" | grep NEEDED | grep -E 'libkernel_|PosixForWebKit'; then
    die "the app imports a kernel variant (above): it would die before main"
fi
"$READELF" -d "$BUILD/llvm-pie.elf" | grep NEEDED | sed 's/^ */   /'
if "$READELF" -l "$BUILD/llvm-pie.elf" | grep -q 'TLS'; then die "the ELF has a TLS segment"; fi
# every import must be something a library THIS title links exports: the SDK stubs (minus the
# kernel variants) and the generated AGC stubs
"$NM" -D --defined-only "${SDK_STUBS[@]}" "${PRX_STUB_SOS[@]}" 2>/dev/null | awk 'NF==3{print $3}' | sort -u > "$BUILD/sdk-exports.txt"
if missing=$(comm -23 "$IMPORTS" "$BUILD/sdk-exports.txt" | grep -v '^ps5_import'); [[ -n "$missing" ]]; then
    echo "   imports no linked library exports (the loader would bind them to NULL):"; echo "$missing" | sed 's/^/     /'
    die "unexported imports — send me the list above"
fi
ok "import check: all $(wc -l < "$IMPORTS") imports are exported by the title's libraries"
# No raw syscalls: a title that executes the syscall instruction dies (SYSTEM_ILLEGAL_FUNCTION_CALL).
if [[ -x "$OBJDUMP" ]]; then
    rawfns=$("$OBJDUMP" -d --no-show-raw-insn "$BUILD/llvm-pie.elf" | awk '/^[0-9a-f]+ <.*>:$/{f=$2} /[[:space:]](syscall|sysenter)$/{print f}' | sort -u | tr '\n' ' ')
    if [[ -n "$rawfns" ]]; then
        echo "   functions that execute a raw syscall (illegal in a title): $rawfns"
        die "raw syscalls in the image — send me the function names above"
    fi
    ok "no raw syscall instructions in the image"
fi

# --- 7. convert, sign, assemble --------------------------------------------------------------------------
log "converting + signing"
CONV_STUB_ARGS=(); for so in "${PRX_STUB_SOS[@]}"; do CONV_STUB_ARGS+=(--stub "$so"); done
"$TOOL" link --in "$BUILD/llvm-pie.elf" --out "$BUILD/eboot.elf" \
    --stub-dir "$STUBDIR" "${CONV_STUB_ARGS[@]}" \
    --module-sdk "$MODULE_SDK" --companion-sdk "$COMPANION_SDK" --file-name eboot.elf
rm -rf -- "$APPDIR"; mkdir -p "$APPDIR/sce_sys" "$APPDIR/sce_module"
"$TOOL" self --sign --in "$BUILD/eboot.elf" --out "$APPDIR/eboot.bin" --magic "$FSELF_MAGIC"
cp "$PARAM" "$APPDIR/sce_sys/param.json"
cp "$LIBC_PRX" "$APPDIR/sce_module/libc.prx"
for asset in icon0.png pic0.png pic1.png pic0.dds pic1.dds; do
    [[ -f "$HERE/sce_sys/$asset" ]] && cp "$HERE/sce_sys/$asset" "$APPDIR/sce_sys/"
done
cp "$HERE/controls.txt" "$APPDIR/controls.txt"   # the example mapping file (all comments: no effect until edited)
cp "$HERE"/presets/*.controls.txt "$APPDIR/"     # per-game presets: /app0/<movie>.controls.txt (src/controls.rs)
mkdir -p "$APPDIR/games"                          # where the menu looks (shell/main.c GAMES_DIR); yours on the console
cp "$HERE/games-README.txt" "$APPDIR/games/README.txt"
grep -q "\"titleId\": \"$TITLE_ID\"" "$PARAM" || die "param.json titleId is not $TITLE_ID"
grep -q "/data/homebrew/$TITLE_ID/games" "$HERE/shell/main.c" || die "shell/main.c GAMES_DIR is not /data/homebrew/$TITLE_ID/games"
"$TOOL" self --inspect --file "$APPDIR/eboot.bin" | grep -E "integrity|digest" | sed 's/^/   /' || true
ZIP="$BUILD/app/Ruffle5-$TITLE_ID.zip"; rm -f "$ZIP"
(cd "$BUILD/app" && zip -qr "$ZIP" "$TITLE_ID")
log "done: $APPDIR"
echo "   $(du -sh "$APPDIR/eboot.bin" | cut -f1) eboot.bin · zip: $ZIP"
echo "   install: copy the $TITLE_ID folder to /data/homebrew/ on the console (FTP), so that"
echo "   /data/homebrew/$TITLE_ID/eboot.bin exists; a 'Ruffle5' tile appears under Games."
echo "   UPDATING: copy eboot.bin (and sce_sys/, controls.txt, *.controls.txt) over the old ones --"
echo "   do not delete the folder first: /data/homebrew/$TITLE_ID/games/ is where your games live."
if [[ $RUFFLE == 1 ]]; then
    echo "   3b: Ruffle5 opens on a menu of /data/homebrew/$TITLE_ID/games/ (*.swf, or one game per"
    echo "       sub-folder with a cover.png); Cross plays, hold OPTIONS in a game to come back, hold"
    echo "       Circle in the menu to close. test-swf/bounce.swf is a known-good game. Default controls:"
    echo "       d-pad/left stick = arrows,"
    echo "       Cross = Space, Square = Z, Triangle = X, Circle = C, L1 = Shift, R1 = Ctrl, L3/R3 = A/S,"
    echo "       Options = Enter; touchpad = mouse (touch to point, click to click; a pointer is drawn),"
    echo "       right stick moves the pointer, R2/L2 = left/right button. Change any of it per game:"
    echo "       controls.txt (a commented example is in the title folder) -> /data/ruffle/controls.txt"
    echo "       or /data/ruffle/<movie>.controls.txt; presets for known games ship in the title folder"
    echo "       ($(ls "$HERE"/presets/*.controls.txt | xargs -n1 basename | sed 's/.controls.txt//' | tr '\n' ' ')). Log: /data/ruffle/ruffle.log"
else
    echo "   3a: the screen should cycle through colours for 8 s, with toasts. Log: /data/ruffle/ruffle.log"
fi
