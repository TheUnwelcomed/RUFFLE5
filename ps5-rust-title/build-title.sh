#!/usr/bin/env bash
# PS5_RUST_TITLE_BUILD_V4 — package the Rust test program as a PS5 title: a PPSA folder for
# /data/homebrew, a tile under Games, launched from the home screen.
# SPDX-License-Identifier: GPL-3.0-or-later
#
# The packaging tail is BlackBearReloaded's native-app tooling (ps5-native-app-boilerplate /
# ProsperoLight, GPL-3.0-or-later) exactly as Jelly5 vendors and runs it — the loader constants,
# link line and PRX rules below are the ones Jelly5's app ships with ("hardware-validated").
# The Rust side comes from ../ruffle-ps5-m1 (the milestone-1 kit): its build.sh produces the
# staticlib libps5_rust_hello.a with the same target and shim as the payload.
#
#   ./build-title.sh              → build/app/PPSA99610/{eboot.bin, sce_sys/, sce_module/libc.prx}
#                                   and build/app/RustHello-PPSA99610.zip
#   PS5_RUST_M1=<path>            the milestone-1 kit (default ../ruffle-ps5-m1)
#   PS5_RUST_STANDIN=1            no Rust: a C stand-in plays the library (tests the tail alone)
set -euo pipefail
KIT="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "${KIT}"
log() { echo "==> $*"; }
ok()  { echo "   ✓ $*"; }
die() { echo "error: $*" >&2; exit 1; }
need() { [[ -e "$1" ]] || die "$2"; }

M1="${PS5_RUST_M1:-${KIT}/../ruffle-ps5-m1}"
M1="$(cd "${M1}" 2>/dev/null && pwd || true)"
STANDIN="${PS5_RUST_STANDIN:-0}"
[[ -n "${M1}" || "${STANDIN}" == 1 ]] || die "milestone-1 kit not found (PS5_RUST_M1=<path to ruffle-ps5-m1>)"

# --- 1. environment: the same discovery as the milestone-1 build.sh -------------------------------
[[ -f "$HOME/.cargo/env" ]] && source "$HOME/.cargo/env"
if [[ -z "${LLVM_CONFIG:-}" && -x "$HOME/llvm18/bin/llvm-config" ]]; then
    export LLVM_CONFIG="$HOME/llvm18/bin/llvm-config"; export PATH="$HOME/llvm18/bin:${PATH}"
fi
if [[ -z "${PS5_PAYLOAD_SDK:-}" ]]; then
    if [[ -x "$HOME/ps5-payload-sdk/bin/prospero-clang" ]]; then export PS5_PAYLOAD_SDK="$HOME/ps5-payload-sdk"
    else die "PS5_PAYLOAD_SDK not set (the milestone-1 build.sh --get-sdk puts it in ~/ps5-payload-sdk)"; fi
fi
export PS5_PAYLOAD_SDK
SDK="${PS5_PAYLOAD_SDK}"; SYSROOT="${SDK}/target"; STUBDIR="${SYSROOT}/lib"
TCC="${SDK}/bin/prospero-clang"; TCXX="${SDK}/bin/prospero-clang++"; LLD="${SDK}/bin/prospero-lld"
LLVM_BIN="$("${SDK}/bin/prospero-llvm-config" --bindir 2>/dev/null || true)"
[[ -n "${LLVM_BIN}" ]] || die "clang/lld not found (LLVM_CONFIG, or ~/llvm18 from the milestone-1 setup)"
NM="${LLVM_BIN}/llvm-nm"; READELF="${LLVM_BIN}/llvm-readelf"; AR="${LLVM_BIN}/llvm-ar"
for t in "$TCC" "$TCXX" "$LLD" "$NM" "$READELF" "$AR"; do [[ -x "$t" ]] || die "missing tool: $t"; done
NATIVE="${KIT}/native-app"
TOOL="${KIT}/host/ps5-native-tool"
if [[ ! -x "${TOOL}" ]]; then
    log "building the host converter (clang++ + zlib)"
    ZI="${ZLIB_INCLUDE:-/usr/include}"; ZA="${ZLIB_ARCHIVE:-}"
    [[ -n "$ZA" ]] || for c in /usr/lib/x86_64-linux-gnu/libz.a /usr/lib/libz.a; do [[ -f "$c" ]] && ZA="$c"; done
    [[ -n "$ZA" && -f "$ZI/zlib.h" ]] || die "static zlib not found: set ZLIB_INCLUDE and ZLIB_ARCHIVE, or use the prebuilt host/ps5-native-tool"
    clang++ -std=c++20 -O2 -static -I "$ZI" "${NATIVE}/native_app_builder.cpp" "${NATIVE}/self_container.cpp" \
        "${NATIVE}/elf_object.cpp" "${NATIVE}/sce_module_writer.cpp" "$ZA" -o "${TOOL}"
