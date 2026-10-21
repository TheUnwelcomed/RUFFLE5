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

# ---- 0. a host C/C++ compiler --------------------------------------------------------------
# PS5_Vulkan's dep setup insists on `clang` by name (for host-side zlib / Mesa tools). The server
# has no bare clang on PATH, but your payload SDK ships a real clang 18. Find one and expose it as
# `clang`/`clang++` in a shim dir on PATH, so the sub-scripts' `command -v clang` succeeds.
log "0. finding a host clang"
find_clang() {
    # explicit override
    if [[ -n "${CLANG:-}" && -x "${CLANG}" ]]; then echo "${CLANG}"; return; fi
    # on PATH, any reasonable version
    local c
    for c in clang clang-18 clang-17 clang-19 clang-16 clang-15 clang-14; do
        command -v "$c" >/dev/null && { command -v "$c"; return; }
    done
    # inside a payload SDK you already have (pacbrew or plain); bin/clang is a full host clang
    for c in "${PS5_PAYLOAD_SDK:-}/bin/clang" "${PS5_PACBREW_SDK:-}/bin/clang" \
             "$HOME"/ps5-payload-sdk/bin/clang "$HOME"/pacbrew/opt/ps5-payload-sdk/bin/clang; do
        [[ -n "$c" && -x "$c" ]] && { echo "$c"; return; }
    done
    # last resort: search your home for an SDK's clang (bounded)
    find "$HOME" -maxdepth 5 -type f -path '*/bin/clang' -perm -u+x 2>/dev/null | head -1
}
HOSTCLANG="$(find_clang || true)"
[[ -n "$HOSTCLANG" && -x "$HOSTCLANG" ]] || die "no clang found. Point me at the one from milestone 1, e.g.:
    CLANG=\$HOME/ps5-payload-sdk/bin/clang bash build-radv-server.sh
(it's the 'clang 18.1.8' your earlier builds used; look under your payload SDK's bin/)."
HOSTCXX="${HOSTCLANG}++"; [[ -x "$HOSTCXX" ]] || HOSTCXX="$HOSTCLANG"
mkdir -p "$SHIM"
ln -sf "$HOSTCLANG" "$SHIM/clang"
ln -sf "$HOSTCXX"  "$SHIM/clang++"
hash -r
printf 'using clang: %s\n  %s\n' "$HOSTCLANG" "$("$HOSTCLANG" --version | head -1)"
export CC="$SHIM/clang" CXX="$SHIM/clang++"

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
        git clone "https://github.com/mihawk-99/$repo.git" "$BASE/$repo" \
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
