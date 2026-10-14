#!/bin/bash
# Dump java.util.Random and Nashorn expression vectors for crates/shimeji-expr.
set -euo pipefail

cd "$(dirname "$0")"


PIN="$(tr -d '[:space:]' < PIN)"
if [[ ! -f vendor/.pinned ]] || [[ "$(tr -d '[:space:]' < vendor/.pinned)" != "$PIN" ]]; then
  ./fetch.sh
fi

gradle -q printClasspath --console=plain
CP="$(cat build/runtime-classpath.txt)"
REPO="$(cd ../.. && pwd)"
OUT_DIR="$REPO/crates/shimeji-expr/tests/vectors"
mkdir -p "$OUT_DIR"

exec java \
  -Dfile.encoding=UTF-8 \
  -cp "$CP" \
  shimeji2.ref.ExprVectors \
  --shime-conf "$REPO/assets/packs/shime/conf" \
  --out-nashorn "$OUT_DIR/nashorn.json" \
  --out-random "$OUT_DIR/java_random.json"
