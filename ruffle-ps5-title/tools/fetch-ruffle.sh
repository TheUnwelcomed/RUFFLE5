#!/usr/bin/env bash
# fetch-ruffle.sh — Ruffle (github.com/ruffle-rs/ruffle, MIT or Apache-2.0) at the revision Ruffle5
# pins (Cargo.toml), into vendor/ruffle, with tools/ruffle-dirplayer.patch applied: what a Director
# (Shockwave) movie asks of its Flash cast members — their variables, functions and timelines —
# in core/src/dirplayer.rs (1.000.036, PS5_RUFFLE_DIRPLAYER_FLASH_V1). Cargo.toml's
# [patch."https://github.com/ruffle-rs/ruffle"] builds Ruffle from this tree.
#
# Only the crates Ruffle5 builds are kept (core, render, swf, flv, video, wstr, tools/asc: about
# 15 MB): taken from cargo's own checkout of that revision when it has one (every build before
# 1.000.036 made it), else fetched (that commit only, and only those folders where the server's git
# can; the whole commit otherwise).
# Safe to re-run: an existing tree at the right revision with the patch in place is kept.
# SPDX-License-Identifier: GPL-3.0-or-later
set -euo pipefail
HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
URL=https://github.com/ruffle-rs/ruffle.git
REV=be342ba07b48f3045a16659bdda5bd654ba064fe
DEST="${1:-$HERE/vendor/ruffle}"
PATCH="$HERE/tools/ruffle-dirplayer.patch"
MARK=PS5_RUFFLE_DIRPLAYER_FLASH_V1
PARTS=(Cargo.toml LICENSE.md core render swf flv video wstr tools/asc)
die() { echo "error: $*" >&2; exit 1; }

grep -q "rev = \"$REV\"" "$HERE/Cargo.toml" || die "Cargo.toml does not pin Ruffle $REV (update REV here with it)"
want="$(sha256sum "$PATCH" | cut -d' ' -f1)"
if [[ -f "$DEST/.fetched-rev" && "$(<"$DEST/.fetched-rev")" == "$REV" && "$(cat "$DEST/.patch-sha" 2>/dev/null)" == "$want" ]] \
    && grep -rq "$MARK" "$DEST/core/src/dirplayer.rs" 2>/dev/null; then
    echo "   ✓ Ruffle ${REV:0:7} + ruffle-dirplayer.patch (vendor/ruffle)"
    exit 0
fi

if [[ ! -f "$DEST/.fetched-rev" || "$(<"$DEST/.fetched-rev")" != "$REV" ]]; then
    tmp="$DEST.new"
    rm -rf "$tmp"; mkdir -p "$tmp"
    # 1. cargo's checkout of the revision (named by its first 7 characters)
    src=""
    for c in "${CARGO_HOME:-$HOME/.cargo}"/git/checkouts/ruffle-*/"${REV:0:7}"*; do
        if [[ -f "$c/core/Cargo.toml" ]] && [[ "$(git -C "$c" rev-parse HEAD 2>/dev/null)" == "$REV" ]]; then src="$c"; break; fi
    done
    if [[ -n "$src" ]]; then
        echo "==> Ruffle ${REV:0:7} into $DEST (from cargo's checkout, $src)"
        for p in "${PARTS[@]}"; do
            mkdir -p "$tmp/$(dirname "$p")"
            cp -a "$src/$p" "$tmp/$p" || die "copying $p from $src failed"
        done
    else
        # 2. fetched: that commit, its blobs only for those folders when the git can
        echo "==> Ruffle ${REV:0:7} into $DEST (fetched from $URL)"
        git -C "$tmp" init -q
        git -C "$tmp" remote add origin "$URL"
        if git -C "$tmp" fetch -q --no-tags --depth 1 --filter=blob:none origin "$REV" 2>/dev/null \
            && git -C "$tmp" sparse-checkout set --no-cone "${PARTS[@]/#//}" 2>/dev/null \
            && git -C "$tmp" -c advice.detachedHead=false checkout -q FETCH_HEAD 2>/dev/null; then
            :
        else
            echo "   (this git cannot fetch part of a commit: fetching all of it, about 300 MB)"
            rm -rf "$tmp"; mkdir -p "$tmp"
            git -C "$tmp" init -q
            git -C "$tmp" remote add origin "$URL"
            git -C "$tmp" fetch -q --no-tags --depth 1 origin "$REV" || die "git fetch $URL $REV failed"
            git -C "$tmp" -c advice.detachedHead=false checkout -q FETCH_HEAD || die "checkout failed"
        fi
        rm -rf "$tmp/.git"
        for p in "${PARTS[@]}"; do [[ -e "$tmp/$p" ]] || die "the fetched Ruffle has no $p"; done
        # what Ruffle5 does not build goes (the frontends, the tests: most of the commit)
        find "$tmp" -mindepth 1 -maxdepth 1 | while read -r e; do
            n="$(basename "$e")"; keep=0
            for p in "${PARTS[@]}"; do [[ "${p%%/*}" == "$n" ]] && keep=1; done
            [[ $keep == 1 ]] || rm -rf "$e"
        done
        [[ -d "$tmp/tools" ]] && find "$tmp/tools" -mindepth 1 -maxdepth 1 ! -name asc -exec rm -rf {} +
    fi
    # a git tree of its own, so the patch is applied (and re-applied when it changes) with git
    git -C "$tmp" init -q
    git -C "$tmp" add -A
    git -C "$tmp" -c user.name=ruffle5 -c user.email=ruffle5@localhost commit -qm "Ruffle ${REV:0:7}" || die "git commit failed"
    printf '%s\n' "$REV" > "$tmp/.fetched-rev"
    rm -rf "$DEST"
    mv "$tmp" "$DEST"
fi

# The patch is re-applied whenever it changes: start from the pinned files each time.
git -C "$DEST" checkout -q -- . && git -C "$DEST" clean -qfd -e .fetched-rev -e .patch-sha
git -C "$DEST" apply --whitespace=nowarn --check "$PATCH" || die "tools/ruffle-dirplayer.patch does not apply to Ruffle ${REV:0:7}"
git -C "$DEST" apply --whitespace=nowarn "$PATCH"
printf '%s\n' "$want" > "$DEST/.patch-sha"
grep -q "$MARK" "$DEST/core/src/dirplayer.rs" || die "the Ruffle patch is not in place"
echo "   ✓ Ruffle ${REV:0:7} + ruffle-dirplayer.patch (vendor/ruffle)"
