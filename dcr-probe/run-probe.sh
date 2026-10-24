#!/usr/bin/env bash
# run-probe.sh — Shockwave in Ruffle5, step 1: play a Director game in DirPlayer on this server
# (natively, no browser, no PS5) and keep what it shows, then pack two things to send back:
#   ~/dcr-probe-out.zip          the stage as PNGs every 2 s, the log (Lingo errors, panics)
#   ~/ruffle5-crates.tar.xz.*    the Rust crates DirPlayer and Ruffle5 build with (cargo vendor),
#                                so the next steps can be compiled and tested before they reach you
# No root. Uses the toolchain you build Ruffle5 with; JOBS=n as build-title.sh (default 4).
#
#   bash ~/ruffle-ps5/dcr-probe/run-probe.sh [game folder] [seconds] [script]
#   game folder: has gamewrapper.dir (or a .dcr / .dir); default ~/ttbattleblitz
#   script: timed inputs, e.g. "20 click 320 240; 25 key Enter" (see src/main.rs)
# SPDX-License-Identifier: GPL-3.0-or-later
set -euo pipefail
PROBE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO="$(cd "$PROBE/.." && pwd)"
TITLE="$REPO/ruffle-ps5-title"
GAME="${1:-$HOME/ttbattleblitz}"
SECONDS_TO_RUN="${2:-150}"
SCRIPT="${3:-30 click 320 240; 40 key Enter; 50 key Space; 60 click 320 300; 70 key Enter; 80 down Right; 82 up Right; 84 key a; 86 key s}"
OUT="$HOME/dcr-probe-out"
JOBS="${JOBS:-4}"
NICE=(nice -n 19); command -v ionice >/dev/null 2>&1 && NICE+=(ionice -c 2 -n 7)
die() { echo "error: $*" >&2; exit 1; }

[[ -d "$GAME" ]] || die "no game folder $GAME (unzip ttbattleblitz.zip into your home folder, or name the folder)"
MOVIE="$GAME/gamewrapper.dir"
[[ -f "$MOVIE" ]] || MOVIE="$(ls "$GAME"/*.dcr "$GAME"/*.dir 2>/dev/null | head -1 || true)"
[[ -n "$MOVIE" && -f "$MOVIE" ]] || die "no .dir or .dcr in $GAME"

# --- 1. DirPlayer at the pinned commit, patched ----------------------------------------------------
bash "$TITLE/tools/fetch-dirplayer.sh"

# --- 2. the probe (for this machine, not the PS5) ------------------------------------------------
# rustup's cargo lives in ~/.cargo/bin, which a non-login shell may not have on PATH.
for d in "${CARGO_HOME:-$HOME/.cargo}/bin" "$HOME/.cargo/bin"; do
    [[ -x "$d/cargo" ]] && export PATH="$d:$PATH"
done
if ! command -v cargo >/dev/null; then
    tc="$(ls -d "$HOME"/.rustup/toolchains/nightly-*/bin 2>/dev/null | head -1 || true)"
    [[ -n "$tc" && -x "$tc/cargo" ]] && export PATH="$tc:$PATH"
fi
command -v cargo >/dev/null || die "cargo not found (looked in ~/.cargo/bin and ~/.rustup/toolchains): send me the output of  ls ~/.cargo/bin ~/.rustup/toolchains"
CARGO=(cargo)
if command -v rustup >/dev/null && rustup run nightly cargo --version >/dev/null 2>&1; then CARGO=(cargo +nightly); fi
cd "$PROBE"
[[ -f Cargo.lock ]] || cp "$TITLE/vendor/dirplayer/vm-rust/Cargo.lock" Cargo.lock   # DirPlayer's own versions
echo "==> building the probe ($("${CARGO[@]}" --version)), $JOBS jobs — a few minutes the first time"
"${NICE[@]}" "${CARGO[@]}" build --release -j "$JOBS" 2>&1 | tee "$PROBE/build.log" | grep -E '^(error|warning: unused)|Compiling vm-rust|Finished' || true
[[ -x target/release/dcr-probe ]] || { tail -40 "$PROBE/build.log"; die "the probe did not build: send me $PROBE/build.log"; }

