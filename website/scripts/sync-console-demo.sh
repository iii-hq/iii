#!/usr/bin/env bash
# Refresh public/console-demo/ from the console repo's demo build.
#
# The landing overlay embeds the REAL console UI (src/demo/ in the console
# repo) rather than a re-implementation of it, so the artifact is built there
# and vendored here — the website has no dependency on the console's toolchain
# and CI stays standalone. Re-run this whenever the scenario or the console's
# chat/traces components change, then commit the result.
#
#   ./scripts/sync-console-demo.sh [path-to-console-repo]
#
# Default source: ../../workers/console (sibling checkout of iii-mono).
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
console="${1:-$here/../../workers/console}"
web="$console/web"
dest="$here/public/console-demo"

if [[ ! -f "$web/vite.demo.config.ts" ]]; then
  echo "console web app not found at $web" >&2
  echo "pass the console checkout as the first argument" >&2
  exit 1
fi

echo "building demo in $web"
(cd "$web" && pnpm run build:demo)

rm -rf "$dest"
mkdir -p "$dest"
cp -R "$web/dist-demo/." "$dest/"
mv "$dest/demo.html" "$dest/index.html"
# Injectable-UI shims ship with the console SPA; the demo never loads them.
rm -rf "$dest/vendor"

echo "synced $(du -sh "$dest" | cut -f1) to $dest"
