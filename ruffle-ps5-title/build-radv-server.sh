#!/usr/bin/env bash
# build-radv-server.sh (v14) — build the static RADV archive on the Bytesized server (no root).
#
# It does what SETUP-RADV.md describes by hand would: make a host clang available, install
# meson+ninja for your user, clone Mihawk's three public repos as siblings, and run THEIR OWN
# setup + build scripts (which pin their own revisions). Output: libvulkan_radeon.ps5.a.
#
# Usage:   bash build-radv-server.sh [BASE_DIR]
#   BASE_DIR defaults to ~/ps5gpu. Safe to re-run: existing clones and tools are reused.
#   If clang isn't found, set it:  CLANG=/path/to/clang bash build-radv-server.sh
#   JOBS=n   parallel compile jobs for every build here (default 4 — this is a shared box).
#
# v14: the host LLVM 18 is no longer compiled here (v13 ran 16 parallel clang jobs; that took the
# shared server down). It is now LLVM's own prebuilt Ubuntu 22.04 packages (apt.llvm.org), unpacked
# with dpkg-deb, no root. Every build runs at most $JOBS jobs under nice/ionice and prints a progress
# line each minute, so a quiet terminal is never a stuck one.
set -euo pipefail

BASE="${1:-$HOME/ps5gpu}"
SHIM="$HOME/.ruffle-build-bin"
export PATH="$HOME/.local/bin:$SHIM:$PATH"

log() { printf '\n=== %s ===\n' "$*"; }
die() { printf '\nERROR: %s\n' "$*" >&2; exit 1; }

# ---- parallelism: this is a shared server --------------------------------------------------
# 16 parallel LLVM compiles (v13) exhausted it. Every ninja/make run here is capped at $JOBS jobs and
# runs at the lowest CPU/IO priority. JOBS=8 bash build-radv-server.sh raises it.
JOBS="${JOBS:-4}"
NICE=(nice -n 19); command -v ionice >/dev/null 2>&1 && NICE+=(ionice -c 2 -n 7)

# Long builds log to a file; a watcher prints one progress line a minute (ninja's [n/N] from the log,
# or from $PROGRESS_LOGS when the build writes its own logs) so the terminal shows the build is alive.
#   run_build LOG "what" cmd...     LOG="-" leaves the command's output on the terminal.
run_build() {
    local log="$1" what="$2" start=$SECONDS mon rc=0; shift 2
    ( while sleep 60; do
          kill -0 $$ 2>/dev/null || exit 0
          p=""
          for f in $(ls -tr "$log" ${PROGRESS_LOGS:-} 2>/dev/null || true); do   # oldest first: the newest log wins
              q="$(grep -o '^\[[0-9]*/[0-9]*\]' "$f" 2>/dev/null | tail -1 || true)"; [[ -n "$q" ]] && p="$q"
          done
          printf '    %s: %s %d min elapsed\n' "$what" "${p:-working}" $(( (SECONDS - start) / 60 ))
      done ) &
    mon=$!
    if [[ "$log" == "-" ]]; then "${NICE[@]}" "$@" || rc=$?
    else "${NICE[@]}" "$@" > "$log" 2>&1 || rc=$?
    fi
    kill "$mon" 2>/dev/null || true; wait "$mon" 2>/dev/null || true
    return $rc
}

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

# Mihawk's build scripts take the ninja to use from NINJA= (and make's job count from BUILD_JOBS=);
# hand them a ninja that carries the job cap and the low priority.
cat > "$SHIM/ninja-capped" <<EOF
#!/usr/bin/env bash
# ninja with the job cap and low priority this shared server needs (build-radv-server.sh)
exec ${NICE[*]} "$(command -v ninja)" -j $JOBS "\$@"
EOF
chmod +x "$SHIM/ninja-capped"
export NINJA="$SHIM/ninja-capped" BUILD_JOBS="$JOBS"

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
# The LLVM 18 tarball cannot back these: it is the ubuntu-18.04 build, compiled against GCC 7's
# libstdc++, whose std::optional is passed by reference, while code built on 22.04 (libstdc++ 11)
# passes it in registers — any std::optional crossing into those libraries is miscalled (verified:
# llvm::GlobalVariable's constructor reads a pointer where the caller put the value; that is the
# mesa_clc segfault in the translator). So the host tools get an LLVM 18 built FOR 22.04: LLVM's own
# packages for Ubuntu 22.04 (apt.llvm.org), unpacked here — compiled against this box's libstdc++.
# The tarball stays what it is good for: the PS5 target's clang/lld binaries.
log "2b0. host tools for mesa_clc (python modules, host LLVM 18 packages, SPIRV-Tools, translator; one-time)"
HOST="$BASE/host"; HOSTLIB="$HOST/lib"; HOSTSRC="$BASE/host-src"
mkdir -p "$HOSTLIB/pkgconfig" "$HOSTSRC"
LLVM_ROOT="$(dirname "$LLVM_BIN")"
ZLIB_ROOT="$BASE/PS5_Vulkan/.deps/native/zlib/root/usr"
[[ -f "$ZLIB_ROOT/lib/libz.a" ]] || die "expected setup's zlib at $ZLIB_ROOT/lib/libz.a — send me 2a's output."
echo "  parallel jobs: $JOBS (JOBS=n to change), low priority"

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

