#!/usr/bin/env bash
# build-radv-server.sh (v11) — build the static RADV archive on the Bytesized server (no root).
#
# It does what SETUP-RADV.md describes by hand would: make a host clang available, install
# meson+ninja for your user, clone Mihawk's three public repos as siblings, and run THEIR OWN
# setup + build scripts (which pin their own revisions). Output: libvulkan_radeon.ps5.a.
#
# Usage:   bash build-radv-server.sh [BASE_DIR]
#   BASE_DIR defaults to ~/ps5gpu. Safe to re-run: existing clones and tools are reused.
#   If clang isn't found, set it:  CLANG=/path/to/clang bash build-radv-server.sh
set -euo pipefail

BASE="${1:-$HOME/ps5gpu}"
SHIM="$HOME/.ruffle-build-bin"
export PATH="$HOME/.local/bin:$SHIM:$PATH"

log() { printf '\n=== %s ===\n' "$*"; }
die() { printf '\nERROR: %s\n' "$*" >&2; exit 1; }

# ---- 00. LLVM 18 (clang + lld + llvm-config) — the PS5 TARGET compiler ----------------------
# The fork SDK's Makefile.inc finds its compiler through `llvm-config --bindir`; with no llvm-config
# on PATH it silently ends up running `--bindir` and `/clang` (the "make: /clang: No such file"
# error). Milestone 1 installed an official LLVM 18 tarball in ~/llvm18; reuse it (or fetch it).
log "00. LLVM 18 for the PS5 target (llvm-config / clang / ld.lld)"
LLVM_URL="https://github.com/llvm/llvm-project/releases/download/llvmorg-18.1.8/clang+llvm-18.1.8-x86_64-linux-gnu-ubuntu-18.04.tar.xz"
if [[ -z "${LLVM_CONFIG:-}" ]]; then
    if [[ -x "$HOME/llvm18/bin/llvm-config" ]]; then
        LLVM_CONFIG="$HOME/llvm18/bin/llvm-config"
    elif lc="$(command -v llvm-config-18 llvm-config 2>/dev/null | head -1)" && [[ -n "$lc" ]]; then
        LLVM_CONFIG="$lc"
    else
        log "no LLVM found — downloading LLVM 18.1.8 into ~/llvm18 (about 1 GB, unpacks to ~2.5 GB)"
        curl -fL -o "$HOME/llvm18.tar.xz" "$LLVM_URL" || die "LLVM download failed; fetch it by hand into ~/llvm18 and re-run."
        mkdir -p "$HOME/llvm18"
        tar -xJf "$HOME/llvm18.tar.xz" -C "$HOME/llvm18" --strip-components=1
        rm -f "$HOME/llvm18.tar.xz"
        LLVM_CONFIG="$HOME/llvm18/bin/llvm-config"
    fi
fi
export LLVM_CONFIG
LLVM_BIN="$("$LLVM_CONFIG" --bindir)"
[[ -x "$LLVM_BIN/clang" && -x "$LLVM_BIN/ld.lld" ]] || die "$LLVM_BIN lacks clang or ld.lld — LLVM_CONFIG must point at a full LLVM 18+ (clang+lld)."
export PATH="$LLVM_BIN:$PATH"
printf 'LLVM_CONFIG=%s\n  %s\n' "$LLVM_CONFIG" "$("$LLVM_BIN/clang" --version | head -1)"

# ---- 0. a working host C/C++ compiler ------------------------------------------------------
# PS5_Vulkan's dep setup insists on `clang` by name, only for HOST-side helper compiles (zlib, the
# mesa-clc host tool). The PS5 target itself is built by the fork SDK's own clang, not this one. So
# any working host compiler does; we expose it as `clang`/`clang++` in a shim dir on PATH. We TEST
# that a candidate actually runs — your ~/ps5-payload-sdk/bin/clang is a wrapper pointing at a
# missing /llvm-clang, so it is skipped — and fall back to the server's gcc, which builds those
# host helpers fine.
log "0. finding a working host compiler"
# Probe with no stdin and a timeout: a broken SDK clang wrapper (yours points at a missing
# /llvm-clang) otherwise blocks forever when asked its version.
works() { timeout 10 "$1" --version </dev/null >/dev/null 2>&1; }
mkdir -p "$SHIM"
rm -f "$SHIM/clang" "$SHIM/clang++"        # clear any stale shim a previous run left pointing at a broken clang
hash -r

