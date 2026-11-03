#!/bin/bash
# ZB-1 House Bass Machine — build + install for macOS.
# Builds the VST3 on this Mac and drops it into your VST3 folder for FL Studio.
set -e
cd "$(dirname "$0")"
exec > >(tee install.log) 2>&1

echo ""
echo "  ZB-1  //  HOUSE BASS MACHINE  //  ZVLE"
echo "  ---------------------------------------"
echo ""

# 1. Apple's command line tools (compiler + linker)
if ! xcode-select -p >/dev/null 2>&1; then
  echo "* Installing Apple command line tools. A popup will appear — click Install."
  echo "* When it finishes, run this script again."
  xcode-select --install || true
  exit 0
fi

# 2. Rust toolchain
if [ -f "$HOME/.cargo/env" ]; then source "$HOME/.cargo/env"; fi
if ! command -v cargo >/dev/null 2>&1; then
  echo "* Installing Rust (one time, about a minute)..."
  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal
  source "$HOME/.cargo/env"
fi

# 3. Build. Universal (Apple Silicon + Intel) when rustup is available.
echo "* Building ZB-1 (first build takes a few minutes)..."
if command -v rustup >/dev/null 2>&1; then
  rustup target add aarch64-apple-darwin x86_64-apple-darwin >/dev/null 2>&1 || true
  cargo xtask bundle-universal zb1 --release || cargo xtask bundle zb1 --release
else
  cargo xtask bundle zb1 --release
fi

# 4. Install into the standard VST3 folder and ad-hoc sign so macOS will load it.
DEST="$HOME/Library/Audio/Plug-Ins/VST3"
mkdir -p "$DEST"
rm -rf "$DEST/ZB-1.vst3"
cp -R "target/bundled/ZB-1.vst3" "$DEST/"
xattr -dr com.apple.quarantine "$DEST/ZB-1.vst3" 2>/dev/null || true
codesign --force --deep --sign - "$DEST/ZB-1.vst3" >/dev/null 2>&1 || true

echo ""
echo "* Installed: $DEST/ZB-1.vst3"
echo ""
echo "  Next, in FL Studio:"
echo "    Options > Manage plugins > Find installed plugins"
echo "    then add ZB-1 from the Generators list (vendor: ZVLE)."
echo ""
echo "ZB1_INSTALL_DONE"