fi
LIBC_PRX="${NATIVE}/runtime/libc.prx"; need "${LIBC_PRX}" "native-app/runtime/libc.prx missing"
if command -v sha256sum >/dev/null; then
    (cd "${NATIVE}/runtime" && sha256sum -c libc.prx.sha256 >/dev/null) || die "libc.prx does not match its pinned digest"
fi
ok "SDK ${SDK}, $("$TCC" --version | head -1), converter ok, libc.prx digest ok"

# Hardware-validated loader profile (Jelly5 app/scripts/build.sh).
MODULE_SDK=0x02000009
COMPANION_SDK=0x08050001
FSELF_MAGIC=0x1D3D154F
TFLAGS=(-fno-plt -fno-stack-protector -ffunction-sections -fdata-sections)

PARAM="${KIT}/sce_sys/param.json"
TITLE_ID="$(python3 -c 'import json,sys; print(json.load(open(sys.argv[1]))["titleId"])' "${PARAM}")"
BUILD="${KIT}/build"; APPDIR="${BUILD}/app/${TITLE_ID}"
rm -rf "${BUILD}/obj" "${BUILD}/stubs"; mkdir -p "${BUILD}/obj" "${BUILD}/stubs" "${BUILD}/app"

# --- 2. the Rust library ------------------------------------------------------------------------
if [[ "${STANDIN}" == 1 ]]; then
    log "stand-in: C plays the Rust library (PS5_RUST_STANDIN=1)"
    "${TCC}" -std=gnu11 -O2 -Wall "${TFLAGS[@]}" -c "${KIT}/shell/standin.c" -o "${BUILD}/obj/standin.o"
    RUST_LIBS=("${BUILD}/obj/standin.o")
    # the shim and the archives std expects, built the way the milestone-1 build.sh builds them
    SHIMSRC="${M1:-${KIT}/../ruffle-ps5-m1}/shim"
    [[ -d "$SHIMSRC" ]] || die "shim sources not found at ${SHIMSRC}"
    "${TCC}" -O2 -Wall -DPS5_TITLE "${TFLAGS[@]}" -c "${SHIMSRC}/ps5_shim.c" -o "${BUILD}/obj/ps5_shim.o"
    "${AR}" rcs "${BUILD}/obj/libps5shim.a" "${BUILD}/obj/ps5_shim.o"
    RUST_LIBS+=("${BUILD}/obj/libps5shim.a")