HOSTCC=""; HOSTCXX=""; USING=""
# Only use a clang if YOU point at a working one (CLANG=...); otherwise gcc, which builds the
# host-side helpers (zlib, the mesa-clc host tool) perfectly. The PS5 target is built by the fork
# SDK's own clang, not this compiler, so gcc here changes nothing about the output.
if [[ -n "${CLANG:-}" ]] && works "$CLANG"; then
    HOSTCC="$CLANG"; HOSTCXX="${CLANG}++"; works "$HOSTCXX" || HOSTCXX="$CLANG"; USING="clang"
elif works "$LLVM_BIN/clang"; then
    HOSTCC="$LLVM_BIN/clang"; HOSTCXX="$LLVM_BIN/clang++"; USING="clang 18 (LLVM_CONFIG)"
elif command -v gcc >/dev/null && works "$(command -v gcc)"; then
    HOSTCC="$(command -v gcc)"; HOSTCXX="$(command -v g++ || command -v gcc)"; USING="gcc"
elif command -v cc >/dev/null && works "$(command -v cc)"; then
    HOSTCC="$(command -v cc)"; HOSTCXX="$(command -v c++ || command -v cc)"; USING="cc"
else
    die "no working host compiler. Install gcc, or set CLANG=/path/to/a/working/clang and re-run."
fi
# mihawk's setup insists on a binary named `clang`; point that name at our working compiler.
ln -sf "$HOSTCC"  "$SHIM/clang"
ln -sf "$HOSTCXX" "$SHIM/clang++"
hash -r
# meson/make use the REAL compiler (so meson identifies it correctly), not the clang-named shim.
export CC="$HOSTCC" CXX="$HOSTCXX"
printf 'host compiler (%s): %s\n  %s\n' "$USING" "$HOSTCC" "$("$HOSTCC" --version 2>/dev/null | head -1)"

# ---- the rest of the tools -----------------------------------------------------------------
for c in git make wget rsync python3; do
    command -v "$c" >/dev/null || die "missing '$c' (git/make/wget/python3 should be on the server; rsync: ask the host if absent)."
done
if ! command -v meson >/dev/null; then
    log "installing meson + ninja for your user (pip --user; no root)"
    python3 -m pip install --user --upgrade meson ninja \
        || die "pip install meson ninja failed. Try: python3 -m pip install --user --upgrade pip, then re-run."
fi
command -v meson >/dev/null || die "meson still not on PATH. Add 'export PATH=\$HOME/.local/bin:\$PATH' to ~/.bashrc and re-run."
command -v ninja >/dev/null || die "ninja still not on PATH (should be in ~/.local/bin from pip)."
printf 'meson %s, ninja %s\n' "$(meson --version)" "$(ninja --version)"

# ---- 1. the three sibling repos ------------------------------------------------------------
log "1. cloning repos into $BASE (as siblings)"
mkdir -p "$BASE"
for repo in PS5_Vulkan PS5_Mesa PS5_PayloadSDK; do
    if git -C "$BASE/$repo" rev-parse HEAD >/dev/null 2>&1; then
        echo "  $repo already cloned — leaving it as is"
    else
        rm -rf "$BASE/$repo"      # clear a half-made dir from an earlier failed clone
        # --no-tags: these repos carry a duplicate tag that makes a normal clone abort
        #   (multiple updates for ref .../retroarch-v0.6.0-alpha.6); the setup scripts pin commits
        #   by SHA, not tags, so dropping tags is harmless.
        # --filter=blob:none: full commit/tree history, blobs on demand, so the Mesa fork is fast.
        git clone --no-tags --filter=blob:none "https://github.com/mihawk-99/$repo.git" "$BASE/$repo" \
            || die "git clone $repo failed (network/allowlist?). Clone it by hand into $BASE and re-run."
    fi
done

