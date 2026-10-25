#!/usr/bin/env bash
# fetch-dirplayer.sh — DirPlayer (github.com/igorlira/dirplayer-rs, GPL-3.0), the Shockwave / Director
# player Ruffle5 builds on for .dcr / .dir games, at the commit Ruffle5 pins, into vendor/dirplayer,
# with tools/dirplayer-native.patch applied (what DirPlayer needs to run outside a browser).
# Safe to re-run: an existing checkout at the right commit with the patch in place is kept.
# SPDX-License-Identifier: GPL-3.0-or-later
set -euo pipefail
HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
DP_URL=https://github.com/igorlira/dirplayer-rs.git
# The cham/w3d-fullscreen branch head (2026-09-09): main as of 68376fb plus the Shockwave 3D work
# (W3D scenes, cameras, lightmaps, motions) that Ruffle5's software 3D renderer draws.
DP_REV=b9b9466628721d810a0bd5fde8e3228ab04b73ed
DEST="${1:-$HERE/vendor/dirplayer}"
PATCH="$HERE/tools/dirplayer-native.patch"
MARK=PS5_RUFFLE_DIRPLAYER_NATIVE
die() { echo "error: $*" >&2; exit 1; }

if [[ ! -f "$DEST/.fetched-rev" || "$(<"$DEST/.fetched-rev")" != "$DP_REV" ]]; then
    echo "==> DirPlayer ${DP_REV:0:7} into $DEST"
    rm -rf "$DEST"; mkdir -p "$DEST"
    git -C "$DEST" init -q
    git -C "$DEST" remote add origin "$DP_URL"
    git -C "$DEST" fetch -q --no-tags --depth 1 origin "$DP_REV" || die "git fetch $DP_URL $DP_REV failed"
    git -C "$DEST" -c advice.detachedHead=false checkout -q FETCH_HEAD || die "checkout failed"
    printf '%s\n' "$DP_REV" > "$DEST/.fetched-rev"
fi
# The patch is re-applied whenever it changes: start from the pinned files each time.
want="$(sha256sum "$PATCH" | cut -d' ' -f1)"
if [[ "$(cat "$DEST/.patch-sha" 2>/dev/null)" != "$want" ]]; then
    git -C "$DEST" checkout -q -- . && git -C "$DEST" clean -qfd -e .fetched-rev -e .patch-sha
    git -C "$DEST" apply --whitespace=nowarn --check "$PATCH" || die "tools/dirplayer-native.patch does not apply to DirPlayer ${DP_REV:0:7}"
    git -C "$DEST" apply --whitespace=nowarn "$PATCH"
    printf '%s\n' "$want" > "$DEST/.patch-sha"
fi
grep -rq "$MARK" "$DEST/vm-rust/src" || die "the DirPlayer patch is not in place"
echo "   ✓ DirPlayer ${DP_REV:0:7} + dirplayer-native.patch"
