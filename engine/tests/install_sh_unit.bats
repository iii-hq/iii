#!/usr/bin/env bats
# Unit tests for engine/install.sh helper functions.
# Sources install.sh in test mode so the main flow doesn't execute.

setup() {
  # Resolve repo root relative to this test file
  BATS_TEST_DIRNAME="${BATS_TEST_DIRNAME:-$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)}"
  INSTALL_SH="$BATS_TEST_DIRNAME/../install.sh"
  [ -f "$INSTALL_SH" ] || { echo "install.sh not found at $INSTALL_SH" >&2; return 1; }

  # Source in test mode. The script returns early after function definitions.
  # Disable `set -e` for sourcing so a non-zero return doesn't kill the test.
  set +e
  III_INSTALL_SH_TEST_MODE=1 . "$INSTALL_SH"
  set -e
}

# ─────────────────────────────────────────────────────────────
# json_str: JSON string escaping via jq
# ─────────────────────────────────────────────────────────────

@test "json_str escapes plain text" {
  run json_str 'hello world'
  [ "$status" -eq 0 ]
  [ "$output" = '"hello world"' ]
}

@test "json_str escapes embedded double quotes" {
  run json_str 'she said "hi"'
  [ "$status" -eq 0 ]
  [ "$output" = '"she said \"hi\""' ]
}

@test "json_str escapes backslashes" {
  run json_str 'path\to\thing'
  [ "$status" -eq 0 ]
  [ "$output" = '"path\\to\\thing"' ]
}

@test "json_str escapes newlines" {
  run json_str "$(printf 'line1\nline2')"
  [ "$status" -eq 0 ]
  [ "$output" = '"line1\nline2"' ]
}

@test "json_str handles empty string" {
  run json_str ''
  [ "$status" -eq 0 ]
  [ "$output" = '""' ]
}

# ─────────────────────────────────────────────────────────────
# pkg_manager_hint: platform-aware install suggestion
# ─────────────────────────────────────────────────────────────

@test "pkg_manager_hint returns a non-empty string" {
  run pkg_manager_hint jq
  [ "$status" -eq 0 ]
  [ -n "$output" ]
  # Must reference the package name
  [[ "$output" == *"jq"* ]]
}

# ─────────────────────────────────────────────────────────────
# install_bin: installs a file with mode 755
# ─────────────────────────────────────────────────────────────

@test "install_bin copies file and sets mode 755" {
  tmpdir=$(mktemp -d)
  printf '#!/bin/sh\necho hi\n' > "$tmpdir/src"
  # source file doesn't need +x — install_bin should handle it
  install_bin "$tmpdir/src" "$tmpdir/dst"
  [ -f "$tmpdir/dst" ]
  # Mode check: portable across BSD/GNU stat
  mode=$(ls -l "$tmpdir/dst" | awk '{print $1}')
  [ "$mode" = "-rwxr-xr-x" ]
  rm -rf "$tmpdir"
}

# ─────────────────────────────────────────────────────────────
# iii_detect_from_version: reads --version output
# ─────────────────────────────────────────────────────────────

@test "iii_detect_from_version returns empty for non-existent binary" {
  run iii_detect_from_version "/nonexistent/path/to/nothing"
  [ "$status" -eq 0 ]
  [ -z "$output" ]
}

@test "iii_detect_from_version extracts last word of --version output" {
  tmpdir=$(mktemp -d)
  cat > "$tmpdir/fakebin" <<'EOF'
#!/bin/sh
echo "iii 0.11.0"
EOF
  chmod 755 "$tmpdir/fakebin"
  run iii_detect_from_version "$tmpdir/fakebin"
  [ "$status" -eq 0 ]
  [ "$output" = "0.11.0" ]
  rm -rf "$tmpdir"
}

# ─────────────────────────────────────────────────────────────
# GitHub API helpers
# ─────────────────────────────────────────────────────────────

