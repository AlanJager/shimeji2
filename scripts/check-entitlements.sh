#!/usr/bin/env bash
# Read a signed app and fail on a weak entitlement, a missing hardened-runtime
# flag, or equal dev and release bundle ids.
#
# Forbidden keys (any com.apple.security.* key, including these):
#   com.apple.security.cs.disable-library-validation
#   com.apple.security.cs.allow-dyld-environment-variables
#   com.apple.security.cs.allow-unsigned-executable-memory
#   com.apple.security.get-task-allow
#   com.apple.security.app-sandbox

set -euo pipefail

ROOT=$(cd "$(dirname "$0")/.." && pwd)
APP=${1:-$ROOT/target/bundle/shimeji2.app}

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

CONFIG="$ROOT/packaging/bundle.toml"
RELEASE_ID=$(toml_get release_bundle_id "$CONFIG")
DEV_ID=$(toml_get dev_bundle_id "$CONFIG")
HOOK=$(toml_get hook "$CONFIG")

if [[ "$RELEASE_ID" == "$DEV_ID" ]]; then
  echo "dev and release bundle ids are equal: $RELEASE_ID" >&2
  exit 1
fi

if [[ ! -d "$APP" ]]; then
  echo "app not found: $APP" >&2
  exit 1
fi

check_runtime() {
  local target=$1
  local sig
  sig=$(codesign -dv "$target" 2>&1) || {
    echo "codesign -dv failed for $target" >&2
    printf '%s\n' "$sig" >&2
    exit 1
  }
  if ! awk '/flags=.*runtime/ { found = 1 } END { exit(found ? 0 : 1) }' <<< "$sig"; then
    echo "hardened runtime flag missing on $target" >&2
    printf '%s\n' "$sig" >&2
    exit 1
  fi
  printf '%s\n' "$sig"
}

check_ents() {
  local target=$1
  local ents
  # `:-` is the form the packaging check calls for (XML on stdout).
  ents=$(codesign -d --entitlements :- "$target" 2>&1) || {
    echo "codesign entitlements failed for $target" >&2
    printf '%s\n' "$ents" >&2
    exit 1
  }
  if awk 'index($0, "com.apple.security.") { found = 1 } END { exit(found ? 0 : 1) }' <<< "$ents"; then
    echo "forbidden entitlement on $target" >&2
    printf '%s\n' "$ents" >&2
    exit 1
  fi
}

APP_SIG=$(check_runtime "$APP")
IDENT=$(awk -F= '/^Identifier=/ { print $2; exit }' <<< "$APP_SIG")
if [[ "$IDENT" != "$RELEASE_ID" && "$IDENT" != "$DEV_ID" ]]; then
  echo "identifier $IDENT is neither $RELEASE_ID nor $DEV_ID" >&2
  exit 1
fi

PLIST_ID=$(plutil -extract CFBundleIdentifier raw -o - "$APP/Contents/Info.plist")
if [[ "$PLIST_ID" != "$IDENT" ]]; then
  echo "Info.plist id $PLIST_ID does not match signature $IDENT" >&2
  exit 1
fi

check_ents "$APP"
check_runtime "$APP/Contents/MacOS/$HOOK" >/dev/null
check_ents "$APP/Contents/MacOS/$HOOK"

echo "entitlements_ok identifier=$IDENT"
printf '%s\n' "$APP_SIG"
