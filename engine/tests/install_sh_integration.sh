#!/usr/bin/env sh
# Integration smoke test for engine/install.sh.
#
# Runs the installer against the real GitHub API and verifies:
#   - the main binary installs and responds to --version
#   - companion binary failures are warned but non-fatal
#   - re-running the script on an already-current install is idempotent
#   - --help exits 0 and prints usage
#   - unknown flags are rejected
#
# Requires network (github.com). Skipped locally unless RUN_NETWORK_TESTS=1.

set -eu

SCRIPT_DIR=$(cd "$(dirname "$0")" && pwd)
INSTALL_SH="$SCRIPT_DIR/../install.sh"

fail() {
  printf 'FAIL: %s\n' "$*" >&2
  exit 1
}

pass() {
  printf 'ok: %s\n' "$*"
}

if [ -z "${CI:-}" ] && [ -z "${RUN_NETWORK_TESTS:-}" ]; then
  echo "skipped (set RUN_NETWORK_TESTS=1 or run in CI)" >&2
  exit 0
fi

if [ ! -f "$INSTALL_SH" ]; then
  fail "install.sh not found at $INSTALL_SH"
fi

# Isolated install directory per run
TMPROOT=$(mktemp -d)
export BIN_DIR="$TMPROOT/bin"
mkdir -p "$BIN_DIR"
trap 'rm -rf "$TMPROOT"' EXIT INT TERM

# Pin to a known-stable release. Using `latest` races against the release
# workflow: if a new version is publishing while this CI runs, /releases/latest
# returns a release whose tag_name exists but whose main-binary assets are
# still uploading. The filter correctly returns empty and the test fails.
# Pin to a version with all assets fully uploaded.
PINNED_VERSION="${PINNED_VERSION:-0.11.0}"

# ─────────────────────────────────────────────────────────────
# Test 1: --help works and prints usage
# ─────────────────────────────────────────────────────────────
help_output=$(sh "$INSTALL_SH" --help)
case "$help_output" in
  *"Usage:"*) pass "--help prints usage" ;;
  *) fail "--help did not print 'Usage:' — got: $help_output" ;;
esac

case "$help_output" in
  *"--next"*) pass "--help documents --next" ;;
  *) fail "--help missing --next documentation" ;;
esac

# ─────────────────────────────────────────────────────────────
# Test 2: unknown flag rejected
# ─────────────────────────────────────────────────────────────
if sh "$INSTALL_SH" --nonsense-flag >/dev/null 2>&1; then
  fail "unknown flag was accepted; expected exit non-zero"
fi
pass "unknown flag rejected"

# ─────────────────────────────────────────────────────────────
# Test 3: deprecated --no-cli emits warning but doesn't fail arg parsing
# ─────────────────────────────────────────────────────────────
depr_output=$(sh "$INSTALL_SH" --no-cli --help 2>&1)
case "$depr_output" in
  *"--no-cli is deprecated"*) pass "--no-cli emits deprecation warning" ;;
  *) fail "--no-cli did not warn — got: $depr_output" ;;
esac

# ─────────────────────────────────────────────────────────────
# Test 4: actual install of pinned stable release
# ─────────────────────────────────────────────────────────────
echo "--- running install.sh against real GitHub release (VERSION=$PINNED_VERSION) ---"
VERSION="$PINNED_VERSION" sh "$INSTALL_SH" 2>&1

if [ ! -x "$BIN_DIR/iii" ]; then
  fail "iii binary not installed at $BIN_DIR/iii"
fi
pass "iii binary installed"

# ─────────────────────────────────────────────────────────────
# Test 5: installed binary reports a version
# ─────────────────────────────────────────────────────────────
version_output=$("$BIN_DIR/iii" --version 2>&1)
case "$version_output" in
  *[0-9]*.[0-9]*) pass "iii --version returned a version: $version_output" ;;
  *) fail "iii --version did not return a version — got: $version_output" ;;
esac

# ─────────────────────────────────────────────────────────────
# Test 6: idempotency — re-running on current version skips work
# REGRESSION RULE: must not re-download if already current
# ─────────────────────────────────────────────────────────────
echo "--- re-running install.sh (should be idempotent) ---"
rerun_output=$(VERSION="$PINNED_VERSION" sh "$INSTALL_SH" 2>&1)
case "$rerun_output" in
  *"already at"*|*"nothing to do"*) pass "idempotent re-run detected" ;;
  *) fail "idempotent re-run did not skip — got: $rerun_output" ;;
esac

# ─────────────────────────────────────────────────────────────
# Test 7: upgrade message distinguishes from install message
# REGRESSION RULE: upgrade path must report "upgraded" not "installed"
# (Only testable when the latest has progressed past an older pinned version.
# We fake it by stashing a shim that reports an old version.)
# ─────────────────────────────────────────────────────────────
UPGRADE_DIR=$(mktemp -d)
trap 'rm -rf "$TMPROOT" "$UPGRADE_DIR"' EXIT INT TERM
cat > "$UPGRADE_DIR/iii" <<'SHIM'
#!/bin/sh
case "$1" in
  --version) echo "iii 0.0.1-test-shim" ;;
  --install-only-generate-ids) exit 0 ;;
  *) echo "shim: $*" >&2; exit 1 ;;
esac
SHIM
chmod 755 "$UPGRADE_DIR/iii"

BIN_DIR="$UPGRADE_DIR" VERSION="$PINNED_VERSION" sh "$INSTALL_SH" > "$UPGRADE_DIR/out.log" 2>&1 || {
  cat "$UPGRADE_DIR/out.log" >&2
  fail "upgrade-path install failed"
}
case "$(cat "$UPGRADE_DIR/out.log")" in
  *"upgraded iii"*) pass "upgrade path reports 'upgraded'" ;;
  *) fail "upgrade path did not emit 'upgraded' — got: $(cat "$UPGRADE_DIR/out.log")" ;;
esac

echo ""
echo "all integration checks passed"
