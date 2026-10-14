#!/bin/bash
# Compile if needed and run one scenario. Extra args go to TraceRunner.
# Working directory for the JVM is the staged pack, or run/shime when --pack is omitted.
# Pass absolute --scenario and --out paths.
set -euo pipefail

cd "$(dirname "$0")"


PIN="$(tr -d '[:space:]' < PIN)"
if [[ ! -f vendor/.pinned ]] || [[ "$(tr -d '[:space:]' < vendor/.pinned)" != "$PIN" ]]; then
  ./fetch.sh
fi

PACK=""
IMAGE_SET=""
ARGS=()
while [[ $# -gt 0 ]]; do
  case "$1" in
    --pack)
      PACK="$2"
      shift 2
      ;;
    --image-set)
      IMAGE_SET="$2"
      shift 2
      ;;
    *)
      ARGS+=("$1")
      shift
      ;;
  esac
done

SCENARIO=""
prev=""
for arg in "${ARGS[@]}"; do
  if [[ "$prev" == "--scenario" ]]; then
    SCENARIO="$arg"
  fi
  prev="$arg"
done

if [[ -n "$PACK" ]]; then
  if [[ -z "$IMAGE_SET" ]]; then
    if [[ -z "$SCENARIO" ]]; then
      echo "run.sh: --pack needs --image-set or --scenario" >&2
      exit 1
    fi
    IMAGE_SET="$(python3 -c 'import json,sys; print(json.load(open(sys.argv[1])).get("imageSet","shime"))' "$SCENARIO")"
  fi
  DEST="run/pack"
  rm -rf "$DEST"
  python3 ./stage-pack.py --pack "$PACK" --dest "$DEST" --image-set "$IMAGE_SET"
  WORKDIR="$DEST"
else
  ./prepare-rundir.sh
  WORKDIR="run/shime"
fi

gradle -q printClasspath --console=plain
CP="$(cat build/runtime-classpath.txt)"
cd "$WORKDIR"
exec java \
  -Dshimeji2.ref.factory=headless \
  -Dshimeji2.ref.pin="$PIN" \
  -Dfile.encoding=UTF-8 \
  -cp "$CP" \
  shimeji2.ref.TraceRunner \
  "${ARGS[@]}"