else
    log "Rust: ${M1}/build.sh (staticlib + payload, same target and shim)"
    (cd "${M1}" && ./build.sh) || die "the milestone-1 build failed"
    RUST_A="${M1}/target/x86_64-sie-ps5/release/libps5_rust_hello.a"
    need "${RUST_A}" "staticlib missing: ${RUST_A} (Cargo.toml needs crate-type staticlib)"
    # the shim in title mode (-DPS5_TITLE): libkernel-backed bodies for the POSIX calls the SDK's
    # libc.a implements with raw syscalls, which a title may not execute (SYSTEM_ILLEGAL_FUNCTION_CALL)
    "${TCC}" -O2 -Wall -DPS5_TITLE "${TFLAGS[@]}" -c "${M1}/shim/ps5_shim.c" -o "${BUILD}/obj/ps5_shim_title.o"
    "${TCC}" -O2 -Wall "${TFLAGS[@]}" -c "${M1}/shim/execinfo_stub.c" -o "${BUILD}/obj/execinfo_stub.o"
    "${AR}" rcs "${BUILD}/obj/libps5shim_title.a" "${BUILD}/obj/ps5_shim_title.o" "${BUILD}/obj/execinfo_stub.o"
    RUST_LIBS=("${RUST_A}" "${BUILD}/obj/libps5shim_title.a")
    ok "$(du -h "${RUST_A}" | cut -f1) ${RUST_A##*/} + the shim in title mode"
fi

# --- 3. the shell and the runtime pieces ----------------------------------------------------------
log "compiling the shell and the native-app runtime"
"${TCC}" -std=gnu11 -O2 -Wall "${TFLAGS[@]}" -c "${KIT}/shell/main.c" -o "${BUILD}/obj/main.o"
"${TCC}" -std=gnu11 -O2 -Wall "${TFLAGS[@]}" -c "${KIT}/shell/boot_log.c" -o "${BUILD}/obj/boot_log.o"
"${TCXX}" -std=c++20 -O2 -fno-exceptions -fno-rtti "${TFLAGS[@]}" -c "${NATIVE}/app_crt.cpp" -o "${BUILD}/obj/app_crt.o"
"${TCXX}" -std=c++20 -O2 -fno-exceptions -fno-rtti "${TFLAGS[@]}" -c "${NATIVE}/app_cpp_runtime.cpp" -o "${BUILD}/obj/app_cpp_runtime.o"
# libunwind.a carries a dependent-library note for "pthread": lld looks for libpthread on the -L
# path. Jelly5 answers it with an archive of the C++ runtime object (the console's pthreads come
# from libkernel); same here.
"${AR}" rcs "${BUILD}/obj/libpthread.a" "${BUILD}/obj/app_cpp_runtime.o"
"${TCC}" -std=gnu11 -O2 -w "${TFLAGS[@]}" -c "${NATIVE}/stubs/libc_ext.c" -o "${BUILD}/obj/libc_ext.o"
"${TCC}" -std=gnu11 -O2 -w "${TFLAGS[@]}" -c "${NATIVE}/stubs/evo_dns.c" -o "${BUILD}/obj/evo_dns.o"
"${TCC}" -std=gnu11 -O2 -w "${TFLAGS[@]}" -c "${NATIVE}/stubs/malloc_shim.c" -o "${BUILD}/obj/malloc_shim.o"
# malloc_shim first: its malloc/free/posix_memalign win over the clean-room libc.prx's bounded heap
RUNTIME_OBJS=("${BUILD}/obj/malloc_shim.o" "${BUILD}/obj/app_crt.o" "${BUILD}/obj/app_cpp_runtime.o"
              "${BUILD}/obj/libc_ext.o" "${BUILD}/obj/evo_dns.o")
OBJS=("${BUILD}/obj/main.o" "${BUILD}/obj/boot_log.o")

# --- 4. PRX import stubs for system modules the SDK has no link stub for ----------------------------
# (none expected for this title: it imports libkernel and libSceLibcInternal only)
UNDEF="${BUILD}/obj-undef.txt"
for o in "${OBJS[@]}" "${RUNTIME_OBJS[@]}" "${RUST_LIBS[@]}"; do
    "${NM}" -u "${o}" 2>/dev/null | grep -oE '\bsce[A-Za-z0-9_]+' || true
