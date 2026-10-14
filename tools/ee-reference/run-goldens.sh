#!/bin/bash
# Run each scenario twice and keep the trace only when the two files match byte for byte.
# ONLY=name1,name2 limits the run. The default is every shime scenario plus every ee-synthetic scenario.
set -euo pipefail

cd "$(dirname "$0")"


PIN="$(tr -d '[:space:]' < PIN)"
if [[ ! -f vendor/.pinned ]] || [[ "$(tr -d '[:space:]' < vendor/.pinned)" != "$PIN" ]]; then
  ./fetch.sh
fi

./prepare-rundir.sh
gradle -q printClasspath --console=plain
CP="$(cat build/runtime-classpath.txt)"

ROOT="$(cd ../.. && pwd)"
SHIME_OUT="$ROOT/tests/golden/shime"
EE_OUT="$ROOT/tests/golden/ee-synthetic"
mkdir -p "$SHIME_OUT" "$EE_OUT"
SHIME_WORKDIR="$(pwd)/run/shime"

shime_names=(
  shime-floor-only
  shime-moving-window
  shime-two-screens
  shime-drag-throw
  shime-breed-limit
  shime-offscreen-teleport
  shime-window-drag-up
)

ee_names=(
  ee-layout-root
  ee-layout-dropped
  ee-layout-set
  ee-error-condition
  ee-error-action
  ee-error-nonboolean
)

want() {
  local name="$1"
  if [[ -z "${ONLY:-}" ]]; then
    return 0
  fi
  local item
  IFS=',' read -r -a items <<< "${ONLY}"
  for item in "${items[@]}"; do
    if [[ "$item" == "$name" ]]; then
      return 0
    fi
  done
  return 1
}

run_pair() {
  local name="$1"
  local workdir="$2"
  local scenario="$3"
  local dest="$4"
  local first second
  first="$(mktemp)"
  second="$(mktemp)"
  echo "running $name"
  (
    cd "$workdir"
    java -Dshimeji2.ref.factory=headless -Dshimeji2.ref.pin="$PIN" -Dfile.encoding=UTF-8 \
      -cp "$CP" shimeji2.ref.TraceRunner --scenario "$scenario" --out "$first"
    java -Dshimeji2.ref.factory=headless -Dshimeji2.ref.pin="$PIN" -Dfile.encoding=UTF-8 \
      -cp "$CP" shimeji2.ref.TraceRunner --scenario "$scenario" --out "$second"
  )
  if ! cmp -s "$first" "$second"; then
    echo "NOT IDENTICAL: $name" >&2
    cmp "$first" "$second" || true
    rm -f "$first" "$second"
    exit 1
  fi
  cp "$first" "$dest"
  rm -f "$first" "$second"
  echo "IDENTICAL $name -> ${dest#"$ROOT"/}"
}

for name in "${shime_names[@]}"; do
  want "$name" || continue
  run_pair "$name" "$SHIME_WORKDIR" "$ROOT/tests/scenarios/$name.json" "$SHIME_OUT/$name.trace"
done

for name in "${ee_names[@]}"; do
  want "$name" || continue
  scenario="$ROOT/tests/scenarios/$name.json"
  pack="$ROOT/tests/fixtures/ee-synthetic/$name"
  image_set="$(python3 -c 'import json,sys; print(json.load(open(sys.argv[1]))["imageSet"])' "$scenario")"
  workdir="$(pwd)/run/ee/$name"
  rm -rf "$workdir"
  python3 ./stage-pack.py --pack "$pack" --dest "$workdir" --image-set "$image_set"
  run_pair "$name" "$workdir" "$scenario" "$EE_OUT/$name.trace"
done
