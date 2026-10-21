#!/usr/bin/env bash
# Convenience orchestration for SETUP-RADV.md. Runs the repos' OWN setup scripts (which pin their
# own revisions); this only clones the siblings and sequences the builds. Run it on the server.
set -euo pipefail
BASE="${1:-$HOME/ps5gpu}"
echo "==> base: $BASE"
mkdir -p "$BASE" && cd "$BASE"
command -v meson >/dev/null || { echo "meson not found; run: python3 -m pip install --user meson ninja; export PATH=\$HOME/.local/bin:\$PATH"; exit 1; }
command -v ninja >/dev/null || { echo "ninja not found; run: python3 -m pip install --user meson ninja"; exit 1; }
command -v rsync >/dev/null || { echo "rsync not found (ask the host, or build it --user)"; exit 1; }
for repo in PS5_Vulkan PS5_Mesa PS5_PayloadSDK; do
    [[ -d $repo ]] || git clone "https://github.com/mihawk-99/$repo.git"
done
cd "$BASE/PS5_Vulkan"
echo "==> setup-native-dependencies.sh (platform-layer SDK)"
tools/setup-native-dependencies.sh
echo "==> build-radv.sh release"
tools/build-radv.sh release
echo "==> done. Archive under $BASE/PS5_Vulkan/.deps/native/radv-release/lib/"
ls -la "$BASE/PS5_Vulkan/.deps/native/radv-release/lib/" 2>/dev/null || true
echo
echo "Next: cd your ruffle-ps5-title, then"
echo "  export PS5_PAYLOAD_SDK=$BASE/PS5_Vulkan/.deps/native/ps5-payload-sdk"
echo "  export PS5_VULKAN_DIR=$BASE/PS5_Vulkan"
echo "  ./build-title.sh"
