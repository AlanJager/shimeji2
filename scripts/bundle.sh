#!/usr/bin/env bash
# Build target/bundle/shimeji2.app, a UDZO DMG, and a zip.
# Signs ad-hoc (--sign -) unless --sign-identity is set. No notarization.

set -euo pipefail

ROOT=$(cd "$(dirname "$0")/.." && pwd)
cd "$ROOT"

toml_get() {
  local key=$1
  local file=$2
  local value
  value=$(awk -v key="$key" '
    $0 ~ "^" key "[[:space:]]*=" {
      sub(/^[^=]*=[[:space:]]*"/, "")
      sub(/".*$/, "")
      print
      exit
    }
  ' "$file")
  if [[ -z "$value" ]]; then
    echo "missing ${key} in ${file}" >&2
    exit 1
  fi
  printf '%s' "$value"
}

workspace_version() {
  local value
  value=$(awk '
    /^\[workspace\.package\]$/ { in_block = 1; next }
    /^\[/ { in_block = 0 }
    in_block && /^version[[:space:]]*=/ {
      sub(/^version[[:space:]]*=[[:space:]]*/, "")
      gsub(/"/, "")
      gsub(/[[:space:]]/, "")
      print
      exit
    }
  ' "$ROOT/Cargo.toml")
  if [[ -z "$value" ]]; then
    echo "Cargo.toml has no [workspace.package] version" >&2
    exit 1
  fi
  printf '%s' "$value"
}

SIGN_ID="-"
DEV=0
OFFLINE=0
while [[ $# -gt 0 ]]; do
  case "$1" in
    --dev)
      DEV=1
      shift
      ;;
    --sign-identity)
      SIGN_ID=${2:?missing identity after --sign-identity}
      shift 2
      ;;
    --offline)
      OFFLINE=1
      shift
      ;;
    *)
      echo "unknown argument: $1" >&2
      exit 2
      ;;
  esac
done

CONFIG="$ROOT/packaging/bundle.toml"
RELEASE_ID=$(toml_get release_bundle_id "$CONFIG")
DEV_ID=$(toml_get dev_bundle_id "$CONFIG")
EXECUTABLE=$(toml_get executable "$CONFIG")
HOOK=$(toml_get hook "$CONFIG")
PACK=$(toml_get pack "$CONFIG")
if [[ "$RELEASE_ID" == "$DEV_ID" ]]; then
  echo "dev and release bundle ids are equal: $RELEASE_ID" >&2
  exit 1
fi
if [[ "$DEV" -eq 1 ]]; then
  BUNDLE_ID=$DEV_ID
else
  BUNDLE_ID=$RELEASE_ID
fi
VERSION=$(workspace_version)

# An empty repository disables the menu item, rather than opening a dummy URL.
REPOSITORY=$(tr -d '[:space:]' < "$ROOT/packaging/repository.txt")
REPOSITORY=${REPOSITORY%/}
REPOSITORY=${REPOSITORY%.git}
export SHIMEJI_RELEASES_URL=""
if [[ -n "$REPOSITORY" ]]; then
  if [[ ! "$REPOSITORY" =~ ^https://github\.com/[A-Za-z0-9_-]+/[A-Za-z0-9_.-]+$ ]]; then
    echo "packaging/repository.txt must contain a GitHub repository HTTPS URL" >&2
    exit 1
  fi
  export SHIMEJI_RELEASES_URL="$REPOSITORY/releases"
fi
BUILD_ARGS=(--locked --release)
if [[ "$OFFLINE" -eq 1 ]]; then BUILD_ARGS+=(--offline); fi

case $(uname -m) in
  arm64) HOST_TARGET=aarch64-apple-darwin ;;
  x86_64) HOST_TARGET=x86_64-apple-darwin ;;
  *)
    echo "unsupported arch: $(uname -m)" >&2
    exit 1
    ;;
esac

INSTALLED=$(rustup target list --installed)
has_target() {
  printf '%s\n' "$INSTALLED" | awk -v t="$1" '$0 == t { found = 1 } END { exit(found ? 0 : 1) }'
}

UNIVERSAL=0
if has_target aarch64-apple-darwin && has_target x86_64-apple-darwin; then
  UNIVERSAL=1
fi

export MACOSX_DEPLOYMENT_TARGET="${MACOSX_DEPLOYMENT_TARGET:-14.0}"
# Strip this build host's home/workspace paths from compiler-generated diagnostics.
export RUSTFLAGS="${RUSTFLAGS:-} --remap-path-prefix=$ROOT=shimeji2 --remap-path-prefix=${HOME:?}=build-host"
TARGET_DIR=${CARGO_TARGET_DIR:-$ROOT/target}

build_target() {
  echo "building $1"
  nice -n 10 cargo build -j 4 "${BUILD_ARGS[@]}" --target "$1" -p shimeji-macos -p shimeji-hook
}

if [[ "$UNIVERSAL" -eq 1 ]]; then
  build_target aarch64-apple-darwin
  build_target x86_64-apple-darwin
elif has_target "$HOST_TARGET"; then
  build_target "$HOST_TARGET"
else
  echo "building host (no --target; $HOST_TARGET is not installed)"
  nice -n 10 cargo build -j 4 "${BUILD_ARGS[@]}" -p shimeji-macos -p shimeji-hook
fi

place_bin() {
  local name=$1
  local dest="$APP/Contents/MacOS/$name"
  if [[ "$UNIVERSAL" -eq 1 ]]; then
    lipo -create \
      "$TARGET_DIR/aarch64-apple-darwin/release/$name" \
      "$TARGET_DIR/x86_64-apple-darwin/release/$name" \
      -output "$dest"
  elif has_target "$HOST_TARGET"; then
    cp -p "$TARGET_DIR/$HOST_TARGET/release/$name" "$dest"
  else
    cp -p "$TARGET_DIR/release/$name" "$dest"
  fi
  chmod 755 "$dest"
}

APP="$ROOT/target/bundle/shimeji2.app"
mkdir -p "$ROOT/target/bundle"
rm -rf "$APP" "$ROOT/target/bundle/shimeji2.dmg" "$ROOT/target/bundle/shimeji2.zip"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"

place_bin "$EXECUTABLE"
place_bin "$HOOK"

sed \
  -e "s/@BUNDLE_ID@/${BUNDLE_ID}/g" \
  -e "s/@VERSION@/${VERSION}/g" \
  "$ROOT/packaging/Info.plist.in" > "$APP/Contents/Info.plist"
plutil -lint "$APP/Contents/Info.plist" >/dev/null

"$ROOT/scripts/build-icon.sh" "$APP/Contents/Resources/shimeji2.icns"
python3 - "$ROOT" <<'PY'
import hashlib, json, sys
from pathlib import Path
root = Path(sys.argv[1])
notice = json.loads((root / 'packaging/THIRD-PARTY-NOTICES.json').read_text())
assert notice['cargo_lock_sha256'] == hashlib.sha256((root / 'Cargo.lock').read_bytes()).hexdigest(), 'Refresh third-party notices after changing Cargo.lock'
PY
cp -p "$ROOT/LICENSE" "$APP/Contents/Resources/LICENSE"
cp -p "$ROOT/NOTICE.md" "$APP/Contents/Resources/NOTICE.md"
cp -p "$ROOT/packaging/THIRD-PARTY-NOTICES.md" "$APP/Contents/Resources/THIRD-PARTY-NOTICES.md"
cp -p "$ROOT/packaging/READ-ME-FIRST.md" "$APP/Contents/Resources/READ-ME-FIRST.md"

PACK_DST="$APP/Contents/Resources/packs/shime"
mkdir -p "$(dirname "$PACK_DST")"
ditto "$ROOT/$PACK" "$PACK_DST"
(
  cd "$PACK_DST"
  shasum -a 256 -c "$ROOT/$PACK/CHECKSUMS.sha256"
)
diff -rq "$ROOT/$PACK" "$PACK_DST"

sign() {
  local target=$1
  if [[ "$SIGN_ID" == "-" ]]; then
    codesign --force --options runtime \
      --entitlements "$ROOT/packaging/entitlements.plist" \
      --sign - \
      "$target"
  else
    codesign --force --options runtime --timestamp \
      --entitlements "$ROOT/packaging/entitlements.plist" \
      --sign "$SIGN_ID" \
      "$target"
  fi
}

# Nested Mach-O first, then the bundle seal.
sign "$APP/Contents/MacOS/$HOOK"
sign "$APP"

"$ROOT/scripts/check-entitlements.sh" "$APP"

STAGE=$(mktemp -d)
ditto "$APP" "$STAGE/shimeji2.app"
cp -p "$ROOT/packaging/READ-ME-FIRST.md" "$STAGE/READ-ME-FIRST.md"
ln -s /Applications "$STAGE/Applications"
hdiutil create \
  -volname shimeji2 \
  -srcfolder "$STAGE" \
  -format UDZO \
  -ov \
  "$ROOT/target/bundle/shimeji2.dmg" >/dev/null
# ZIP users also receive the first-open instructions beside the app.
rm "$STAGE/Applications"
ditto -c -k "$STAGE" "$ROOT/target/bundle/shimeji2.zip"
rm -r "$STAGE"

echo "bundle_id=$BUNDLE_ID"
echo "version=$VERSION"
if [[ "$UNIVERSAL" -eq 1 ]]; then
  echo "arch=universal"
else
  echo "arch=$HOST_TARGET"
fi
echo "app=$APP"
echo "dmg=$ROOT/target/bundle/shimeji2.dmg"
echo "zip=$ROOT/target/bundle/shimeji2.zip"
echo "releases_url=${SHIMEJI_RELEASES_URL:-not_configured}"
