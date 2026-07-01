#!/usr/bin/env bash
# Self-check for regen-golden.sh's SIZE_GIB validation guard.
#
# A bad SIZE_GIB (typo or injection) must be rejected BEFORE the Docker
# container command runs — otherwise it either fails cryptically or silently
# mints a wrong-capacity golden that gets committed and embedded into every
# worker. No Docker/Rust needed: shims on PATH stand in for `docker`/`cargo`
# so the happy path runs offline, and a marker file proves a rejected value
# never reached the container command.
set -uo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
SCRIPT="$HERE/regen-golden.sh"

SHIM="$(mktemp -d)"
trap 'rm -rf "$SHIM"' EXIT
MARKER="$SHIM/docker-was-called"
printf '#!/usr/bin/env bash\ntouch "%s"\n' "$MARKER" > "$SHIM/docker"
printf '#!/usr/bin/env bash\nexit 0\n' > "$SHIM/cargo"
chmod +x "$SHIM/docker" "$SHIM/cargo"

run() { # $1 = SIZE_GIB value; returns the script's exit code
  rm -f "$MARKER"
  PATH="$SHIM:$PATH" SIZE_GIB="$1" bash "$SCRIPT" >/dev/null 2>&1
}

fail=0
assert_reject() {
  run "$1"; local rc=$?
  if [ "$rc" -ne 2 ]; then echo "FAIL reject '$1': expected exit 2, got $rc"; fail=1
  elif [ -f "$MARKER" ]; then echo "FAIL reject '$1': reached docker (guard missing)"; fail=1
  else echo "ok   reject '$1'"; fi
}
assert_accept() {
  run "$1"; local rc=$?
  if [ "$rc" -ne 0 ]; then echo "FAIL accept '$1': expected exit 0, got $rc"; fail=1
  else echo "ok   accept '$1'"; fi
}

assert_reject "abc"
assert_reject "0"
assert_reject "8.5"
assert_reject "-4"
assert_reject "16; touch pwned"
assert_accept "16"
assert_accept "8"

[ "$fail" -eq 0 ] && { echo "PASS"; exit 0; } || { echo "FAILED"; exit 1; }
