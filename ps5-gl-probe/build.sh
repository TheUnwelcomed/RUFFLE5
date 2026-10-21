#!/usr/bin/env bash
# PS5_GL_PROBE_BUILD_V1 — build ps5-gl-probe.elf against a pacbrew SDK (v0.41.1 or later: it must
# have Mesa with zink/RADV and the SDL2 that loads it; the plain ps5-payload-sdk has neither).
#
#   PS5_PAYLOAD_SDK=~/pacbrew/opt/ps5-payload-sdk ./build.sh
#
# Get the SDK: https://github.com/ps5-payload-dev/pacbrew-repo/releases/download/v0.41.1/ps5-payload-dev.tar.gz
# (217 MB; sha256 of the tarball this was built with: e811bc2e4676c5bb…). Extract anywhere:
#   mkdir -p ~/pacbrew && tar -xzf ps5-payload-dev.tar.gz -C ~/pacbrew
set -euo pipefail
HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
SDK="${PS5_PAYLOAD_SDK:-}"
for c in "$SDK" "$HOME/pacbrew/opt/ps5-payload-sdk" /opt/ps5-payload-sdk; do
    [[ -n "$c" && -x "$c/bin/prospero-clang" && -f "$c/target/user/homebrew/lib/libSDL2.a" ]] && SDK="$c" && break
done
[[ -n "$SDK" && -f "$SDK/target/user/homebrew/lib/libSDL2.a" ]] || { echo "error: set PS5_PAYLOAD_SDK to a pacbrew SDK (needs target/user/homebrew/lib/libSDL2.a)" >&2; exit 1; }
[[ -f "$SDK/target/user/homebrew/lib/libEGL.so.1" ]] || { echo "error: $SDK has no Mesa (libEGL.so.1): pacbrew v0.41.1 or later is needed" >&2; exit 1; }
export PS5_PAYLOAD_SDK="$SDK"
PKG="$SDK/bin/prospero-pkg-config"
CFLAGS="$("$PKG" --cflags sdl2)"
LIBS="$("$PKG" --libs sdl2)"
echo "==> SDK $SDK (SDL $("$PKG" --modversion sdl2))"
"$SDK/bin/prospero-clang" -std=gnu11 -O2 -Wall -Wextra $CFLAGS -o "$HERE/ps5-gl-probe.elf" "$HERE/main.c" $LIBS -lSceAgc -lm
"$SDK/bin/prospero-strip" --strip-debug "$HERE/ps5-gl-probe.elf" 2>/dev/null || true
ls -la "$HERE/ps5-gl-probe.elf"
echo "==> NEEDED: $(readelf -d "$HERE/ps5-gl-probe.elf" | grep NEEDED | sed 's/.*\[\(.*\)\]/\1/' | tr '\n' ' ')"
echo "==> run: put libEGL.so.1 libGLESv2.so.2 libGL.so.1 libgallium-26.2.0.so libvulkan.so.1 (from the SDK's"
echo "    target/user/homebrew/lib, or ruffle-ps5-mesa-v0.41.1.zip) in /data/ruffle/lib on the console, then send"
echo "    ps5-gl-probe.elf to elfldr (port 9021) or launch it from websrv. Log: /data/ruffle/gl-probe.log"
