#!/usr/bin/env bash
# Convert the included PNG master into macOS's ten standard icon representations.
set -euo pipefail
ROOT=$(cd "$(dirname "$0")/.." && pwd)
DEST=${1:?usage: build-icon.sh path/to/shimeji2.icns}
STAGE=$(mktemp -d)
trap 'rm -rf "$STAGE"' EXIT
ICONSET="$STAGE/shimeji2.iconset"
mkdir -p "$ICONSET" "$(dirname "$DEST")"
for size in 16 32 128 256 512; do
  sips -z "$size" "$size" "$ROOT/packaging/icons/shimeji2.png" \
    --out "$ICONSET/icon_${size}x${size}.png" >/dev/null
  double=$((size * 2))
  sips -z "$double" "$double" "$ROOT/packaging/icons/shimeji2.png" \
    --out "$ICONSET/icon_${size}x${size}@2x.png" >/dev/null
done
iconutil -c icns "$ICONSET" -o "$DEST"
