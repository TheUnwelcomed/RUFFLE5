#!/usr/bin/env bash
# online-check.sh — ask the Online tab's two sources, from this server, what the console will ask
# them, with the same code (net/: HTTPS through rustls with pure-Rust crypto): the Internet Archive's
# Flash collection (search, an item's files, a movie, a picture) and Flash Museum (its list, a search,
# an item's page and player page, a movie, a picture). A few minutes; nothing is installed.
#
#   bash tools/online-check.sh [OUT_DIR]        (default ~/ruffle5-online-check)
#
# Writes OUT_DIR/check.log, the answers as they came (JSON, HTML, pictures) and OUT_DIR.tar.gz to
# send back. The movies it downloads go to OUT_DIR/movies, are only measured, and are not in the
# .tar.gz.
#
# The check builds a copy of net/ outside the title folder: the title's .cargo/config.toml builds
# for the PS5, this runs here.
# SPDX-License-Identifier: GPL-3.0-or-later
set -euo pipefail
HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
OUT="${1:-$HOME/ruffle5-online-check}"
SRC="$HOME/.cache/ruffle5-online-check-src"
export PATH="$HOME/.cargo/bin:$PATH"
die() { echo "error: $*" >&2; exit 1; }
[[ -f "$HERE/net/Cargo.toml" ]] || die "$HERE/net is missing (apply the 1.000.027 update first)"
command -v cargo >/dev/null || die "cargo is not on PATH"

mkdir -p "$SRC"
# a fresh copy of the sources (the build cache in $SRC/target is kept between runs)
rm -rf "${SRC:?}/src" "${SRC:?}/examples"
cp -a "$HERE/net/." "$SRC/"
host="$(rustc -vV | sed -n 's/^host: //p')"
echo "==> building the check ($host; the first time fetches and compiles rustls and RustCrypto: ~2 min)"
( cd "$SRC" && cargo build --release --examples --target "$host" 2>&1 | grep -E '^(error|warning: unused)|Finished|Compiling (r5net|rustls)' ) || true
BIN="$SRC/target/$host/release/examples/online-check"
[[ -x "$BIN" ]] || die "the check did not build (see above); send me the output"

rm -rf "${OUT:?}"; mkdir -p "$OUT"
echo "==> asking archive.org and flashmuseum.net (answers into $OUT)"
"$BIN" "$OUT" || true
tar -C "$(dirname "$OUT")" --exclude="$(basename "$OUT")/movies" -czf "$OUT.tar.gz" "$(basename "$OUT")"
echo
grep -E '^(ok|FAIL) ' "$OUT/check.log" || true
echo
echo "==> done: send me $OUT.tar.gz ($(du -h "$OUT.tar.gz" | cut -f1))"