@test "github_rate_limited returns non-zero for a successful endpoint" {
  # Against a clearly successful URL, curl should return 200 and function should return 1
  if [ -z "${CI:-}" ] && [ -z "${RUN_NETWORK_TESTS:-}" ]; then
    skip "network test — set RUN_NETWORK_TESTS=1 to run locally"
  fi
  if github_rate_limited "https://api.github.com/zen"; then
    false  # unexpectedly rate-limited (or it returned 0 for some other reason)
  else
    true
  fi
}

# ─────────────────────────────────────────────────────────────
# End-to-end: --help flag
# ─────────────────────────────────────────────────────────────

@test "install.sh --help exits 0 and prints usage" {
  run sh "$INSTALL_SH" --help
  [ "$status" -eq 0 ]
  [[ "$output" == *"Usage:"* ]]
  [[ "$output" == *"--next"* ]]
  [[ "$output" == *"TARGET"* ]]
}

@test "install.sh -h exits 0 and prints usage" {
  run sh "$INSTALL_SH" -h
  [ "$status" -eq 0 ]
  [[ "$output" == *"Usage:"* ]]
}

@test "install.sh --help mentions jq requirement" {
  run sh "$INSTALL_SH" --help
  [ "$status" -eq 0 ]
  [[ "$output" == *"jq"* ]]
}

@test "install.sh --help includes env var examples" {
  run sh "$INSTALL_SH" --help
  [ "$status" -eq 0 ]
  [[ "$output" == *"x86_64-apple-darwin"* ]]
}

# ─────────────────────────────────────────────────────────────
# Argument parsing: unknown flags
# ─────────────────────────────────────────────────────────────

@test "install.sh rejects unknown flag with clear error" {
  run sh "$INSTALL_SH" --nonsense
  [ "$status" -ne 0 ]
  [[ "$output" == *"unknown option"* ]]
  [[ "$output" == *"--nonsense"* ]]
  [[ "$output" == *"--help"* ]]
}

# ─────────────────────────────────────────────────────────────
# Deprecated flags emit a warning but don't fail
# REGRESSION: previously these silently no-op'd
# ─────────────────────────────────────────────────────────────

@test "install.sh --no-cli emits deprecation warning to stderr" {
  # Use a dependency error (TARGET=bogus) to make the script exit quickly
  # after argument parsing. We only want to observe the deprecation warning.
  run bash -c 'sh "$1" --no-cli --help 2>&1' _ "$INSTALL_SH"
  [ "$status" -eq 0 ]
  [[ "$output" == *"--no-cli is deprecated"* ]]
}

@test "install.sh --cli-version emits deprecation warning" {
  run bash -c 'sh "$1" --cli-version 1.2.3 --help 2>&1' _ "$INSTALL_SH"
  [ "$status" -eq 0 ]
  [[ "$output" == *"--cli-version is deprecated"* ]]
}

@test "install.sh --cli-dir emits deprecation warning" {
  run bash -c 'sh "$1" --cli-dir /tmp/foo --help 2>&1' _ "$INSTALL_SH"
  [ "$status" -eq 0 ]
  [[ "$output" == *"--cli-dir is deprecated"* ]]
}

@test "install.sh --cli-version without arg does not crash" {
  run bash -c 'sh "$1" --cli-version --help 2>&1' _ "$INSTALL_SH"
  [ "$status" -eq 0 ]
}

# ─────────────────────────────────────────────────────────────
# Dependency errors include fix hints
# REGRESSION: previously "curl is required" had no fix hint
# ─────────────────────────────────────────────────────────────

@test "missing curl produces error with fix hint" {
  # Can only test this where curl is NOT in a minimal PATH — most systems have
  # /usr/bin/curl so we skip there rather than running the real installer.
  if PATH="/usr/bin:/bin" command -v curl >/dev/null 2>&1; then
    skip "curl is in /usr/bin, cannot test the missing-curl path here"
  fi
  run env -i PATH="/usr/bin:/bin" HOME="$HOME" sh "$INSTALL_SH"
  [ "$status" -ne 0 ]
  [[ "$output" == *"curl is required"* ]]
  [[ "$output" == *"install"* ]]
}
