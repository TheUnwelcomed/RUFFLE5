#!/usr/bin/env bash
# build-radv-server.sh (v2) — build the static RADV archive on the Bytesized server (no root).
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
    if [[ -d "$BASE/$repo/.git" ]]; then
        echo "  $repo already cloned — leaving it as is"
    else
        # blobless: full commit/tree history (the setup scripts git-archive a pinned commit) but
        # blobs fetched on demand, so the big Mesa fork comes down fast instead of in full.
        git clone --filter=blob:none "https://github.com/mihawk-99/$repo.git" "$BASE/$repo" \
            || die "git clone $repo failed (network/allowlist?). Clone it by hand into $BASE and re-run."
    fi
done

# ---- 2. platform-layer SDK, then RADV ------------------------------------------------------
cd "$BASE/PS5_Vulkan"
log "2a. setup-native-dependencies.sh (downloads upstream SDK v0.42, installs the platform layer over it)"
tools/setup-native-dependencies.sh || die "setup-native-dependencies.sh failed — send me its output."

log "2b. build-radv.sh release (Mesa build; minutes, one-time subproject fetches are normal)"
tools/build-radv.sh release || die "build-radv.sh failed — send me the error lines it printed."

# ---- 3. report + next command --------------------------------------------------------------
ARCHIVE="$BASE/PS5_Vulkan/.deps/native/radv-release/lib/libvulkan_radeon.ps5.a"
[[ -f "$ARCHIVE" ]] || die "build reported success but $ARCHIVE is missing — send me the tail of build-radv's log."
log "DONE"
ls -la "$ARCHIVE"
sha256sum "$ARCHIVE" 2>/dev/null | cut -c1-16 | sed 's/^/  sha256: /'
cat > "$HOME/.ruffle-radv-env" <<ENV
# source this before ./build-title.sh
export PATH="\$HOME/.local/bin:$SHIM:\$PATH"
export PS5_PAYLOAD_SDK=$BASE/PS5_Vulkan/.deps/native/ps5-payload-sdk
export PS5_VULKAN_DIR=$BASE/PS5_Vulkan
ENV
cat <<NEXT

Wrote ~/.ruffle-radv-env. To build the Ruffle title (phase 3a, clears the screen on RADV):

  cd <your ruffle-ps5-title>
  source ~/.ruffle-radv-env
  ./build-title.sh

NEXT
