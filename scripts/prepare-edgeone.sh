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

test -f "$public/index.html"
test -f "$public/assets/shime1.png"
test -f "$editor/server.py"
test -f "$editor/blobs.py"
test -f "$editor/assets/catalog/sources.json"
grep -q '動作.xml' "$editor/blobs.py"
