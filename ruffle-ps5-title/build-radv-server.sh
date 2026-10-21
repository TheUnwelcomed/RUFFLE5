#!/usr/bin/env bash
# build-radv-server.sh — build the static RADV archive on the Bytesized server (no root).
#
# It only does what SETUP-RADV.md describes, by hand would: install meson+ninja for your user,
# clone Mihawk's three public repos as siblings, and run THEIR OWN setup + build scripts (which pin
# their own revisions). Output: libvulkan_radeon.ps5.a, which the Ruffle title links.
#
# Usage:   bash build-radv-server.sh [BASE_DIR]
#   BASE_DIR defaults to ~/ps5gpu. Safe to re-run: existing clones are reused.
set -euo pipefail

BASE="${1:-$HOME/ps5gpu}"
export PATH="$HOME/.local/bin:$PATH"

log() { printf '\n=== %s ===\n' "$*"; }
die() { printf '\nERROR: %s\n' "$*" >&2; exit 1; }

# ---- 0. tools -------------------------------------------------------------------------------
log "0. checking tools"
for c in git clang-18 make wget rsync python3; do
    command -v "$c" >/dev/null || command -v "${c%-18}" >/dev/null \
        || die "missing '$c'. git/make/wget/python3 should be on the server; clang-18 you used for milestone 1. rsync: ask the host if absent."
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
        # full clone (not shallow): the setup scripts git-archive from pinned commits that a
        # shallow clone might not contain.
        git clone "https://github.com/mihawk-99/$repo.git" "$BASE/$repo" \
            || die "git clone $repo failed (network/allowlist?). Clone it by hand and re-run."
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
export PS5_PAYLOAD_SDK=$BASE/PS5_Vulkan/.deps/native/ps5-payload-sdk
export PS5_VULKAN_DIR=$BASE/PS5_Vulkan
ENV
cat <<NEXT

Wrote ~/.ruffle-radv-env. To build the Ruffle title (phase 3a, clears the screen on RADV):

  cd <your ruffle-ps5-title>
  source ~/.ruffle-radv-env
  ./build-title.sh

NEXT