# The clone didn't preserve executable bits on the repos' scripts; restore them so internal
# ./script calls work, and invoke the entry scripts via bash either way.
find "$BASE/PS5_Vulkan" "$BASE/PS5_Mesa" "$BASE/PS5_PayloadSDK" -name '*.sh' -exec chmod +x {} + 2>/dev/null || true

# ---- 2. platform-layer SDK, then RADV ------------------------------------------------------
cd "$BASE/PS5_Vulkan"
log "2a. setup-native-dependencies.sh (downloads upstream SDK v0.42, installs the platform layer over it)"
bash tools/setup-native-dependencies.sh || die "setup-native-dependencies.sh failed — send me its output."

# ---- 2b0. what Mesa's host tool (mesa_clc) needs, none of it packaged on the server ------------
# build-radv.sh first builds mesa_clc + vtn_bindgen2 natively (they compile RADV's OpenCL kernels at
# build time). At this Mesa revision that needs: python mako/pyyaml/packaging; SPIRV-Tools >= 2024.1
# (Ubuntu 22.04 ships 2022.1); a SPIRV-LLVM-Translator matching LLVM 18.1; and LLVM+clang libraries.
# The LLVM 18 tarball ships only STATIC libLLVM*.a/libclang*.a (no libLLVM.so), while Mihawk's script
# hardcodes -Dshared-llvm=enabled — so we configure that one build directory ourselves with static
# LLVM, and build-radv.sh, seeing build.ninja already there, only runs ninja in it. The tarball's LLVM
# is built without RTTI, so Mesa must be too (cpp_rtti=false; Mesa checks and says so).
log "2b0. host tools for mesa_clc (python modules, SPIRV-Tools, SPIRV-LLVM-Translator; one-time)"
HOST="$BASE/host"; HOSTLIB="$HOST/lib"; HOSTSRC="$BASE/host-src"
mkdir -p "$HOSTLIB/pkgconfig" "$HOSTSRC"
LLVM_ROOT="$(dirname "$LLVM_BIN")"
ZLIB_ROOT="$BASE/PS5_Vulkan/.deps/native/zlib/root/usr"
[[ -f "$ZLIB_ROOT/lib/libz.a" ]] || die "expected setup's zlib at $ZLIB_ROOT/lib/libz.a — send me 2a's output."
JOBS="$(nproc 2>/dev/null || echo 4)"

python3 -c 'import mako, yaml, packaging' 2>/dev/null \
    || python3 -m pip install --user --upgrade mako pyyaml packaging \
    || die "pip install mako pyyaml packaging failed."
python3 -c 'import mako, yaml, packaging' || die "python3 still cannot import mako/yaml/packaging."

# cmake >= 3.22.1 (SPIRV-Tools' minimum); a pip wheel if the server's is missing or older.
cmake_ok() { command -v cmake >/dev/null && python3 - "$(cmake --version | head -1 | awk '{print $3}')" <<'PY'
import sys; v=tuple(int(x) for x in sys.argv[1].split('.')[:3]); sys.exit(0 if v>=(3,22,1) else 1)
PY
}
cmake_ok || python3 -m pip install --user --upgrade cmake || die "pip install cmake failed."
cmake_ok || die "cmake >= 3.22.1 still not on PATH."

# llvm-config says the static LLVM links with: -lrt -ldl -lpthread -lm -lz -ltinfo. On a server without
# the -dev packages, -lz and -ltinfo have no libz.so/libtinfo.so to resolve against; provide them in
# $HOSTLIB (zlib: the static one 2a built; tinfo: a link to the runtime library already installed).
ln -sf "$ZLIB_ROOT/lib/libz.a" "$HOSTLIB/libz.a"
if ! printf 'int main(void){return 0;}' | "$HOSTCC" -x c - -ltinfo -o /dev/null 2>/dev/null; then
    tinfo="$(ls /lib/x86_64-linux-gnu/libtinfo.so.6 /usr/lib/x86_64-linux-gnu/libtinfo.so.6 \
               /lib/x86_64-linux-gnu/libtinfo.so.5 /usr/lib/x86_64-linux-gnu/libtinfo.so.5 2>/dev/null | head -1)"
    [[ -n "$tinfo" ]] || die "no libtinfo.so.[56] on this system (LLVM links -ltinfo)."
    ln -sf "$tinfo" "$HOSTLIB/libtinfo.so"
    echo "  -ltinfo via $HOSTLIB/libtinfo.so -> $tinfo"
