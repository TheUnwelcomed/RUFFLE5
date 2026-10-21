#!/usr/bin/env bash
# PS5_RUFFLE_TITLE_BUILD_V1 — build the Ruffle PS5 title.
#
# This reuses two things that already work:
#   * ruffle-ps5-m1/build.sh + shim/ps5_shim.c (title mode): the Rust-for-PS5 toolchain and the C
#     shim for the libc functions the sandbox lacks or refuses (no raw syscalls, getdents reader,
#     SO_NBIO, etc.) — milestone 1, proven on hardware.
#   * ps5-rust-title/build-title.sh: the PIE->module converter and FSELF signer, the PRX stubs, the
#     NEEDED / import / raw-syscall gates, and the PPSA assembly. The Ruffle title is that pipeline
#     with two additions: it compiles ruffle-ps5 (this crate) instead of the hello lib, and it links
#     the static RADV archive.
#
# What must be in place:
#   PS5_PAYLOAD_SDK   a pacbrew SDK (for libc++/libunwind and the headers)      [required]
#   PS5_VULKAN_DIR    a PS5_Vulkan checkout that has built the RADV archive     [required]
#                     (tools/build-radv.sh release) and carries tools/radv-link.sh
#   RADV_ARCHIVE      override for the archive path (default: the release one in PS5_VULKAN_DIR)
#
# SERVER: the final link/convert/sign block is ps5-rust-title/build-title.sh's, with the RADV link
# inputs/flags from radv-link.sh spliced into APP_LINK_FLAGS. Keep that script as the source of truth
# and apply the three deltas marked below; they are small and mechanical.
set -euo pipefail
HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
SDK="${PS5_PAYLOAD_SDK:?set PS5_PAYLOAD_SDK to a pacbrew SDK}"
VK="${PS5_VULKAN_DIR:?set PS5_VULKAN_DIR to a PS5_Vulkan checkout with a built RADV archive}"
RADV_ARCHIVE="${RADV_ARCHIVE:-$VK/.deps/native/radv-release/lib/libvulkan_radeon.ps5.a}"
[[ -f "$RADV_ARCHIVE" ]] || { echo "no RADV archive at $RADV_ARCHIVE — run tools/build-radv.sh release in $VK" >&2; exit 1; }
[[ -f "$VK/tools/radv-link.sh" ]] || { echo "$VK has no tools/radv-link.sh" >&2; exit 1; }

echo "==> 1. cargo: build this crate to a staticlib (libruffle_ps5.a)"
# Same target and shim as milestone 1. The first build (3a) has no --features, so Ruffle is not
# compiled yet and only src/gpu.rs is exercised; add --features ruffle for 3b.
( cd "$HERE" && cargo build --release "${CARGO_FEATURES[@]:-}" )
STATICLIB="$HERE/target/x86_64-sie-ps5/release/libruffle_ps5.a"
echo "    $STATICLIB"

echo "==> 2. RADV link recipe from PS5_Vulkan"
# shellcheck source=/dev/null
source "$VK/tools/radv-link.sh"
radv_link_recipe "$VK" "$SDK" "$RADV_ARCHIVE"   # sets radv_linker_script, radv_link_inputs, radv_link_flags
echo "    archive: $RADV_ARCHIVE"
echo "    link flags: ${radv_link_flags[*]}"

echo "==> 3. final link + convert + sign: ps5-rust-title/build-title.sh pipeline, with:"
echo "    (a) OBJS/RUST_LIBS += $STATICLIB   (this crate, not the hello lib)"
echo "    (b) the shim compiled with -DPS5_TITLE (ruffle-ps5-m1/shim/ps5_shim.c) as before"
echo "    (c) APP_LINK_FLAGS += \${radv_link_flags[*]} and the RADV archive whole-archived,"
echo "        plus libc++/libunwind/builtins/libps5platform from radv_link_inputs"
echo "    param.json: applicationCategoryType 0 (Games row), flexibleMemorySize large (Ruffle+RADV)"
echo
echo "This skeleton stops here on purpose: the assembly is ps5-rust-title's proven script, and the"
echo "three deltas above are applied there once the cargo staticlib and the RADV archive both exist."
