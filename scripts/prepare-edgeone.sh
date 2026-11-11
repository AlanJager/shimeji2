#!/bin/sh
# Stage the EdgeOne static site and the Python files the function reads.
set -eu
cd "$(dirname "$0")/.."

public=edgeone-public
runtime=cloud-functions/api/runtime
rm -rf "$public" "$runtime"
mkdir -p "$public/assets" "$runtime/tools/editor" "$runtime/assets/catalog" "$runtime/assets/packs"

for name in server.py media.py parts.py catalog.py dance.py; do
    cp "tools/editor/$name" "$runtime/tools/editor/$name"
done
cp -R assets/packs/shime "$runtime/assets/packs/shime"
cp assets/catalog/sources.json "$runtime/assets/catalog/sources.json"
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
test -f "$runtime/assets/packs/shime/conf/動作.xml"
test -f "$runtime/assets/catalog/sources.json"
