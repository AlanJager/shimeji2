#!/bin/bash
# Lay out the working directory the engine expects: conf/ and img/shime/.
# XML and PNGs are symlinks to the bundled shime pack. Language files come from the vendor checkout.
set -euo pipefail

cd "$(dirname "$0")"
ROOT="$(cd ../.. && pwd)"
PACK="$ROOT/assets/packs/shime"
RUN="run/shime"

if [[ ! -d vendor/conf ]]; then
  echo "prepare-rundir.sh: vendor/conf is missing; run fetch.sh first" >&2
  exit 1
fi

rm -rf run
mkdir -p "$RUN/conf" "$RUN/img/shime"

cp vendor/conf/language*.properties "$RUN/conf/"

cat > "$RUN/conf/logging.properties" <<'EOF'
handlers=java.util.logging.ConsoleHandler
.level=WARNING
java.util.logging.ConsoleHandler.level=WARNING
java.util.logging.ConsoleHandler.formatter=java.util.logging.SimpleFormatter
EOF

ln -s "$PACK/conf/動作.xml" "$RUN/conf/動作.xml"
ln -s "$PACK/conf/行動.xml" "$RUN/conf/行動.xml"

shopt -s nullglob
for image in "$PACK/img/"*.png; do
  ln -s "$image" "$RUN/img/shime/$(basename "$image")"
done

echo "prepared $RUN"