done | sort -u > "${UNDEF}"
PRX_STUB_SOS=()
for base in libSceVideodec2 libSceAudiodec libSceAgc libSceAgcDriver libSceCommonDialog libSceWebBrowserDialog; do
    csrc="${BUILD}/stubs/${base}.c"; : > "${csrc}"
    while read -r s; do
        case "${base}" in
            libSceAgcDriver) [[ "${s}" == sceAgcDriver* ]] || continue ;;
            libSceAgc)       [[ "${s}" == sceAgc* && "${s}" != sceAgcDriver* ]] || continue ;;
            libSceVideodec2) [[ "${s}" == sceVideodec2* ]] || continue ;;
            libSceAudiodec)  [[ "${s}" == sceAudiodec* ]] || continue ;;
            libSceCommonDialog) [[ "${s}" == sceCommonDialog* ]] || continue ;;
            libSceWebBrowserDialog) [[ "${s}" == sceWebBrowserDialog* ]] || continue ;;
        esac
        echo "void ${s}(void){}" >> "${csrc}"
    done < "${UNDEF}"
    [[ -s "${csrc}" ]] || continue
    so="${BUILD}/stubs/${base}.so"
    "${TCC}" -shared -nostdlib -nodefaultlibs -fPIC -Wl,-soname,"${base}.sprx" -o "${so}" "${csrc}"
    PRX_STUB_SOS+=("${so}")
    printf '   %-28s %s syms\n' "${base}.sprx" "$(wc -l < "${csrc}")"
done
ok "sce* imports: $(wc -l < "${UNDEF}") ($(tr '\n' ' ' < "${UNDEF}"))"

