#!/usr/bin/env bash
# Dev loop for the console demo: poll the demo's sources once a second and run
# a one-shot build into public/console-demo/ only when a file actually changed
# (vite --watch loops: tailwind's content scan re-triggers on its own output).
# Refresh the page after "built @ ...".
#
#   ./scripts/dev-console-demo.sh [path-to-console-repo]
set -uo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
console="${1:-$here/../../workers/console}"
web="$console/web"
dest="$here/public/console-demo"

# Content hash, not mtime: BSD and GNU stat disagree on flags.
snap() {
  find "$web/src" "$web/demo.html" "$web/vite.demo.config.ts" -type f \
    -exec shasum {} + | sort | shasum
}

last=""
while :; do
  cur="$(snap)"
  if [[ "$cur" != "$last" ]]; then
    last="$cur"
    if (cd "$web" && pnpm exec vite build --config vite.demo.config.ts \
      --outDir "$dest" --emptyOutDir >/dev/null 2>&1) &&
      mv "$dest/demo.html" "$dest/index.html" &&
      rm -rf "$dest/vendor"; then
      echo "built @ $(date +%T)"
    else
      echo "build failed @ $(date +%T) — rerun without >/dev/null to see why"
    fi
  fi
  sleep 1
done
