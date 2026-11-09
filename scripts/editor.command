#!/bin/sh
# Double-click in Finder, or run ./scripts/editor.command.
set -eu
cd "$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)"
exec python3 tools/editor/server.py --open "$@"