fi
export LIBRARY_PATH="$HOSTLIB${LIBRARY_PATH:+:$LIBRARY_PATH}"
export PKG_CONFIG_PATH="$HOSTLIB/pkgconfig:$ZLIB_ROOT/lib/pkgconfig${PKG_CONFIG_PATH:+:$PKG_CONFIG_PATH}"
export ZLIB_ROOT   # CMake's FindZLIB (LLVMConfig.cmake runs it) honours <Pkg>_ROOT

# On this server `git clone --branch <tag>` (and plain clones that follow tags) abort with
# "multiple updates for ref 'refs/tags/...' not allowed" — the upstream repos carry each tag once, so
# it is the server's git configuration mapping tags twice. Fetching one explicit revision into a bare
# init bypasses every configured refspec, so that is how every pinned checkout is made here.
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

# SPIRV-Tools v2025.4 (static libs + SPIRV-Tools.pc) with the SPIRV-Headers revision its DEPS pins
# (external/spirv-headers, where its CMake looks). Tests and their deps are skipped.
if [[ ! -f "$HOSTLIB/pkgconfig/SPIRV-Tools.pc" ]]; then
    echo "  building SPIRV-Tools v2025.4"
    fetch_rev "$HOSTSRC/SPIRV-Tools" https://github.com/KhronosGroup/SPIRV-Tools.git v2025.4
    fetch_rev "$HOSTSRC/SPIRV-Tools/external/spirv-headers" https://github.com/KhronosGroup/SPIRV-Headers.git \
        01e0577914a75a2569c846778c2f93aa8e6feddd
    cmake -S "$HOSTSRC/SPIRV-Tools" -B "$HOSTSRC/SPIRV-Tools/build" -G Ninja \
        -DCMAKE_BUILD_TYPE=Release -DCMAKE_INSTALL_PREFIX="$HOST" -DCMAKE_INSTALL_LIBDIR=lib \
        -DCMAKE_C_COMPILER="$HOSTCC" -DCMAKE_CXX_COMPILER="$HOSTCXX" \
        -DSPIRV_SKIP_TESTS=ON -DSPIRV_SKIP_EXECUTABLES=ON -DBUILD_SHARED_LIBS=OFF \
        >"$HOSTSRC/spirv-tools-cmake.log" 2>&1 || { tail -20 "$HOSTSRC/spirv-tools-cmake.log"; die "SPIRV-Tools cmake failed"; }
    ninja -C "$HOSTSRC/SPIRV-Tools/build" -j "$JOBS" install >"$HOSTSRC/spirv-tools-build.log" 2>&1 \
        || { grep -E "error|FAILED" "$HOSTSRC/spirv-tools-build.log" | head -20; die "SPIRV-Tools build failed — send me $HOSTSRC/spirv-tools-build.log"; }
fi
echo "  SPIRV-Tools: $(PKG_CONFIG_PATH=$HOSTLIB/pkgconfig pkg-config --modversion SPIRV-Tools 2>/dev/null || echo '?')"

