# Building the static RADV archive on the server (the one thing milestone 3 needs)

A PS5 title links RADV as a static archive built from Mesa-26.2-with-a-PS5-winsys, together with
the platform layer that supplies the libc functions RADV needs. The established, hardware-tested
combination is Mihawk's three repos; we use them as intended rather than hand-rolling a mix.

You run this on the Bytesized server (it's a Mesa build — minutes, not seconds, and it needs meson).
Nothing here is specific to Ruffle; it produces `libvulkan_radeon.ps5.a`, which many PS5 titles link.

## 0. Prerequisites (no root)

```sh
# meson + ninja via pip (the ninja wheel bundles the binary; no root needed)
python3 -m pip install --user --upgrade meson ninja
export PATH="$HOME/.local/bin:$PATH"       # add to ~/.bashrc so it persists
meson --version && ninja --version && rsync --version | head -1
```
You already have clang-18, git, wget, make from the milestone-1 work.

## 1. The three sibling repos (same parent directory)

```sh
mkdir -p ~/ps5gpu && cd ~/ps5gpu
git clone https://github.com/mihawk-99/PS5_Vulkan.git
git clone https://github.com/mihawk-99/PS5_Mesa.git
git clone https://github.com/mihawk-99/PS5_PayloadSDK.git
```
PS5_Vulkan's setup script pins the exact revisions of the other two itself (it uses `git archive`
from a pinned commit), so you don't check out anything by hand — just have them cloned as siblings.

## 2. Build the platform-layer SDK, then RADV (release)

```sh
cd ~/ps5gpu/PS5_Vulkan
tools/setup-native-dependencies.sh        # downloads upstream SDK v0.42, installs the platform layer over it
tools/build-radv.sh release               # builds libvulkan_radeon.ps5.a (the title archive)
```
`build-radv.sh release` prints where the archive landed (it's under
`.deps/native/radv-release/lib/libvulkan_radeon.ps5.a`). If meson fetches subproject wraps (zlib),
that's normal and only happens once.

## 3. Point the Ruffle title build at it

```sh
cd ~/ruffle-ps5-title                      # the crate from ruffle-ps5-title-v1.zip
export PS5_PAYLOAD_SDK=~/ps5gpu/PS5_Vulkan/.deps/native/ps5-payload-sdk   # the fork SDK with the platform layer
export PS5_VULKAN_DIR=~/ps5gpu/PS5_Vulkan
./build-title.sh                           # phase 3a: wgpu on RADV clears the screen
```

Note the SDK that builds the *title* is the **fork** SDK at
`~/ps5gpu/PS5_Vulkan/.deps/native/ps5-payload-sdk` (it has `libps5platform.a`), not the plain pacbrew
SDK — RADV needs the platform layer's `ps5_*` libc functions, which the link recipe binds.

## If a step fails

Send me the output of the failing step. The likely snags are known: meson not on PATH (step 0),
a missing build tool the SDK fork needs (it names it), or a Mesa subproject download (the server's
allowlist). None are dead ends — we worked through worse on the Rust side.
