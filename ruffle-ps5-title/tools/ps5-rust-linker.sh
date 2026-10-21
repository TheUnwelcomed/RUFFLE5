#!/usr/bin/env bash
# PS5_RUST_LINKER_V1 — the linker cargo calls for the x86_64-sie-ps5 target.
# SPDX-License-Identifier: GPL-3.0-or-later
#
# rustc drives the link through a C compiler driver with -nodefaultlibs, which makes the SDK's
# prospero-clang wrapper drop its own -lc/-lkernel_web/-lSceLibcInternal/-lSceNet. This script
# puts them back, adds the kit's lib/ (the shim, and the archives std expects by FreeBSD names:
# libgcc_s = the SDK's libunwind, libexecinfo/librt/libutil stubs), and hands everything to
# prospero-clang, which adds crt1.o, the SDK sysroot and the PS5 linker script (-pie,
# max-page-size 0x4000, emulated TLS) and runs lld.
set -euo pipefail
HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
KIT="$(cd "${HERE}/.." && pwd)"
: "${PS5_PAYLOAD_SDK:?PS5_PAYLOAD_SDK is not set — run the link through build.sh}"
[[ -f "${KIT}/lib/libps5shim.a" ]] || { echo "lib/libps5shim.a missing — build.sh builds it first" >&2; exit 1; }
exec "${PS5_PAYLOAD_SDK}/bin/prospero-clang" "$@" \
     -L"${KIT}/lib" -lps5shim \
     -Wl,--start-group -lc -lkernel_web -lSceLibcInternal -lSceNet -Wl,--end-group
