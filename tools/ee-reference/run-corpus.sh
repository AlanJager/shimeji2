#!/bin/bash
# Run every tests/scenarios/shime-*.json against each corpus/packs/<id> the reference can load.
# Traces go to corpus/traces/<id>/<scenario>.trace, which is gitignored. A summary is printed
# and written to corpus/traces/summary.json (also gitignored).
set -euo pipefail

cd "$(dirname "$0")"


PIN="$(tr -d '[:space:]' < PIN)"
if [[ ! -f vendor/.pinned ]] || [[ "$(tr -d '[:space:]' < vendor/.pinned)" != "$PIN" ]]; then
  ./fetch.sh
fi

gradle -q printClasspath --console=plain
exec python3 ./run-corpus.py
