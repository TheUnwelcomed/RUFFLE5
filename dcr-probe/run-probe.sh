#!/usr/bin/env bash
# run-probe.sh — Shockwave in Ruffle5, step 1: play a Director game in DirPlayer on this server
# (natively, no browser, no PS5) and keep what it shows, then pack two things to send back:
#   ~/dcr-probe-out.zip          the stage as PNGs every 2 s, the log (Lingo errors, panics)
#   ~/ruffle5-crates.tar.part*   the Rust crates DirPlayer and Ruffle5 build with (from cargo's cache),
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
# The .crate files cargo already downloaded for the probe and for Ruffle5 (both Cargo.lock files),
# straight from cargo's cache — `cargo vendor` cannot take Ruffle's tree, which has `swf 0.3.0`
# from crates.io and from Ruffle's own repository. Git dependencies are left out (fetched from
# GitHub at the other end), and so are the Windows import-library crates (tens of MB, never used).
if [[ "${SKIP_CRATES:-0}" == 1 ]]; then echo "==> crates skipped (SKIP_CRATES=1)"; exit 0; fi
echo "==> collecting the crates from cargo's cache"
V="$HOME/ruffle5-crates"
rm -rf "$V"; mkdir -p "$V/crates"
cp "$PROBE/Cargo.lock" "$V/dcr-probe.Cargo.lock"
[[ -f "$TITLE/Cargo.lock" ]] && cp "$TITLE/Cargo.lock" "$V/ruffle-ps5-title.Cargo.lock"
python3 - "$V" "${CARGO_HOME:-$HOME/.cargo}" <<'PY'
import glob, os, re, shutil, sys
v, cargo_home = sys.argv[1], sys.argv[2]
want, skipped = {}, []
for lock in glob.glob(os.path.join(v, "*.Cargo.lock")):
    for block in open(lock).read().split("[[package]]")[1:]:
        name = re.search(r'^name = "([^"]+)"', block, re.M)
        ver = re.search(r'^version = "([^"]+)"', block, re.M)
        src = re.search(r'^source = "([^"]+)"', block, re.M)
        if not (name and ver and src) or not src.group(1).startswith("registry+"):
            continue
        n, x = name.group(1), ver.group(1)
        if re.match(r"windows_(x86_64|i686|aarch64)_(gnu|gnullvm|msvc)$", n) or re.match(r"winapi-.*-pc-windows-gnu$", n):
            skipped.append(f"{n}-{x}")
            continue
        want[(n, x)] = True
caches = glob.glob(os.path.join(cargo_home, "registry", "cache", "*"))
missing, total = [], 0
for (n, x) in sorted(want):
    found = None
    for c in caches:
        p = os.path.join(c, f"{n}-{x}.crate")
        if os.path.isfile(p):
            found = p
            break
    if found:
        shutil.copy2(found, os.path.join(v, "crates"))
        total += os.path.getsize(found)
    else:
        missing.append(f"{n}-{x}")
open(os.path.join(v, "skipped.txt"), "w").write("\n".join(skipped) + "\n")
open(os.path.join(v, "missing.txt"), "w").write("\n".join(missing) + "\n")
print(f"   {len(want) - len(missing)} crates, {total / 1e6:.0f} MB; {len(missing)} not in the cache; {len(skipped)} Windows library crates left out")
PY
(cd "$HOME" && rm -f ruffle5-crates.tar ruffle5-crates.tar.part* && tar -cf ruffle5-crates.tar ruffle5-crates \
    && split -b 20m -d ruffle5-crates.tar ruffle5-crates.tar.part && rm -f ruffle5-crates.tar)
echo "   -> $(ls "$HOME"/ruffle5-crates.tar.part* | wc -l) parts of at most 20 MB:"
ls -l "$HOME"/ruffle5-crates.tar.part* | awk '{printf "      %s  %s\n", $5, $9}'
echo
echo "==> send me: ~/dcr-probe-out.zip and every ~/ruffle5-crates.tar.partNN file"