# --- 5. link (Jelly5's line) --------------------------------------------------------------------------
log "linking"
CXX_RUNTIME=("${SYSROOT}/lib/libc++.a" "${SYSROOT}/lib/libc++abi.a" "${SYSROOT}/lib/libunwind.a")
# SDK link stubs, minus the WebKit-process and system-process kernel variants:
# apps import libkernel only; a kernel variant by any name dies before main.
SDK_STUBS=()
for so in "${STUBDIR}"/*.so; do
    case "$(basename "${so}")" in libkernel_web.so|libScePosixForWebKit.so|libkernel_sys.so) continue ;; esac
    SDK_STUBS+=("${so}")
done
# libc.a without its raw-syscall members (mman.o, mount.o, syscalls.o): a title may not execute
# the syscall instruction. With them gone, anything that still reaches one of those functions is a
# named undefined symbol here — the shim's PS5_TITLE section is where it gets a libkernel body.
LIBC_TITLE="${BUILD}/obj/libc_title.a"
rm -rf "${BUILD}/libc-title"; mkdir -p "${BUILD}/libc-title"
(cd "${BUILD}/libc-title" && "${AR}" x "${SYSROOT}/lib/libc.a" && rm -f mman.o mount.o syscalls.o && "${AR}" rcs "${LIBC_TITLE}" ./*.o)
ok "libc.a minus mman.o mount.o syscalls.o → $(basename "${LIBC_TITLE}")"
LINK_LOG="${BUILD}/link.log"
# first link with an empty import table; the second carries the real one
printf 'int ps5_import_count(void) { return 0; }\nconst char *ps5_import_null(int i) { (void)i; return 0; }\n' > "${BUILD}/obj/import_check0.c"
"${TCC}" -std=gnu11 -O1 -fPIC -w "${TFLAGS[@]}" -c "${BUILD}/obj/import_check0.c" -o "${BUILD}/obj/import_check0.o"
link_app() {
    # --allow-multiple-definition: evo_dns.o (first) supplies getaddrinfo & co. on the console
    # resolver; libc.a still comes in for the rest of netdb.
    # --wrap=ioctl: a sandboxed title's ioctl(FIONBIO) is EACCES, and so is fcntl(F_SETFL,
    # O_NONBLOCK) (v8/v9's probes on the console); std's set_nonblocking is the former. The shim
    # (title mode, PS5_RUST_SHIM_V4) turns it into libSceNet's SO_NBIO socket option and passes
    # every other request to __real_ioctl = libkernel's.
    "${LLD}" -T "${NATIVE}/ps5-pie.ld" --eh-frame-hdr --allow-multiple-definition \
        --wrap=ioctl \
        --version-script "${NATIVE}/app-symbols.map" --exclude-libs=ALL --error-limit=0 \
        -L "${BUILD}/obj" -e _start -o "${BUILD}/llvm-pie.elf" \
        "${RUNTIME_OBJS[@]}" "${OBJS[@]}" "$@" "${PRX_STUB_SOS[@]}" \
        --start-group "${RUST_LIBS[@]}" --end-group "${CXX_RUNTIME[@]}" \
        --as-needed "${SDK_STUBS[@]}" "${PRX_STUB_SOS[@]}" \
        --start-group "${LIBC_TITLE}" --end-group \
        2> "${LINK_LOG}"
}
if ! link_app "${BUILD}/obj/import_check0.o"; then
    grep -oE "undefined symbol: .*" "${LINK_LOG}" | sed 's/^undefined symbol: //' | sort -u > "${BUILD}/undefined-symbols.txt" || true
    if [[ -s "${BUILD}/undefined-symbols.txt" ]]; then
        echo "   $(wc -l < "${BUILD}/undefined-symbols.txt") undefined symbols (build/undefined-symbols.txt):"
        head -60 "${BUILD}/undefined-symbols.txt" | sed 's/^/     /'
    else tail -30 "${LINK_LOG}"; fi
    die "link failed"
fi
# Runtime import check: the loader binds an import the console does not export to NULL,
# silently. Relink with a table of every import; the shell toasts the NULL ones at start.
IMPORTS="${BUILD}/imports.txt"
# weak undefined ("w") count too: the converter refuses any undefined symbol no stub exports
"${NM}" "${BUILD}/llvm-pie.elf" | awk '$1=="U"||$1=="w"{print $2}' | sort -u > "${IMPORTS}"
python3 - "${IMPORTS}" "${BUILD}/obj/import_check.c" <<'PY'
import sys
names = [n for n in open(sys.argv[1]).read().split() if n and not n.startswith("ps5_import")]
c = ["/* generated by build-title.sh: every import of the eboot */"]
# weak: otherwise the compiler takes &f as never NULL and folds the whole table away
c += ["extern void %s(void) __asm__(\"%s\") __attribute__((weak));" % (n.replace('@','_').replace('.','_'), n) for n in names]
c += ["static const void *const s_addr[] = {"] + ["    (const void *)&%s," % n.replace('@','_').replace('.','_') for n in names] + ["};"]
c += ["static const char *const s_name[] = {"] + ['    "%s",' % n for n in names] + ["};"]
c += ["int ps5_import_count(void) { return %d; }" % len(names),
      "const char *ps5_import_null(int i) { return s_addr[i] ? 0 : s_name[i]; }"]
open(sys.argv[2], "w").write("\n".join(c) + "\n")
PY
"${TCC}" -std=gnu11 -O1 -fPIC -w "${TFLAGS[@]}" -c "${BUILD}/obj/import_check.c" -o "${BUILD}/obj/import_check.o"
link_app "${BUILD}/obj/import_check.o" || { tail -30 "${LINK_LOG}"; die "relink with the import check failed"; }
ok "linked $(stat -c %s "${BUILD}/llvm-pie.elf") bytes, $(wc -l < "${IMPORTS}") imports"
if "${READELF}" -d "${BUILD}/llvm-pie.elf" | grep NEEDED | grep -E 'libkernel_|PosixForWebKit'; then
    die "the app imports a kernel variant (above): it would die before main"
