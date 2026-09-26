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

@test "install.sh --help passes environment overrides to sh" {
  run sh "$INSTALL_SH" --help
  [ "$status" -eq 0 ]
  [[ "$output" == *"curl -fsSL https://iii.dev/install.sh | VERSION=0.11.0 sh"* ]]
  [[ "$output" == *"curl -fsSL https://iii.dev/install.sh | BIN_DIR=/usr/local/bin sh"* ]]
  [[ "$output" != *"VERSION=0.11.0 curl"* ]]
  [[ "$output" != *"BIN_DIR=/usr/local/bin curl"* ]]
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

# MOT-4783: the worker must match the host libc, not the engine target.
@test "worker selects musl on Alpine x86_64" {
  run worker_target_for_host Linux x86_64 musl
  [ "$status" -eq 0 ]
  [ "$output" = "x86_64-unknown-linux-musl" ]
}

@test "worker selects musl on Alpine aarch64" {
  run worker_target_for_host Linux aarch64 musl
  [ "$status" -eq 0 ]
  [ "$output" = "aarch64-unknown-linux-musl" ]
}

@test "worker keeps GNU on glibc despite static musl engine target" {
  export TARGET=x86_64-unknown-linux-musl
  run worker_target_for_host Linux x86_64 gnu
  [ "$status" -eq 0 ]
  [ "$output" = "x86_64-unknown-linux-gnu" ]
}

@test "worker keeps GNU on glibc aarch64" {
  run worker_target_for_host Linux aarch64 gnu
  [ "$status" -eq 0 ]
  [ "$output" = "aarch64-unknown-linux-gnu" ]
}

@test "worker libc selection ignores engine glibc override" {
  export III_USE_GLIBC=1
  run worker_target_for_host Linux x86_64 musl
  [ "$status" -eq 0 ]
  [ "$output" = "x86_64-unknown-linux-musl" ]
}

@test "worker preserves macOS Apple Silicon support" {
  run worker_target_for_host Darwin aarch64 gnu
  [ "$status" -eq 0 ]
  [ "$output" = "aarch64-apple-darwin" ]
}

@test "worker does not invent assets on unsupported platforms" {
  run worker_target_for_host Darwin x86_64 gnu
  [ "$status" -eq 0 ]
  [ -z "$output" ]
  run worker_target_for_host Linux armv7 gnu
  [ "$status" -eq 0 ]
  [ -z "$output" ]
}

# ─────────────────────────────────────────────────────────────
# --start-with / --need-envs: the harness setup offer
# ─────────────────────────────────────────────────────────────

@test "learn_args_for returns --learn-iii alone by default" {
  run learn_args_for "" ""
  [ "$status" -eq 0 ]
  [ "$output" = "--learn-iii" ]
}

@test "learn_args_for passes the worker list through" {
  run learn_args_for "worker1,worker2" ""
  [ "$status" -eq 0 ]
  [ "$output" = "--learn-iii --start-with worker1,worker2" ]
}

@test "learn_args_for passes the extra env vars through" {
  run learn_args_for "worker1" "WORKER_API_KEY,SECOND_KEY"
  [ "$status" -eq 0 ]
  [ "$output" = "--learn-iii --start-with worker1 --need-envs WORKER_API_KEY,SECOND_KEY" ]
}

@test "learn_args_for drops --need-envs without --start-with" {
  # The engine rejects the flag on its own, so never build a command it refuses.
  run learn_args_for "" "WORKER_API_KEY"
  [ "$status" -eq 0 ]
  [ "$output" = "--learn-iii" ]
}

@test "install.sh --start-with rejects the next option as its value" {
  # `shift 2` would otherwise swallow the flag, leaving it unset.
  run sh "$INSTALL_SH" --start-with --skip-bin-download
  [ "$status" -ne 0 ]
  [[ "$output" == *"--start-with needs a comma-separated worker list"* ]]
  [[ "$output" == *"--skip-bin-download"* ]]
}

@test "install.sh --need-envs rejects the next option as its value" {
  run sh "$INSTALL_SH" --need-envs -h
  [ "$status" -ne 0 ]
  [[ "$output" == *"--need-envs needs a comma-separated variable list"* ]]
}

@test "install.sh passes a version selector through without globbing it" {
  # `worker@*` is a legal selector; an unquoted `*` would glob against the
  # working directory instead.
  _bin="$BATS_TEST_TMPDIR/bin"
  mkdir -p "$_bin"
  printf '#!/bin/sh\nexit 0\n' > "$_bin/iii"
  chmod +x "$_bin/iii"

  cd "$BATS_TEST_TMPDIR"
  touch decoy-file

  run env BIN_DIR="$_bin" sh "$INSTALL_SH" --skip-bin-download --start-with 'worker1@*' </dev/null
  [ "$status" -eq 0 ]
  [[ "$output" == *"--start-with worker1@*"* ]]
  [[ "$output" != *"decoy-file"* ]]
}

@test "install.sh --start-with rejects whitespace" {
  run sh "$INSTALL_SH" --start-with "worker1, worker2"
  [ "$status" -ne 0 ]
  [[ "$output" == *"--start-with does not accept whitespace"* ]]
}

@test "install.sh --need-envs rejects whitespace" {
  run sh "$INSTALL_SH" --need-envs "A B"
  [ "$status" -ne 0 ]
  [[ "$output" == *"--need-envs does not accept whitespace"* ]]
}

@test "install.sh --help documents --start-with and --need-envs" {
  run sh "$INSTALL_SH" --help
  [ "$status" -eq 0 ]
  [[ "$output" == *"--start-with LIST"* ]]
  [[ "$output" == *"--need-envs LIST"* ]]
}

@test "install.sh --help documents --skip-bin-download" {
  run sh "$INSTALL_SH" --help
  [ "$status" -eq 0 ]
  [[ "$output" == *"--skip-bin-download"* ]]
}

@test "install.sh --skip-bin-download reaches the setup offer without installing" {
  # No release is resolved and no asset is downloaded, so this needs no
  # network: the offer runs against whatever iii is already on this machine.
  #
  # That "whatever" is a stub here. The offer is only made when the binary in
  # BIN_DIR accepts the flags this run would pass, and a machine with no iii
  # at all — every CI runner — is told about the quickstart instead.
  _bin="$BATS_TEST_TMPDIR/bin"
  mkdir -p "$_bin"
  printf '#!/bin/sh\nexit 0\n' > "$_bin/iii"
  chmod +x "$_bin/iii"

  run env BIN_DIR="$_bin" sh "$INSTALL_SH" --skip-bin-download --start-with worker1 </dev/null
  [ "$status" -eq 0 ]
  [[ "$output" == *"--learn-iii --start-with worker1"* ]]
  [[ "$output" != *"Downloading"* ]]
}

@test "install.sh --skip-bin-download names no command a binary would reject" {
  # A binary that does not know the flags must never be handed them, so the
  # run that cannot offer the setup names the quickstart instead.
  _bin="$BATS_TEST_TMPDIR/oldbin"
  mkdir -p "$_bin"
  printf '#!/bin/sh\nexit 2\n' > "$_bin/iii"
  chmod +x "$_bin/iii"

  run env BIN_DIR="$_bin" sh "$INSTALL_SH" --skip-bin-download --start-with worker1 </dev/null
  [ "$status" -eq 0 ]
  [[ "$output" != *"--start-with worker1"* ]]
  [[ "$output" == *"quickstart"* ]]
}

@test "cleanup is harmless when no download directory was made" {
  # --skip-bin-download never creates one, and the harness prompt re-arms
  # the trap that calls this.
  unset tmpdir
  run cleanup
  [ "$status" -eq 0 ]
}

@test "cleanup removes the download directory" {
  tmpdir="$BATS_TEST_TMPDIR/dl"
  mkdir -p "$tmpdir"
  run cleanup
  [ "$status" -eq 0 ]
  [ ! -d "$tmpdir" ]
}
