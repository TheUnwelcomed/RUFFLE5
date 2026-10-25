#!/usr/bin/env bash
# ruffle5-status.sh — where the Ruffle5 build is: what runs now, and the build log's progress,
# with which build that log is from. apply-update.sh copies it to ~/ruffle5-status.sh.
#
#   ~/ruffle5-status.sh [title folder]     (default ~/ruffle-ps5/ruffle-ps5-title)
#
# 1.000.025: the log's first line names the version being built (build-title.sh V8). When no build
# is running and the sources are newer than the log, the log is the previous build's: this says
# so, with the command that starts the new one.
# 1.000.028: says so when more than one build runs at once, with the commands that stop them.
# SPDX-License-Identifier: GPL-3.0-or-later
LOG="$HOME/ruffle5-build.log"
TITLE="${1:-$HOME/ruffle-ps5/ruffle-ps5-title}"
version() { sed -n 's/^pub const APP_VERSION: &str = "\(.*\)";/\1/p' "$TITLE/src/lib.rs" 2>/dev/null; }

echo "== running now =="
running=$(ps -u "$(id -u)" -o pid=,etime=,pcpu=,rss=,args= 2>/dev/null \
    | grep -E 'build-title\.sh|cargo build|rustc --crate-name|ld\.lld|clang' \
    | grep -v -E 'grep|ruffle5-status' \
    | sed -E 's#/[^ ]*/bin/##; s/(--crate-name [^ ]+).*/\1/' | cut -c1-110)
if [[ -n "$running" ]]; then echo "$running"; else echo "(no build running)"; fi
# one build = one process group (its subshells share it); more than one group = builds at once
groups=$(ps -u "$(id -u)" -o pgid=,args= 2>/dev/null | awk '($2 ~ /(^|\/)(ba)?sh$/ && $3 ~ /build-title\.sh$/) || $2 ~ /build-title\.sh$/ {print $1}' | sort -u)
if (( $(printf '%s\n' "$groups" | grep -c .) > 1 )); then
    echo
    echo "WARNING: $(printf '%s\n' "$groups" | grep -c .) builds are running at once; they overwrite each other's files and log."
    echo "         Stop them all, then start one:"
    for g in $groups; do echo "           kill -- -$g"; done
fi

if [[ ! -f "$LOG" ]]; then
    echo "== no build log yet ($LOG) =="
    exit 0
fi
age=$(( ($(date +%s) - $(stat -c %Y "$LOG")) / 60 ))
built=$(sed -n 's/^==> Ruffle5 \([0-9.]*\) — build started.*/\1/p' "$LOG" | head -1)
echo "== progress ($LOG, last written $(date -r "$LOG" '+%H:%M'), $age min ago${built:+, Ruffle5 $built}) =="
grep -E '^(==>|   ✓|   Compiling (vm-rust|ruffle-ps5)|    Finished|wrote |error|warning: unused var)' "$LOG" | tail -n 14 | cut -c1-220

src=$(version)
if [[ -z "$running" ]]; then
    if [[ -n "$built" && -n "$src" && "$built" != "$src" ]] || [[ -z "$built" && "$TITLE/src/lib.rs" -nt "$LOG" ]]; then
        echo
        echo "note: this log is from the build before (${built:-an earlier version}); the sources are ${src:-newer} and have not been built."
        echo "      start it:  \"$TITLE/build-title.sh\" --ruffle --bg"
    elif grep -q '^==> done' "$LOG"; then
        echo
        echo "built: ${built:-$src} — copy build/app/PPSA99620/eboot.bin and sce_sys/ to the console"
    elif grep -q '^error' "$LOG"; then
        echo
        echo "the build stopped with an error (above); send me ~/ruffle5-build.log"
    fi
fi