# SPIRV-LLVM-Translator v18.1.9 against the static LLVM 18 (libLLVMSPIRVLib.a + LLVMSPIRVLib.pc). Its
# CMake would git-clone the SPIRV-Headers tag it pins (spirv-headers-tag.conf = 1c6bb274, the
# vulkan-sdk-1.3.275.0 tag) — that clone would hit the same git problem, so it is fetched here and
# handed over.
if [[ ! -f "$HOSTLIB/pkgconfig/LLVMSPIRVLib.pc" ]]; then
    echo "  building SPIRV-LLVM-Translator v18.1.9 (several minutes)"
    fetch_rev "$HOSTSRC/SPIRV-LLVM-Translator" https://github.com/KhronosGroup/SPIRV-LLVM-Translator.git v18.1.9
    fetch_rev "$HOSTSRC/spirv-headers-for-translator" https://github.com/KhronosGroup/SPIRV-Headers.git \
        1c6bb2743599e6eb6f37b2969acc0aef812e32e3
    cmake -S "$HOSTSRC/SPIRV-LLVM-Translator" -B "$HOSTSRC/SPIRV-LLVM-Translator/build" -G Ninja \
        -DCMAKE_BUILD_TYPE=Release -DCMAKE_INSTALL_PREFIX="$HOST" -DCMAKE_INSTALL_LIBDIR=lib \
        -DCMAKE_PREFIX_PATH="$HOST" -DCMAKE_C_COMPILER="$HOSTCC" -DCMAKE_CXX_COMPILER="$HOSTCXX" \
        -DLLVM_DIR="$LLVM_ROOT/lib/cmake/llvm" -DLLVM_SPIRV_BUILD_EXTERNAL=YES -DLLVM_SPIRV_INCLUDE_TESTS=OFF \
        -DLLVM_EXTERNAL_SPIRV_HEADERS_SOURCE_DIR="$HOSTSRC/spirv-headers-for-translator" \
        >"$HOSTSRC/translator-cmake.log" 2>&1 || { tail -20 "$HOSTSRC/translator-cmake.log"; die "SPIRV-LLVM-Translator cmake failed"; }
    ninja -C "$HOSTSRC/SPIRV-LLVM-Translator/build" -j "$JOBS" install >"$HOSTSRC/translator-build.log" 2>&1 \
        || { grep -E "error|FAILED" "$HOSTSRC/translator-build.log" | head -20; die "SPIRV-LLVM-Translator build failed — send me $HOSTSRC/translator-build.log"; }
fi
echo "  LLVMSPIRVLib: $(PKG_CONFIG_PATH=$HOSTLIB/pkgconfig pkg-config --modversion LLVMSPIRVLib 2>/dev/null || echo '?')"

# Mihawk's mesa_clc build directory, configured here with static LLVM. Same source export and option
# list as build-radv.sh, differing only in -Dshared-llvm=disabled. LDFLAGS=-Wl,--start-group lets the
# static clang/LLVM/LLVMSPIRVLib archives resolve each other whatever order meson emits them in (both
# ld and lld accept the group left open).
MESA_REV="${PS5_MESA_REVISION:-1ba9874f478657ccf6da72ce55f0260164cc0bd0}"
SRC_TREE="$BASE/PS5_Vulkan/.deps/work/radv-src"
CLC_BUILD="$BASE/PS5_Vulkan/.deps/work/radv-clc-build"
if [[ ! -f "$SRC_TREE/.revision" || "$(<"$SRC_TREE/.revision")" != "$MESA_REV" ]]; then
    echo "  exporting Mesa $MESA_REV into $SRC_TREE"
    staging="$SRC_TREE.new"; rm -rf "$staging"; mkdir -p "$staging" "$SRC_TREE"
    git -C "$BASE/PS5_Mesa" archive "$MESA_REV" | tar -x -C "$staging"
    rsync -rlp --checksum --delete --exclude=/.revision --exclude=/subprojects/packagecache/ \
        --exclude=/subprojects/zlib-*/ "$staging/" "$SRC_TREE/"
    rm -rf "$staging"; printf '%s\n' "$MESA_REV" > "$SRC_TREE/.revision"
fi
if [[ ! -f "$CLC_BUILD/build.ninja" ]]; then
    echo "  configuring mesa_clc with static LLVM 18"
    rm -rf "$CLC_BUILD"
    LDFLAGS="-Wl,--start-group -L$HOSTLIB" CC="$HOSTCC" CXX="$HOSTCXX" \
    meson setup "$CLC_BUILD" "$SRC_TREE" -Dbuildtype=release -Dmesa-clc=enabled -Dinstall-mesa-clc=true \
        -Dgallium-drivers= -Dvulkan-drivers= -Dplatforms= -Dglx=disabled -Degl=disabled -Dgbm=disabled \
        -Dopengl=false -Dgles1=disabled -Dgles2=disabled -Dllvm=enabled -Dshared-llvm=disabled \
        -Dbuild-tests=false -Dvalgrind=disabled -Dlibunwind=disabled -Dzstd=disabled -Dxmlconfig=disabled \
        -Dtools= -Dcpp_rtti=false > "$CLC_BUILD.setup.log" 2>&1 \
        || { tail -30 "$CLC_BUILD.setup.log"; die "mesa_clc meson setup failed — send me $CLC_BUILD.setup.log"; }
