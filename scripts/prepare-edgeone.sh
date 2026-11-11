#!/bin/sh
# Stage the static site and the Python files the function builder packages.
set -eu
cd "$(dirname "$0")/.."

public=edgeone-public
editor=cloud-functions/api/editor
rm -rf "$public" "$editor"
mkdir -p "$public/assets"

python3 scripts/stage-edgeone-editor.py
cp assets/packs/shime/img/shime*.png "$public/assets/"

find tools/editor -type f \
    ! -name '*.py' \
    ! -name '*.test.mjs' \
    ! -name 'package.json' \
    ! -name 'package-lock.json' \
    ! -name 'build-vrm-vendor.mjs' \
    ! -name '.gitignore' |
while IFS= read -r file; do
    relative=${file#tools/editor/}
    mkdir -p "$public/$(dirname "$relative")"
    cp "$file" "$public/$relative"
done

# EdgeOne serves .mjs as application/octet-stream, and browsers will not run a module with that type.
python3 - "$public" <<'PY'
import sys
from pathlib import Path
public = Path(sys.argv[1])
for path in public.rglob("*"):
    if path.is_file() and path.suffix in {".html", ".mjs", ".css", ".js", ".json"} and ".mjs" in path.read_text(encoding="utf-8"):
        path.write_text(path.read_text(encoding="utf-8").replace(".mjs", ".js"), encoding="utf-8")
for path in list(public.rglob("*.mjs")):
    path.rename(path.with_suffix(".js"))
PY

test -f "$public/index.html"
test -f "$public/editor.js"
test ! -f "$public/editor.mjs"
grep -q 'src="/editor.js"' "$public/index.html"
grep -q "from './model.js'" "$public/editor.js"
test -f "$public/assets/shime1.png"
test -f "$editor/server.py"
test -f "$editor/blobs.py"
test -f "$editor/assets/catalog/sources.json"
grep -q '動作.xml' "$editor/blobs.py"
