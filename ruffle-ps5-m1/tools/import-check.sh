#!/usr/bin/env bash
# PS5_RUST_IMPORT_CHECK_V1 — every undefined symbol the ELF imports must come from a library
# the console exports (the SDK's stub .so files are the list). Same idea as Jelly5's
# "Runtime import check": a symbol the loader cannot bind is a crash at start, not at link time.
# usage: tools/import-check.sh <elf>
set -euo pipefail
ELF="$1"
: "${PS5_PAYLOAD_SDK:?}"
NM="${PS5_PAYLOAD_SDK}/bin/llvm-nm"; command -v "$NM" >/dev/null || NM=llvm-nm
TMP="$(mktemp -d)"; trap 'rm -rf "$TMP"' EXIT
for so in "${PS5_PAYLOAD_SDK}"/target/lib/*.so; do "$NM" -D --defined-only "$so" 2>/dev/null | awk '{print $3}'; done | sort -u > "$TMP/exports"
"$NM" -D --undefined-only "$ELF" 2>/dev/null | awk '{print $2}' | sed 's/@.*//' | sort -u > "$TMP/imports"
n=$(wc -l < "$TMP/imports")
missing=$(comm -23 "$TMP/imports" "$TMP/exports")
if [[ -z "$missing" ]]; then
    echo "import check: all $n imports are exported by the SDK's libraries"
else
    echo "import check: $(echo "$missing" | wc -l) of $n imports are NOT in any SDK library (the loader will refuse or crash):"
    echo "$missing" | sed 's/^/   /'
    exit 1
fi