# --- 3. play the game ---------------------------------------------------------------------------------
rm -rf "$OUT"; mkdir -p "$OUT"
echo "==> playing $(basename "$MOVIE") for $SECONDS_TO_RUN s (pictures in $OUT)"
set +e
"$PROBE/target/release/dcr-probe" "$MOVIE" "$OUT" "$SECONDS_TO_RUN" "$SCRIPT" > >(tee "$OUT/log.txt") 2> >(tee "$OUT/errors.txt" >&2)
rc=$?
set -e
sleep 1
echo "==> probe exit code $rc; $(ls "$OUT"/*.png 2>/dev/null | wc -l) pictures; $(grep -c '^\[ERROR\]' "$OUT/errors.txt" || true) Lingo/player errors"
[[ -f "$OUT/panic.txt" ]] && { echo "   panic:"; sed 's/^/     /' "$OUT/panic.txt"; }
(cd "$HOME" && rm -f dcr-probe-out.zip && zip -qr dcr-probe-out.zip dcr-probe-out)
echo "   -> $HOME/dcr-probe-out.zip ($(du -h "$HOME/dcr-probe-out.zip" | cut -f1))"

# --- 4. the crates, for building and testing the next steps here --------------------------------
echo "==> cargo vendor (DirPlayer's and Ruffle5's crates)"
V="$HOME/ruffle5-crates"
rm -rf "$V"
SYNC=()
[[ -f "$TITLE/Cargo.lock" ]] && SYNC=(--sync "$TITLE/Cargo.toml")
"${NICE[@]}" "${CARGO[@]}" vendor --versioned-dirs "${SYNC[@]}" "$V" > "$HOME/ruffle5-crates.config.toml" 2> "$PROBE/vendor.log" \
    || { tail -20 "$PROBE/vendor.log"; die "cargo vendor failed: send me $PROBE/vendor.log"; }
cp "$PROBE/Cargo.lock" "$V/dcr-probe.Cargo.lock"
[[ -f "$TITLE/Cargo.lock" ]] && cp "$TITLE/Cargo.lock" "$V/ruffle-ps5-title.Cargo.lock"
# The Windows import libraries (lib/*.a, tens of MB) are never used on Linux or the PS5: drop them
# and their checksum lines, so the upload is smaller.
python3 - "$V" <<'PY'
import json, os, shutil, sys
v = sys.argv[1]
for d in sorted(os.listdir(v)):
    lib = os.path.join(v, d, "lib")
    if not (d.startswith("windows_") or d.startswith("winapi-")) or not os.path.isdir(lib):
        continue
    shutil.rmtree(lib)
    ck = os.path.join(v, d, ".cargo-checksum.json")
    data = json.load(open(ck))
    data["files"] = {k: s for k, s in data["files"].items() if not k.startswith("lib/")}
    json.dump(data, open(ck, "w"))
PY
(cd "$HOME" && rm -f ruffle5-crates.tar.xz ruffle5-crates.tar.xz.part* \
    && XZ_OPT=-6 tar -cJf ruffle5-crates.tar.xz ruffle5-crates ruffle5-crates.config.toml \
    && split -b 20m -d ruffle5-crates.tar.xz ruffle5-crates.tar.xz.part)
echo "   -> $(ls "$HOME"/ruffle5-crates.tar.xz.part* | wc -l) parts of at most 20 MB:"
ls -l "$HOME"/ruffle5-crates.tar.xz.part* | awk '{printf "      %s  %s\n", $5, $9}'
echo
echo "==> send me: ~/dcr-probe-out.zip and every ~/ruffle5-crates.tar.xz.partNN file"