fi

# glslangValidator (>= 11.3): RADV compiles its internal BVH shaders with it at build time. Ubuntu
# packages it only as a system package, so build glslang 16.0.0 into $HOST — the GLSL->SPIR-V front end
# only (ENABLE_OPT=OFF: no SPIRV-Tools optimizer needed), static, no tests.
if [[ ! -x "$HOST/bin/glslangValidator" ]]; then
    echo "  building glslang 16.0.0 (glslangValidator)"
    fetch_rev "$HOSTSRC/glslang" https://github.com/KhronosGroup/glslang.git 16.0.0
    cmake -S "$HOSTSRC/glslang" -B "$HOSTSRC/glslang/build" -G Ninja \
        -DCMAKE_BUILD_TYPE=Release -DCMAKE_INSTALL_PREFIX="$HOST" -DCMAKE_INSTALL_LIBDIR=lib \
        -DCMAKE_C_COMPILER="$HOSTCC" -DCMAKE_CXX_COMPILER="$HOSTCXX" \
        -DENABLE_OPT=OFF -DBUILD_EXTERNAL=OFF -DGLSLANG_TESTS=OFF -DENABLE_GLSLANG_BINARIES=ON -DBUILD_SHARED_LIBS=OFF \
        >"$HOSTSRC/glslang-cmake.log" 2>&1 || { tail -20 "$HOSTSRC/glslang-cmake.log"; die "glslang cmake failed"; }
    ninja -C "$HOSTSRC/glslang/build" -j "$JOBS" install >"$HOSTSRC/glslang-build.log" 2>&1 \
        || { grep -E "error|FAILED" "$HOSTSRC/glslang-build.log" | head -20; die "glslang build failed — send me $HOSTSRC/glslang-build.log"; }
fi
export PATH="$HOST/bin:$PATH"
echo "  glslangValidator: $(glslangValidator --version 2>/dev/null | head -1)"

# A cross configure that failed part-way leaves a build dir with no build.ninja; start it clean.
REL_BUILD="$BASE/PS5_Vulkan/.deps/work/radv-build-ps5-release"
[[ -d "$REL_BUILD" && ! -f "$REL_BUILD/build.ninja" ]] && rm -rf "$REL_BUILD"

log "2b. build-radv.sh release (Mesa build; minutes, one-time subproject fetches are normal)"
bash tools/build-radv.sh release || die "build-radv.sh failed — send me the error lines it printed."

# ---- 3. report + next command --------------------------------------------------------------
ARCHIVE="$BASE/PS5_Vulkan/.deps/native/radv-release/lib/libvulkan_radeon.ps5.a"
[[ -f "$ARCHIVE" ]] || die "build reported success but $ARCHIVE is missing — send me the tail of build-radv's log."
log "DONE"
ls -la "$ARCHIVE"
sha256sum "$ARCHIVE" 2>/dev/null | cut -c1-16 | sed 's/^/  sha256: /'
cat > "$HOME/.ruffle-radv-env" <<ENV
# source this before ./build-title.sh
export PATH="\$HOME/.local/bin:$SHIM:$LLVM_BIN:\$PATH"
export LLVM_CONFIG=$LLVM_CONFIG
export PS5_PAYLOAD_SDK=$BASE/PS5_Vulkan/.deps/native/ps5-payload-sdk
export PS5_VULKAN_DIR=$BASE/PS5_Vulkan
ENV
cat <<NEXT

Wrote ~/.ruffle-radv-env. To build the Ruffle title (phase 3a, clears the screen on RADV):

  cd <your ruffle-ps5-title>
  source ~/.ruffle-radv-env
  ./build-title.sh

NEXT
