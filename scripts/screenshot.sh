#!/usr/bin/env bash
# One PNG of the demo data with animations jumped to their end.
#
#   scripts/screenshot.sh [out.png]     default screenshots/messages.png
#
# macOS renders the frame offscreen with Metal (the `screenshot` feature);
# Linux runs the release build inside scripts/linux-headless.sh's own sway.
set -euo pipefail

root=$(cd "$(dirname "$0")/.." && pwd)
out=${1:-$root/screenshots/messages.png}
mkdir -p "$(dirname "$out")"

if [ "$(uname)" = Darwin ]; then
  MESSAGES_DEMO=1 GPUIX_BACKGROUND=1 MESSAGES_SCREENSHOT=$out \
    cargo run --manifest-path "$root/Cargo.toml" -p messages --release --features screenshot
else
  cargo build --manifest-path "$root/Cargo.toml" -p messages --release
  MESSAGES_DEMO=1 MESSAGES_STILL=1 "$root/scripts/linux-headless.sh" "$root/target/release/messages" "$out" 3
fi
