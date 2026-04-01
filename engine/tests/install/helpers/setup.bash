#!/usr/bin/env bash
# Shared BATS setup/teardown for install.sh tests.

INSTALL_SH="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)/install.sh"
FIXTURES_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../fixtures" && pwd)"
HELPERS_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

# shellcheck source=mocks.bash
source "${HELPERS_DIR}/mocks.bash"

common_setup() {
  SANDBOX="$(mktemp -d)"
  MOCK_BIN_DIR="${SANDBOX}/mock_bin"
  TELEMETRY_LOG="${SANDBOX}/telemetry.log"
  FAKE_HOME="${SANDBOX}/home"
  FAKE_BIN_DIR="${SANDBOX}/install_bin"
  MOCK_CURL_RESPONSES="${SANDBOX}/curl_responses"

  mkdir -p "$MOCK_BIN_DIR" "$FAKE_HOME" "$FAKE_BIN_DIR" "$MOCK_CURL_RESPONSES"

  export HOME="$FAKE_HOME"
  export MOCK_CURL_TELEMETRY_LOG="$TELEMETRY_LOG"
  export MOCK_BIN_DIR
  export MOCK_CURL_RESPONSES

  export ORIGINAL_PATH="$PATH"
  export PATH="${MOCK_BIN_DIR}:${PATH}"

  # Default env for install.sh
  export BIN_DIR="$FAKE_BIN_DIR"
  export TARGET="x86_64-apple-darwin"
  export REPO="iii-hq/iii"
  export BIN_NAME="iii"
  export VERSION="0.8.0"

  # Ensure telemetry is enabled in tests (unset CI vars)
  unset CI GITHUB_ACTIONS GITLAB_CI CIRCLECI JENKINS_URL TRAVIS
  unset BUILDKITE TF_BUILD CODEBUILD_BUILD_ID BITBUCKET_BUILD_NUMBER
  unset DRONE TEAMCITY_VERSION
  unset III_TELEMETRY_ENABLED

  # Create default mocks
  create_mock_curl
  create_mock_uname "Darwin" "x86_64"
  create_mock_tar
  create_mock_install_cmd
  create_mock_uuidgen

  # Default: curl returns release_single.json for all API calls
  setup_curl_api_response "*" "${FIXTURES_DIR}/release_single.json"
  create_fake_archive "${SANDBOX}/fake_archive"
  setup_curl_download "${SANDBOX}/fake_archive/iii-x86_64-apple-darwin.tar.gz"
}

common_teardown() {
  if [ -n "${SANDBOX:-}" ] && [ -d "$SANDBOX" ]; then
    rm -rf "$SANDBOX"
  fi
  if [ -n "${ORIGINAL_PATH:-}" ]; then
    export PATH="$ORIGINAL_PATH"
  fi
}

# Run install.sh capturing stdout, stderr, and exit code.
# Also waits briefly for backgrounded telemetry POSTs to flush.
run_install_sh() {
  local _stdout_file _stderr_file
  _stdout_file="${SANDBOX}/run_stdout"
  _stderr_file="${SANDBOX}/run_stderr"
  set +e
  sh "$INSTALL_SH" "$@" >"$_stdout_file" 2>"$_stderr_file"
  status=$?
  set -e
  # Allow backgrounded telemetry curl subprocesses to finish writing
  sleep 0.2
  stdout_output="$(cat "$_stdout_file")"
  stderr_output="$(cat "$_stderr_file")"
  output="${stdout_output}${stderr_output:+
${stderr_output}}"
}

# ---- Telemetry assertion helpers ----

# Assert telemetry log contains an event of the given type
assert_telemetry_event() {
  local event_type="$1"
  if [ ! -f "$TELEMETRY_LOG" ]; then
    echo "Expected telemetry event '${event_type}' but telemetry log does not exist"
    return 1
  fi
  grep -q "event_type=${event_type}" "$TELEMETRY_LOG" \
    || { echo "Expected telemetry log to contain event_type=${event_type}"; cat "$TELEMETRY_LOG"; return 1; }
}

# Assert telemetry log does NOT contain an event of the given type
refute_telemetry_event() {
  local event_type="$1"
  if [ ! -f "$TELEMETRY_LOG" ]; then
    return 0
  fi
  if grep -q "event_type=${event_type}" "$TELEMETRY_LOG" 2>/dev/null; then
    echo "Expected telemetry log NOT to contain event_type=${event_type}"
    cat "$TELEMETRY_LOG"
    return 1
  fi
}

# Count total events in telemetry log
telemetry_event_count() {
  if [ -f "$TELEMETRY_LOG" ]; then
    grep -c "event_type=" "$TELEMETRY_LOG" 2>/dev/null || echo 0
  else
    echo 0
  fi
}

# Get the raw payload line for a specific event type
get_telemetry_payload() {
  local event_type="$1"
  grep "event_type=${event_type}" "$TELEMETRY_LOG" | head -1 | sed 's/.*payload=//'
}

# Assert that a telemetry event payload contains a substring
assert_telemetry_payload_contains() {
  local event_type="$1" expected_substr="$2"
  local payload
  payload=$(get_telemetry_payload "$event_type")
  if ! printf '%s' "$payload" | grep -qF "$expected_substr"; then
    echo "Expected ${event_type} payload to contain '${expected_substr}'"
    echo "Actual payload: ${payload}"
    return 1
  fi
}

# Set up an "existing binary" at FAKE_BIN_DIR for upgrade tests
setup_existing_binary() {
  local version="${1:-0.7.0}"
  create_mock_iii_binary "$FAKE_BIN_DIR" "$version"
}

# Remove existing binary for fresh install tests (default state)
remove_existing_binary() {
  rm -f "${FAKE_BIN_DIR}/iii"
}

# Remove a command from PATH by shadowing directories that contain it.
# Creates symlinks for every executable EXCEPT the target into a shadow dir,
# then replaces the original directory in PATH with the shadow.
# Usage: hide_command_from_path <command_name>
hide_command_from_path() {
  local cmd="$1"
  rm -f "${MOCK_BIN_DIR}/${cmd}"
  local shadow="${SANDBOX}/${cmd}_shadow"
  mkdir -p "$shadow"
  local new_path="${MOCK_BIN_DIR}" dir bn
  local saved_ifs="$IFS"
  IFS=":"
  for dir in $PATH; do
    [ "$dir" = "$MOCK_BIN_DIR" ] && continue
    if [ -x "${dir}/${cmd}" ]; then
      for f in "$dir"/*; do
        [ ! -x "$f" ] && continue
        bn=$(basename "$f")
        [ "$bn" = "$cmd" ] && continue
        [ -e "${shadow}/${bn}" ] && continue
        ln -s "$f" "${shadow}/${bn}" 2>/dev/null || true
      done
      new_path="${new_path}:${shadow}"
    else
      new_path="${new_path}:${dir}"
    fi
  done
  IFS="$saved_ifs"
  export PATH="$new_path"
}

hide_jq_from_path() {
  hide_command_from_path "jq"
}

hide_curl_from_path() {
  hide_command_from_path "curl"
}

hide_unzip_from_path() {
  rm -f "${MOCK_BIN_DIR}/unzip"
  hide_command_from_path "unzip"
}