# A static LLVM (the HOSTLLVM_FROM_SOURCE route) links with -lz -ltinfo. On a server without the -dev
# packages those have no libz.so/libtinfo.so to resolve against; provide them in $HOSTLIB (zlib: the
# static one 2a built; tinfo: a link to the runtime library already installed). Harmless otherwise.
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

# Host LLVM 18 for the host tools: LLVM's own prebuilt packages for Ubuntu 22.04 (apt.llvm.org, the
# same llvm-toolchain-18 packaging Ubuntu ships) — built on 22.04 with its libstdc++, RTTI on,
# libLLVM-18.so + libclang-cpp.so shared: the configuration verified to work. They are unpacked with
# dpkg-deb (no root) under $HOSTLLVM_ROOT; Debian's llvm-config and LLVMConfig.cmake find everything
# relative to their own location, so the relocated prefix works as installed. Nothing is compiled.
# Runtime libraries the packages need that the server lacks (z3, libedit, ...) are fetched the same
# way from Ubuntu's archive. HOSTLLVM_FROM_SOURCE=1 is the old from-source build, now at 2 jobs.
HOSTLLVM_ROOT="$HOST/llvm18-deb"
SYSLIBS="$HOSTLLVM_ROOT/usr/lib/x86_64-linux-gnu"
APT_LLVM="https://apt.llvm.org/jammy"; APT_DIST="llvm-toolchain-jammy-18"
UBUNTU="https://archive.ubuntu.com/ubuntu"; UBUNTU_DIST="jammy"
DEBS="$HOSTSRC/debs"

