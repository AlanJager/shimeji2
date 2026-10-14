#!/bin/bash
# Clone DalekCraft2/Shimeji-Desktop at the commit in PIN, then patch Math.random.
# The checkout lives in vendor/ and is gitignored. Re-running deletes and reclones it.
set -euo pipefail

cd "$(dirname "$0")"


PIN="$(tr -d '[:space:]' < PIN)"
URL="https://github.com/DalekCraft2/Shimeji-Desktop.git"

rm -rf vendor
mkdir -p vendor
git init -q vendor
git -C vendor remote add origin "$URL"
# The proxy rejects HTTP/2 (Error in the HTTP2 framing layer). This -c flag is
# process-local and does not change git config.
git -c http.version=HTTP/1.1 -C vendor fetch --depth 1 origin "$PIN"
git -C vendor checkout --detach FETCH_HEAD

GOT="$(git -C vendor rev-parse HEAD)"
if [[ "$GOT" != "$PIN" ]]; then
  echo "fetch.sh: expected $PIN but vendor HEAD is $GOT" >&2
  exit 1
fi

python3 apply-patches.py vendor
cp vendor/LICENSE.txt UPSTREAM-LICENSE.txt
echo "$PIN" > vendor/.pinned
echo "vendored Shimeji-Desktop $PIN"