fi
"${READELF}" -d "${BUILD}/llvm-pie.elf" | grep NEEDED | sed 's/^ */   /'
if "${READELF}" -l "${BUILD}/llvm-pie.elf" | grep -q 'TLS'; then die "the ELF has a TLS segment"; fi
# every import must be something a library THIS title links exports: the SDK stubs minus the
# WebKit/system kernel variants (__getcwd, the *at family, strcasestr… live only there and bind
# to NULL in a title — PS5_RetroArch's src/platform_wraps.c lists the ones that bit them)
"${NM}" -D --defined-only "${SDK_STUBS[@]}" 2>/dev/null | awk 'NF==3{print $3}' | sort -u > "${BUILD}/sdk-exports.txt"
if missing=$(comm -23 "${IMPORTS}" "${BUILD}/sdk-exports.txt" | grep -v '^ps5_import'); [[ -n "$missing" ]]; then
    echo "   imports no SDK library exports (the loader would bind them to NULL):"; echo "$missing" | sed 's/^/     /'
    die "unexported imports"
fi
ok "import check: all $(wc -l < "${IMPORTS}") imports are exported by system libraries"
# No raw syscalls: a title that executes the `syscall` instruction dies with
# SYSTEM_ILLEGAL_FUNCTION_CALL (0xa002030a). The SDK's libc.a has such members (mman.o, mount.o,
# syscalls.o); anything that pulled one in shows up here, by function.
OBJDUMP="${LLVM_BIN}/llvm-objdump"
if [[ -x "$OBJDUMP" ]]; then
    rawfns=$("$OBJDUMP" -d --no-show-raw-insn "${BUILD}/llvm-pie.elf" | awk '/^[0-9a-f]+ <.*>:$/{f=$2} /[[:space:]](syscall|sysenter)$/{print f}' | sort -u | tr '\n' ' ')
    if [[ -n "$rawfns" ]]; then
        echo "   functions that execute a raw syscall (illegal in a title): $rawfns"
        die "raw syscalls in the image — give them libkernel-backed bodies in the shim (PS5_TITLE section)"
    fi
    ok "no raw syscall instructions in the image"
fi

# --- 6. convert, sign, assemble ---------------------------------------------------------------------
log "converting + signing"
CONV_STUB_ARGS=(); for so in "${PRX_STUB_SOS[@]}"; do CONV_STUB_ARGS+=(--stub "${so}"); done
"${TOOL}" link --in "${BUILD}/llvm-pie.elf" --out "${BUILD}/eboot.elf" \
    --stub-dir "${STUBDIR}" "${CONV_STUB_ARGS[@]}" \
    --module-sdk "${MODULE_SDK}" --companion-sdk "${COMPANION_SDK}" --file-name eboot.elf
rm -rf -- "${APPDIR}"; mkdir -p "${APPDIR}/sce_sys" "${APPDIR}/sce_module"
"${TOOL}" self --sign --in "${BUILD}/eboot.elf" --out "${APPDIR}/eboot.bin" --magic "${FSELF_MAGIC}"
cp "${PARAM}" "${APPDIR}/sce_sys/param.json"
cp "${LIBC_PRX}" "${APPDIR}/sce_module/libc.prx"
for asset in icon0.png pic0.png pic1.png pic0.dds pic1.dds; do
    [[ -f "${KIT}/sce_sys/${asset}" ]] && cp "${KIT}/sce_sys/${asset}" "${APPDIR}/sce_sys/"
done
"${TOOL}" self --inspect --file "${APPDIR}/eboot.bin" | grep -E "integrity|digest" | sed 's/^/   /' || true
ZIP="${BUILD}/app/RustHello-${TITLE_ID}.zip"; rm -f "${ZIP}"
(cd "${BUILD}/app" && zip -qr "${ZIP}" "${TITLE_ID}")
log "done: ${APPDIR}"
echo "   $(du -sh "${APPDIR}/eboot.bin" | cut -f1) eboot.bin · zip: ${ZIP}"
echo "   install: copy the ${TITLE_ID} folder to /data/homebrew/ on the console (ps5upload or FTP),"
echo "   so that /data/homebrew/${TITLE_ID}/eboot.bin exists; ShadowMount+ adds a 'Rust Hello' tile under Games."
echo "   result: a toast per check; the report is /app0/ps5-rust-hello/report.txt = /data/homebrew/PPSA99610/ps5-rust-hello/report.txt"