pkg_lookup() {   # pkg_lookup INDEX PACKAGE [VERSION] -> "VERSION<TAB>FILENAME<TAB>SIZE" (newest, or VERSION)
    awk -v p="$2" -v want="${3:-}" '
        /^Package: /{cur=$2; ver=""; fn=""; sz=""}
        cur==p && /^Version: /{ver=$2}
        cur==p && /^Size: /{sz=$2}
        cur==p && /^Filename: /{fn=$2}
        cur==p && /^$/ && fn!="" {if (want=="" || ver==want) print ver "\t" fn "\t" sz; fn=""}
        END{if (cur==p && fn!="" && (want=="" || ver==want)) print ver "\t" fn "\t" sz}
    ' "$1" | sort -V | tail -1 || true
}
fetch_deb() {   # fetch_deb URLBASE FILENAME -> local path (downloads are kept in $DEBS and reused)
    local out="$DEBS/$(basename "$2" | sed 's/%3[aA]/_/g')"
    if [[ ! -s "$out" ]] || ! dpkg-deb --info "$out" >/dev/null 2>&1; then
        rm -f "$out"
        curl -fsSL -o "$out" "$1/$2" || die "download failed: $1/$2"
    fi
    printf '%s\n' "$out"
}
relocate_links() {   # the few absolute symlinks in Debian's layout -> the same path under the extracted root
    local l t
    while IFS= read -r l; do
        t="$(readlink "$l")"
        if [[ "$t" == /* && -e "$1$t" ]]; then ln -sfn "$1$t" "$l"; fi
    done < <(find "$1" -type l)
}
ensure_link() {   # ensure_link DIR NAME 'glob': DIR/NAME must exist; link it to the versioned library if not
    local real
    [[ -e "$1/$2" ]] && return 0
    real="$(ls "$1"/$3 2>/dev/null | head -1 || true)"
    [[ -n "$real" ]] || die "no $3 in $1 — send me: ls -la $1 | head -40"
    ln -sfn "$(basename "$real")" "$1/$2"; echo "    linked $2 -> $(basename "$real")"
}
missing_sonames() {   # shared libraries libLLVM / libclang-cpp need that nothing on this server provides
    LD_LIBRARY_PATH="$HOSTLLVM/lib:$SYSLIBS" ldd "$HOSTLLVM"/lib/libLLVM*.so.1* "$HOSTLLVM"/lib/libclang-cpp.so.1* 2>/dev/null \
        | awk '/not found/{print $1}' | sort -u || true
}

if [[ "${HOSTLLVM_FROM_SOURCE:-0}" == 1 ]]; then
    # The v13 route, kept for a server that cannot reach apt.llvm.org: LLVM 18.1.8 libraries from
    # source with the tarball's clang + lld. 2 jobs by default (LLVMJOBS=n): ~3-4 hours, but gentle.
    HOSTLLVM="$HOST/llvm18"
    LLVM_SRC_URL="https://github.com/llvm/llvm-project/releases/download/llvmorg-18.1.8/llvm-project-18.1.8.src.tar.xz"
    if [[ ! -x "$HOSTLLVM/bin/llvm-config" || ! -e "$HOSTLLVM/lib/libLLVM-18.so" || ! -e "$HOSTLLVM/lib/libclang-cpp.so" ]]; then
        LLVMJOBS="${LLVMJOBS:-2}"
        echo "  building host LLVM 18.1.8 libraries from source (~3250 compile steps, $LLVMJOBS at a time, hours; resumable)"
        LSRC="$HOSTSRC/llvm-project-18.1.8"
        if [[ ! -f "$LSRC/llvm/CMakeLists.txt" ]]; then
            rm -rf "$LSRC"; mkdir -p "$LSRC"
            curl -fsSL "$LLVM_SRC_URL" | tar -xJf - -C "$LSRC" --strip-components=1 \
                --wildcards '*/llvm/*' '*/clang/*' '*/cmake/*' '*/third-party/*' \
                || die "downloading/extracting the LLVM source failed"
        fi
        cmake -S "$LSRC/llvm" -B "$LSRC/build" -G Ninja -DCMAKE_BUILD_TYPE=Release -DCMAKE_INSTALL_PREFIX="$HOSTLLVM" \
            -DCMAKE_C_COMPILER="$LLVM_BIN/clang" -DCMAKE_CXX_COMPILER="$LLVM_BIN/clang++" -DLLVM_USE_LINKER=lld \
            -DLLVM_ENABLE_PROJECTS=clang -DLLVM_TARGETS_TO_BUILD=X86 \
            -DLLVM_LINK_LLVM_DYLIB=ON -DCLANG_LINK_CLANG_DYLIB=ON -DLLVM_ENABLE_RTTI=ON -DLLVM_ENABLE_ASSERTIONS=OFF \
            -DLLVM_ENABLE_ZLIB=OFF -DLLVM_ENABLE_ZSTD=OFF -DLLVM_ENABLE_LIBXML2=OFF -DLLVM_ENABLE_TERMINFO=OFF -DLLVM_ENABLE_LIBEDIT=OFF \
            -DLLVM_ENABLE_Z3_SOLVER=OFF -DLLVM_ENABLE_FFI=OFF -DLLVM_ENABLE_LIBPFM=OFF -DLLVM_ENABLE_BINDINGS=OFF -DLLVM_ENABLE_OCAMLDOC=OFF \
            -DLLVM_INCLUDE_TESTS=OFF -DLLVM_INCLUDE_EXAMPLES=OFF -DLLVM_INCLUDE_BENCHMARKS=OFF -DLLVM_INCLUDE_DOCS=OFF -DLLVM_INCLUDE_UTILS=OFF \
            -DCLANG_INCLUDE_TESTS=OFF -DCLANG_INCLUDE_DOCS=OFF -DCLANG_ENABLE_STATIC_ANALYZER=OFF -DCLANG_ENABLE_ARCMT=OFF \
            -DLLVM_BUILD_TOOLS=OFF -DCLANG_BUILD_TOOLS=OFF -DLLVM_PARALLEL_LINK_JOBS=1 \
            > "$HOSTSRC/hostllvm-cmake.log" 2>&1 || { tail -20 "$HOSTSRC/hostllvm-cmake.log"; die "host LLVM cmake failed"; }
        run_build "$HOSTSRC/hostllvm-build.log" "host LLVM" ninja -C "$LSRC/build" -j "$LLVMJOBS" llvm-config install \
            || { grep -E "error:|FAILED" "$HOSTSRC/hostllvm-build.log" | head -20; die "host LLVM build failed — send me the last 40 lines of $HOSTSRC/hostllvm-build.log"; }
        mkdir -p "$HOSTLLVM/bin" && cp "$LSRC/build/bin/llvm-config" "$HOSTLLVM/bin/llvm-config"
    fi
else
    HOSTLLVM="$HOSTLLVM_ROOT/usr/lib/llvm-18"
    if [[ ! -f "$HOSTLLVM_ROOT/.complete" || ! -x "$HOSTLLVM/bin/llvm-config" || ! -e "$HOSTLLVM/lib/libLLVM-18.so" \
          || ! -e "$HOSTLLVM/lib/libclang-cpp.so" || ! -f "$HOSTLLVM/lib/clang/18/include/opencl-c.h" ]]; then
        echo "  installing LLVM 18 for Ubuntu 22.04 from $APT_LLVM (prebuilt packages unpacked into $HOSTLLVM_ROOT; no build)"
        command -v dpkg-deb >/dev/null || die "dpkg-deb not found (it is part of dpkg, which every Ubuntu has)."
        rm -rf "$HOSTLLVM_ROOT"; mkdir -p "$HOSTLLVM_ROOT" "$DEBS"
        idx="$DEBS/Packages.$APT_DIST"
        curl -fsSL "$APT_LLVM/dists/$APT_DIST/main/binary-amd64/Packages" -o "$idx" \
            || { curl -fsSL "$APT_LLVM/dists/$APT_DIST/main/binary-amd64/Packages.gz" | gzip -dc > "$idx"; } \
            || { curl -fsSL "$APT_LLVM/dists/$APT_DIST/main/binary-amd64/Packages.xz" | xz -dc > "$idx"; } \
            || die "cannot fetch the package index $APT_LLVM/dists/$APT_DIST/main/binary-amd64/Packages — can this server reach apt.llvm.org? (Fallback, slow but gentle: HOSTLLVM_FROM_SOURCE=1 bash build-radv-server.sh)"
        ver="$(pkg_lookup "$idx" libllvm18 | cut -f1)"
        [[ -n "$ver" ]] || die "no libllvm18 in that index — send me: grep '^Package:' $idx | sort -u | head -80"
        echo "  version $ver"
        # The shared libraries, llvm-config, headers + cmake files, clang's resource headers (opencl-c.h).
        # The tool packages only serve LLVMExports.cmake, which refuses to load if a listed file is absent.
        for p in libllvm18 llvm-18 llvm-18-dev libclang-cpp18 libclang-cpp18-dev libclang-18-dev libclang-common-18-dev \
                 llvm-18-tools llvm-18-linker-tools llvm-18-runtime; do
            rec="$(pkg_lookup "$idx" "$p" "$ver")"; [[ -n "$rec" ]] || rec="$(pkg_lookup "$idx" "$p")"
            if [[ -z "$rec" ]]; then
                case "$p" in llvm-18-tools|llvm-18-linker-tools|llvm-18-runtime|libclang-cpp18-dev)
                    echo "    (no $p in the index — skipped)"; continue;; esac
                die "package $p is not in the apt.llvm.org index — send me: grep '^Package:' $idx | sort -u"
            fi
            fn="$(printf '%s' "$rec" | cut -f2)"; sz="$(printf '%s' "$rec" | cut -f3)"
            printf '    %-26s %4d MB\n' "$p" "$(( ${sz:-0} / 1048576 ))"
            deb="$(fetch_deb "$APT_LLVM" "$fn")"
            dpkg-deb -x "$deb" "$HOSTLLVM_ROOT" || die "dpkg-deb -x $deb failed"
        done
        relocate_links "$HOSTLLVM_ROOT"
        [[ -d "$HOSTLLVM/lib" ]] || die "no $HOSTLLVM/lib after unpacking — send me: find $HOSTLLVM_ROOT -maxdepth 4 -type d"
        ensure_link "$HOSTLLVM/lib" libLLVM-18.so 'libLLVM*.so.1*'
        ensure_link "$HOSTLLVM/lib" libclang-cpp.so 'libclang-cpp.so.1*'
        # runtime libraries of libLLVM that this server does not have: from Ubuntu's own packages
        for round in 1 2 3 4 5; do
            miss="$(missing_sonames)"; [[ -n "$miss" ]] || break
            for so in $miss; do
                case "$so" in
                    libz3.so.*) p=libz3-4;;      libedit.so.*) p=libedit2;;   libbsd.so.*) p=libbsd0;;
                    libmd.so.*) p=libmd0;;       libffi.so.*) p=libffi8;;     libxml2.so.*) p=libxml2;;
                    libicu*.so.*) p=libicu70;;   libtinfo.so.*) p=libtinfo6;; libzstd.so.*) p=libzstd1;;
                    liblzma.so.*) p=liblzma5;;   libz.so.*) p=zlib1g;;        libgomp.so.*) p=libgomp1;;
                    *) die "libLLVM needs $so, which this server lacks and I have no package name for — send me this line.";;
                esac
                echo "    $so is not on this server -> $p from Ubuntu $UBUNTU_DIST"
                rec=""
                for comp in main universe; do
                    uidx="$DEBS/Packages.$UBUNTU_DIST-$comp"
                    [[ -s "$uidx" ]] || { curl -fsSL "$UBUNTU/dists/$UBUNTU_DIST/$comp/binary-amd64/Packages.gz" | gzip -dc > "$uidx"; } \
                        || die "cannot fetch Ubuntu's $comp package index from $UBUNTU"
                    rec="$(pkg_lookup "$uidx" "$p")"; [[ -n "$rec" ]] && break
                done
                [[ -n "$rec" ]] || die "$p is not in Ubuntu $UBUNTU_DIST main/universe"
                deb="$(fetch_deb "$UBUNTU" "$(printf '%s' "$rec" | cut -f2)")"
                dpkg-deb -x "$deb" "$HOSTLLVM_ROOT" || die "dpkg-deb -x $deb failed"
            done
            relocate_links "$HOSTLLVM_ROOT"
        done
        miss="$(missing_sonames)"; [[ -z "$miss" ]] || die "libLLVM still lacks: $miss"
        # LLVMExports.cmake lists every file of a full install and refuses to load if one is missing.
        # The packages above should cover it; a tool nobody links may still be absent (an empty
        # placeholder satisfies the check), a library must not be (then the package set is wrong).
        n=0; badlibs=""
        while IFS= read -r f; do
            [[ -e "$f" ]] && continue
            case "$f" in
                "$HOSTLLVM"/bin/*) mkdir -p "$(dirname "$f")"; : > "$f"; n=$((n + 1));;
                *) badlibs="$badlibs $f";;
            esac
        done < <(grep -h '_cmake_import_check_files_for_' "$HOSTLLVM"/lib/cmake/llvm/LLVMExports*.cmake 2>/dev/null \
                 | grep -o '"\${_IMPORT_PREFIX}/[^"]*"' | tr -d '"' | sed "s|\${_IMPORT_PREFIX}|$HOSTLLVM|" | sort -u || true)
        (( n == 0 )) || echo "    ($n tools LLVMExports.cmake lists are not in these packages — empty placeholders made so cmake loads)"
        [[ -z "$badlibs" ]] || die "LLVMExports.cmake expects libraries these packages did not provide — send me this:$badlibs"
        for f in bin/llvm-config lib/libLLVM-18.so lib/libclang-cpp.so include/llvm/IR/Module.h include/clang/Basic/Version.h \
                 lib/cmake/llvm/LLVMConfig.cmake lib/clang/18/include/opencl-c.h; do
            [[ -e "$HOSTLLVM/$f" ]] || die "$HOSTLLVM/$f is missing after unpacking — send me: find $HOSTLLVM_ROOT -name '$(basename "$f")'"
        done
        export LD_LIBRARY_PATH="$HOSTLLVM/lib:$SYSLIBS${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
        got="$("$HOSTLLVM/bin/llvm-config" --prefix)"
        [[ "$(realpath "$got")" == "$(realpath "$HOSTLLVM")" ]] || die "the unpacked llvm-config reports prefix $got, expected $HOSTLLVM"
        # The check that matters: a std::optional crossing into libLLVM (GlobalVariable's constructor,
        # the mesa_clc crash site with the ubuntu-18.04 tarball) must arrive intact.
        cat > "$HOSTSRC/llvmcheck.cpp" <<'CPP'
#include "llvm/IR/LLVMContext.h"
#include "llvm/IR/Module.h"
#include "llvm/IR/GlobalVariable.h"
#include "llvm/IR/Constants.h"
#include "llvm/Support/raw_ostream.h"
#include <optional>
#include <string>
int main() {
  llvm::LLVMContext C; llvm::Module M("m", C);
  llvm::Type *T = llvm::Type::getInt32Ty(C);
  new llvm::GlobalVariable(M, T, false, llvm::GlobalValue::ExternalLinkage, llvm::ConstantInt::get(T, 7), "g",
                           nullptr, llvm::GlobalValue::NotThreadLocal, std::optional<unsigned>(3));
  std::string S; llvm::raw_string_ostream OS(S); M.print(OS, nullptr);
  bool ok = S.find("addrspace(3)") != std::string::npos;
  llvm::outs() << (ok ? "host LLVM check: ok" : "host LLVM check: BAD module text:\n" + S) << "\n";
  return ok ? 0 : 1;
}
CPP
        "$HOSTCXX" $("$HOSTLLVM/bin/llvm-config" --cxxflags) "$HOSTSRC/llvmcheck.cpp" -o "$HOSTSRC/llvmcheck" \
            $("$HOSTLLVM/bin/llvm-config" --ldflags --link-shared --libs core support) -Wl,-rpath-link,"$HOSTLLVM/lib:$SYSLIBS" \
            || die "a test program does not compile/link against the unpacked LLVM — send me the errors above"
        "$HOSTSRC/llvmcheck" || die "the unpacked LLVM fails the ABI check — send me the lines above"
        touch "$HOSTLLVM_ROOT/.complete"
        if [[ -d "$HOSTSRC/llvm-project-18.1.8" ]]; then
            echo "  removing the LLVM source tree + partial build of the earlier run ($(du -sh "$HOSTSRC/llvm-project-18.1.8" 2>/dev/null | cut -f1))"
            rm -rf "$HOSTSRC/llvm-project-18.1.8"
        fi
    fi
fi
echo "  host LLVM: $("$HOSTLLVM/bin/llvm-config" --version) at $HOSTLLVM ($("$HOSTLLVM/bin/llvm-config" --link-shared --libs core))"
# the host tools (mesa_clc, vtn_bindgen2, llvm-spirv) link these shared libraries; make them findable
# for every later step, including the cross build that runs mesa_clc
export LD_LIBRARY_PATH="$HOSTLLVM/lib:$SYSLIBS${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
RPATH_LINK="-Wl,-rpath-link,$HOSTLLVM/lib:$SYSLIBS"   # lets the linker resolve libLLVM's own dependencies
printf "[binaries]\nllvm-config = '%s'\n" "$HOSTLLVM/bin/llvm-config" > "$HOSTSRC/host-llvm.ini"

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
    run_build "$HOSTSRC/spirv-tools-build.log" "SPIRV-Tools" ninja -C "$HOSTSRC/SPIRV-Tools/build" -j "$JOBS" install \
        || { grep -E "error|FAILED" "$HOSTSRC/spirv-tools-build.log" | head -20; die "SPIRV-Tools build failed — send me $HOSTSRC/spirv-tools-build.log"; }
fi
echo "  SPIRV-Tools: $(PKG_CONFIG_PATH=$HOSTLIB/pkgconfig pkg-config --modversion SPIRV-Tools 2>/dev/null || echo '?')"

# SPIRV-LLVM-Translator v18.1.9 against the host LLVM (libLLVMSPIRVLib.a + LLVMSPIRVLib.pc). Its
# CMake would git-clone the SPIRV-Headers tag it pins (spirv-headers-tag.conf = 1c6bb274, the
# vulkan-sdk-1.3.275.0 tag) — that clone would hit the same git problem, so it is fetched here and
# handed over. A translator built against another LLVM (the tarball, in earlier runs) is rebuilt.
if [[ ! -f "$HOSTLIB/pkgconfig/LLVMSPIRVLib.pc" || "$(cat "$HOST/.translator-llvm" 2>/dev/null)" != "$HOSTLLVM" ]]; then
    echo "  building SPIRV-LLVM-Translator v18.1.9 (several minutes)"
    rm -rf "$HOSTSRC/SPIRV-LLVM-Translator/build" "$HOSTLIB/libLLVMSPIRVLib.a" "$HOSTLIB/pkgconfig/LLVMSPIRVLib.pc" "$HOST/include/LLVMSPIRVLib"
    fetch_rev "$HOSTSRC/SPIRV-LLVM-Translator" https://github.com/KhronosGroup/SPIRV-LLVM-Translator.git v18.1.9
    fetch_rev "$HOSTSRC/spirv-headers-for-translator" https://github.com/KhronosGroup/SPIRV-Headers.git \
        1c6bb2743599e6eb6f37b2969acc0aef812e32e3
    cmake -S "$HOSTSRC/SPIRV-LLVM-Translator" -B "$HOSTSRC/SPIRV-LLVM-Translator/build" -G Ninja \
        -DCMAKE_BUILD_TYPE=Release -DCMAKE_INSTALL_PREFIX="$HOST" -DCMAKE_INSTALL_LIBDIR=lib \
        -DCMAKE_PREFIX_PATH="$HOST" -DCMAKE_C_COMPILER="$HOSTCC" -DCMAKE_CXX_COMPILER="$HOSTCXX" \
        -DLLVM_DIR="$HOSTLLVM/lib/cmake/llvm" -DLLVM_SPIRV_BUILD_EXTERNAL=YES -DLLVM_SPIRV_INCLUDE_TESTS=OFF \
        -DLLVM_EXTERNAL_SPIRV_HEADERS_SOURCE_DIR="$HOSTSRC/spirv-headers-for-translator" \
        -DCMAKE_EXE_LINKER_FLAGS="$RPATH_LINK" -DCMAKE_SHARED_LINKER_FLAGS="$RPATH_LINK" \
        >"$HOSTSRC/translator-cmake.log" 2>&1 || { tail -20 "$HOSTSRC/translator-cmake.log"; die "SPIRV-LLVM-Translator cmake failed"; }
    run_build "$HOSTSRC/translator-build.log" "SPIRV-LLVM-Translator" ninja -C "$HOSTSRC/SPIRV-LLVM-Translator/build" -j "$JOBS" install \
        || { grep -E "error|FAILED" "$HOSTSRC/translator-build.log" | head -20; die "SPIRV-LLVM-Translator build failed — send me $HOSTSRC/translator-build.log"; }
    printf '%s\n' "$HOSTLLVM" > "$HOST/.translator-llvm"
fi
echo "  LLVMSPIRVLib: $(PKG_CONFIG_PATH=$HOSTLIB/pkgconfig pkg-config --modversion LLVMSPIRVLib 2>/dev/null || echo '?')"

# Mihawk's mesa_clc build directory, configured here so meson takes the host LLVM (a native file names
# its llvm-config; on PATH the tarball's would be found first). Same source export and option list as
# build-radv.sh. A directory configured against another LLVM (earlier runs) is redone.
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
if [[ ! -f "$CLC_BUILD/build.ninja" || "$(cat "$CLC_BUILD/.host-llvm" 2>/dev/null)" != "$HOSTLLVM" ]]; then
    echo "  configuring mesa_clc against the host LLVM 18"
    rm -rf "$CLC_BUILD"
    LDFLAGS="-L$HOSTLIB $RPATH_LINK" CC="$HOSTCC" CXX="$HOSTCXX" \
    meson setup "$CLC_BUILD" "$SRC_TREE" --native-file "$HOSTSRC/host-llvm.ini" \
        -Dbuildtype=release -Dmesa-clc=enabled -Dinstall-mesa-clc=true \
        -Dgallium-drivers= -Dvulkan-drivers= -Dplatforms= -Dglx=disabled -Degl=disabled -Dgbm=disabled \
        -Dopengl=false -Dgles1=disabled -Dgles2=disabled -Dllvm=enabled -Dshared-llvm=enabled \
        -Dbuild-tests=false -Dvalgrind=disabled -Dlibunwind=disabled -Dzstd=disabled -Dxmlconfig=disabled \
        -Dtools= > "$CLC_BUILD.setup.log" 2>&1 \
        || { tail -30 "$CLC_BUILD.setup.log"; die "mesa_clc meson setup failed — send me $CLC_BUILD.setup.log"; }
    printf '%s\n' "$HOSTLLVM" > "$CLC_BUILD/.host-llvm"
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
    run_build "$HOSTSRC/glslang-build.log" "glslang" ninja -C "$HOSTSRC/glslang/build" -j "$JOBS" install \
        || { grep -E "error|FAILED" "$HOSTSRC/glslang-build.log" | head -20; die "glslang build failed — send me $HOSTSRC/glslang-build.log"; }
fi
export PATH="$HOST/bin:$PATH"
echo "  glslangValidator: $(glslangValidator --version 2>/dev/null | head -1)"

# A cross configure that failed part-way leaves a build dir with no build.ninja; start it clean.
REL_BUILD="$BASE/PS5_Vulkan/.deps/work/radv-build-ps5-release"
[[ -d "$REL_BUILD" && ! -f "$REL_BUILD/build.ninja" ]] && rm -rf "$REL_BUILD"

# When a build step dies of a signal (ninja: "FAILED: [code=139]" = SIGSEGV), get a backtrace rather
# than guess: re-run that exact command under gdb, or, without gdb, under a preloaded signal handler
# whose addresses are resolved against the binary's symbol table.
crash_diagnose() {
    local blog="$1" cmd mesa_clc
    grep -q "^FAILED: \[code=1[0-9][0-9]\]" "$blog" || return 0
    set +e   # everything below is expected to fail; keep going and report
    printf '\n=== crash diagnosis ===\n'
    grep -m1 "^FAILED:" "$blog"
    cmd="$(grep -A1 -m1 "^FAILED:" "$blog" | tail -1)"
    printf 'command (run from %s):\n  %s\n' "$REL_BUILD" "$cmd"
    mesa_clc="$CLC_BUILD/src/compiler/clc/mesa_clc"
    [[ -x "$mesa_clc" ]] && { file "$mesa_clc" | cut -d, -f1-3; ldd "$mesa_clc" | grep -v "linux-vdso\|ld-linux" | sed 's/^/  /'; }
    printf 'kernel void k(global int *p){ p[0] = 1; }\n' > "$HOSTSRC/trivial.cl"
    ( cd "$HOSTSRC" && "$mesa_clc" -o trivial.spv trivial.cl ) 2>&1 | tail -5
    echo "trivial kernel through mesa_clc: rc=${PIPESTATUS[0]}"
    if command -v gdb >/dev/null; then
        echo "--- gdb backtrace ---"
        ( cd "$REL_BUILD" && eval "gdb -q -batch -ex run -ex bt -ex 'info registers rip' --args $cmd" ) 2>&1 | grep -v "^\[New Thread\|^\[Thread debugging\|^Using host libthread" | tail -45
    else
        echo "--- no gdb: backtrace from a preloaded signal handler ---"
        cat > "$HOSTSRC/segv.c" <<'C'
#define _GNU_SOURCE
#include <execinfo.h>
#include <signal.h>
#include <unistd.h>
#include <stdlib.h>
static void h(int s){ void *b[64]; int n = backtrace(b, 64);
    write(2, "\n=== signal backtrace ===\n", 26); backtrace_symbols_fd(b, n, 2); _exit(128 + s); }
__attribute__((constructor)) static void i(void){ signal(SIGSEGV, h); signal(SIGABRT, h); signal(SIGBUS, h); signal(SIGILL, h); }
C
        "$HOSTCC" -shared -fPIC -o "$HOSTSRC/segv.so" "$HOSTSRC/segv.c" || echo "(could not build the handler)"
        ( cd "$REL_BUILD" && eval "LD_PRELOAD=$HOSTSRC/segv.so $cmd" ) 2>"$HOSTSRC/crash.txt" || true
        grep -A40 "signal backtrace" "$HOSTSRC/crash.txt" | head -42
        echo "--- offsets resolved against mesa_clc's symbol table ---"
        nm -nC --defined-only "$mesa_clc" 2>/dev/null | grep -i " [tTwW] " > "$HOSTSRC/mesa_clc.syms"
        grep -o "mesa_clc(+0x[0-9a-f]*)\|mesa_clc() \[0x[0-9a-f]*\]" "$HOSTSRC/crash.txt" | grep -o "0x[0-9a-f]*" | head -30 \
            | python3 -c '
import sys, bisect
syms = []
for line in open(sys.argv[1]):
    p = line.split(None, 2)
    if len(p) == 3: syms.append((int(p[0], 16), p[2].strip()))
syms.sort(); keys = [a for a, _ in syms]
for tok in sys.stdin:
    off = int(tok, 16); i = bisect.bisect_right(keys, off) - 1
    print("  %s  %s" % (tok.strip(), syms[i][1] if i >= 0 else "?"))' "$HOSTSRC/mesa_clc.syms"
    fi
    echo "=== end crash diagnosis — send me everything from 'crash diagnosis' down ==="
    set -e
}

log "2b. build-radv.sh release (Mesa build at $JOBS jobs: mesa_clc first, then RADV; one-time subproject fetches are normal)"
if ! PROGRESS_LOGS="$CLC_BUILD.log $REL_BUILD.log" run_build - "Mesa" bash tools/build-radv.sh release; then
    crash_diagnose "$REL_BUILD.log"
    crash_diagnose "$CLC_BUILD.log"
    die "build-radv.sh failed — send me the error lines it printed."
fi

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
