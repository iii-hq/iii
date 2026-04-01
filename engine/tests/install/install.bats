#!/usr/bin/env bats
# Comprehensive test suite for engine/install.sh

HELPERS_DIR="$(cd "$(dirname "$BATS_TEST_FILENAME")/helpers" && pwd)"
source "${HELPERS_DIR}/setup.bash"

setup() {
  common_setup
}

teardown() {
  common_teardown
}

# ===========================================================================
# Section A: Utility functions (unit tests via source_functions.bash)
# ===========================================================================

@test "A1: iii_telemetry_enabled returns true when no CI vars set" {
  source "${HELPERS_DIR}/source_functions.bash"
  unset CI GITHUB_ACTIONS GITLAB_CI CIRCLECI JENKINS_URL TRAVIS
  unset BUILDKITE TF_BUILD CODEBUILD_BUILD_ID BITBUCKET_BUILD_NUMBER DRONE TEAMCITY_VERSION
  unset III_TELEMETRY_ENABLED
  run iii_telemetry_enabled
  [ "$status" -eq 0 ]
}

@test "A2: iii_telemetry_enabled returns false when III_TELEMETRY_ENABLED=false" {
  source "${HELPERS_DIR}/source_functions.bash"
  export III_TELEMETRY_ENABLED=false
  run iii_telemetry_enabled
  [ "$status" -eq 1 ]
}

@test "A3: iii_telemetry_enabled returns false when III_TELEMETRY_ENABLED=0" {
  source "${HELPERS_DIR}/source_functions.bash"
  export III_TELEMETRY_ENABLED=0
  run iii_telemetry_enabled
  [ "$status" -eq 1 ]
}

@test "A4: iii_telemetry_enabled returns false when CI=true" {
  source "${HELPERS_DIR}/source_functions.bash"
  unset III_TELEMETRY_ENABLED
  export CI=true
  run iii_telemetry_enabled
  [ "$status" -eq 1 ]
}

@test "A5: iii_telemetry_enabled returns false when GITHUB_ACTIONS=true" {
  source "${HELPERS_DIR}/source_functions.bash"
  unset III_TELEMETRY_ENABLED
  export GITHUB_ACTIONS=true
  run iii_telemetry_enabled
  [ "$status" -eq 1 ]
}

@test "A6: iii_gen_uuid produces a UUID-like string" {
  source "${HELPERS_DIR}/source_functions.bash"
  result=$(iii_gen_uuid)
  [[ "$result" =~ [0-9a-f-]{36} ]] || [[ "$result" =~ [0-9a-fA-F-]{36} ]]
}

@test "A7: iii_toml_path returns correct path" {
  source "${HELPERS_DIR}/source_functions.bash"
  result=$(iii_toml_path)
  [ "$result" = "${HOME}/.iii/telemetry.toml" ]
}

@test "A8: iii_read_toml_key returns empty for missing file" {
  source "${HELPERS_DIR}/source_functions.bash"
  result=$(iii_read_toml_key "identity" "id")
  [ -z "$result" ]
}

@test "A9: iii_read_toml_key reads existing key from TOML" {
  source "${HELPERS_DIR}/source_functions.bash"
  mkdir -p "${HOME}/.iii"
  cat > "${HOME}/.iii/telemetry.toml" <<'TOML'
[identity]
id = "test-uuid-123"
TOML
  result=$(iii_read_toml_key "identity" "id")
  [ "$result" = "test-uuid-123" ]
}

@test "A10: iii_read_toml_key returns empty for missing key" {
  source "${HELPERS_DIR}/source_functions.bash"
  mkdir -p "${HOME}/.iii"
  cat > "${HOME}/.iii/telemetry.toml" <<'TOML'
[identity]
other = "value"
TOML
  result=$(iii_read_toml_key "identity" "id")
  [ -z "$result" ]
}

@test "A11: iii_read_toml_key returns empty for wrong section" {
  source "${HELPERS_DIR}/source_functions.bash"
  mkdir -p "${HOME}/.iii"
  cat > "${HOME}/.iii/telemetry.toml" <<'TOML'
[other_section]
id = "wrong-section"
TOML
  result=$(iii_read_toml_key "identity" "id")
  [ -z "$result" ]
}

@test "A12: iii_set_toml_key creates new file with section and key" {
  source "${HELPERS_DIR}/source_functions.bash"
  iii_set_toml_key "identity" "id" "new-uuid-456"
  result=$(iii_read_toml_key "identity" "id")
  [ "$result" = "new-uuid-456" ]
}

@test "A13: iii_set_toml_key updates existing key in section" {
  source "${HELPERS_DIR}/source_functions.bash"
  mkdir -p "${HOME}/.iii"
  cat > "${HOME}/.iii/telemetry.toml" <<'TOML'
[identity]
id = "old-value"
TOML
  iii_set_toml_key "identity" "id" "updated-value"
  result=$(iii_read_toml_key "identity" "id")
  [ "$result" = "updated-value" ]
}

@test "A14: iii_set_toml_key adds key to existing section" {
  source "${HELPERS_DIR}/source_functions.bash"
  mkdir -p "${HOME}/.iii"
  cat > "${HOME}/.iii/telemetry.toml" <<'TOML'
[identity]
other = "keep-this"
TOML
  iii_set_toml_key "identity" "id" "added-value"
  result_id=$(iii_read_toml_key "identity" "id")
  result_other=$(iii_read_toml_key "identity" "other")
  [ "$result_id" = "added-value" ]
  [ "$result_other" = "keep-this" ]
}

@test "A15: iii_get_or_create_telemetry_id returns existing ID" {
  source "${HELPERS_DIR}/source_functions.bash"
  mkdir -p "${HOME}/.iii"
  cat > "${HOME}/.iii/telemetry.toml" <<'TOML'
[identity]
id = "existing-uuid"
TOML
  result=$(iii_get_or_create_telemetry_id)
  [ "$result" = "existing-uuid" ]
}

@test "A16: iii_get_or_create_telemetry_id migrates legacy telemetry_id file" {
  source "${HELPERS_DIR}/source_functions.bash"
  mkdir -p "${HOME}/.iii"
  echo "legacy-id-789" > "${HOME}/.iii/telemetry_id"
  result=$(iii_get_or_create_telemetry_id)
  [ "$result" = "legacy-id-789" ]
  # Also persisted to TOML
  toml_result=$(iii_read_toml_key "identity" "id")
  [ "$toml_result" = "legacy-id-789" ]
}

@test "A17: iii_get_or_create_telemetry_id creates new auto-prefixed ID" {
  source "${HELPERS_DIR}/source_functions.bash"
  result=$(iii_get_or_create_telemetry_id)
  [[ "$result" == auto-* ]]
  # Persisted to TOML
  toml_result=$(iii_read_toml_key "identity" "id")
  [ "$toml_result" = "$result" ]
}

@test "A18: iii_send_event logs event to Amplitude endpoint via curl" {
  source "${HELPERS_DIR}/source_functions.bash"
  export MOCK_CURL_TELEMETRY_LOG="$TELEMETRY_LOG"
  iii_send_event "test_event" '"key":"value"' "device-123" "install-456"
  wait
  sleep 0.2
  [ -f "$TELEMETRY_LOG" ]
  grep -q "event_type=test_event" "$TELEMETRY_LOG"
}

@test "A19: iii_send_event skips when telemetry is disabled" {
  source "${HELPERS_DIR}/source_functions.bash"
  export III_TELEMETRY_ENABLED=false
  export MOCK_CURL_TELEMETRY_LOG="$TELEMETRY_LOG"
  iii_send_event "should_not_send" '"key":"value"' "device-123" "install-456"
  wait
  sleep 0.1
  [ ! -f "$TELEMETRY_LOG" ] || ! grep -q "event_type=" "$TELEMETRY_LOG"
}

@test "A20: iii_send_event skips when API key is empty" {
  source "${HELPERS_DIR}/source_functions.bash"
  AMPLITUDE_API_KEY=""
  export MOCK_CURL_TELEMETRY_LOG="$TELEMETRY_LOG"
  iii_send_event "should_not_send" '"key":"value"' "device-123" "install-456"
  wait
  sleep 0.1
  [ ! -f "$TELEMETRY_LOG" ] || ! grep -q "event_type=" "$TELEMETRY_LOG"
}

@test "A21: iii_detect_from_version returns version for existing binary" {
  source "${HELPERS_DIR}/source_functions.bash"
  create_mock_iii_binary "${SANDBOX}/detect_bin" "1.2.3"
  result=$(iii_detect_from_version "${SANDBOX}/detect_bin/iii")
  [ "$result" = "1.2.3" ]
}

@test "A22: iii_detect_from_version returns empty for missing binary" {
  source "${HELPERS_DIR}/source_functions.bash"
  result=$(iii_detect_from_version "/nonexistent/path/iii")
  [ -z "$result" ]
}

@test "A23: iii_export_host_user_id writes to shell profile" {
  source "${HELPERS_DIR}/source_functions.bash"
  mkdir -p "${HOME}/.iii"
  cat > "${HOME}/.iii/telemetry.toml" <<'TOML'
[identity]
id = "profile-test-id"
TOML
  # Create a .bashrc for the function to write to
  touch "${HOME}/.bashrc"
  iii_export_host_user_id
  grep -q 'III_HOST_USER_ID' "${HOME}/.bashrc"
  grep -q 'profile-test-id' "${HOME}/.bashrc"
}

@test "A24: iii_export_host_user_id skips if already present" {
  source "${HELPERS_DIR}/source_functions.bash"
  mkdir -p "${HOME}/.iii"
  cat > "${HOME}/.iii/telemetry.toml" <<'TOML'
[identity]
id = "profile-test-id"
TOML
  printf 'export III_HOST_USER_ID="existing"\n' > "${HOME}/.bashrc"
  iii_export_host_user_id
  count=$(grep -c 'III_HOST_USER_ID' "${HOME}/.bashrc")
  [ "$count" -eq 1 ]
}

@test "A25: iii_export_host_user_id skips if no telemetry id" {
  source "${HELPERS_DIR}/source_functions.bash"
  touch "${HOME}/.bashrc"
  iii_export_host_user_id
  ! grep -q 'III_HOST_USER_ID' "${HOME}/.bashrc"
}

@test "A26: err exits 1 and prints to stderr" {
  source "${HELPERS_DIR}/source_functions.bash"
  run bash -c 'source "'"${HELPERS_DIR}/source_functions.bash"'"; err "test_stage" "test message"'
  [ "$status" -eq 1 ]
  [[ "$output" == *"error: test message"* ]]
}

@test "A27: err with install prefix sends install_failed event" {
  source "${HELPERS_DIR}/source_functions.bash"
  export MOCK_CURL_TELEMETRY_LOG="$TELEMETRY_LOG"
  run bash -c '
    source "'"${HELPERS_DIR}/source_functions.bash"'"
    install_event_prefix="install"
    install_id="test-install-id"
    telemetry_id="test-device-id"
    err "test_stage" "test error message"
  '
  [ "$status" -eq 1 ]
  sleep 0.2
  [ -f "$TELEMETRY_LOG" ]
  grep -q "event_type=install_failed" "$TELEMETRY_LOG"
}

@test "A28: err with upgrade prefix sends upgrade_failed event" {
  source "${HELPERS_DIR}/source_functions.bash"
  export MOCK_CURL_TELEMETRY_LOG="$TELEMETRY_LOG"
  run bash -c '
    source "'"${HELPERS_DIR}/source_functions.bash"'"
    install_event_prefix="upgrade"
    install_id="test-install-id"
    telemetry_id="test-device-id"
    from_version="0.7.0"
    err "test_stage" "upgrade error"
  '
  [ "$status" -eq 1 ]
  sleep 0.2
  [ -f "$TELEMETRY_LOG" ]
  grep -q "event_type=upgrade_failed" "$TELEMETRY_LOG"
}

@test "A29: err without prefix does not send telemetry" {
  export MOCK_CURL_TELEMETRY_LOG="$TELEMETRY_LOG"
  run bash -c '
    source "'"${HELPERS_DIR}/source_functions.bash"'"
    err "test_stage" "no prefix error"
  '
  [ "$status" -eq 1 ]
  sleep 0.1
  [ ! -f "$TELEMETRY_LOG" ] || ! grep -q "event_type=" "$TELEMETRY_LOG"
}

# ===========================================================================
# Section B: Argument parsing
# ===========================================================================

@test "B1: --help prints usage and exits 0" {
  run_install_sh --help
  [ "$status" -eq 0 ]
  [[ "$stdout_output" == *"Usage: install.sh"* ]]
  [[ "$stdout_output" == *"--help"* ]]
}

@test "B2: -h prints usage and exits 0" {
  run_install_sh -h
  [ "$status" -eq 0 ]
  [[ "$stdout_output" == *"Usage: install.sh"* ]]
}

@test "B3: --no-cli is silently consumed" {
  run_install_sh --no-cli
  [ "$status" -eq 0 ]
  [[ "$stdout_output" == *"installed iii"* ]]
}

@test "B4: --cli-version with value is silently consumed" {
  run_install_sh --cli-version 1.0.0
  [ "$status" -eq 0 ]
  [[ "$stdout_output" == *"installed iii"* ]]
}

@test "B5: --cli-version at end of args is consumed" {
  run_install_sh --cli-version
  [ "$status" -eq 0 ]
  [[ "$stdout_output" == *"installed iii"* ]]
}

@test "B6: --cli-dir with value is silently consumed" {
  run_install_sh --cli-dir /tmp/fakedir
  [ "$status" -eq 0 ]
  [[ "$stdout_output" == *"installed iii"* ]]
}

@test "B7: --cli-dir at end of args is consumed" {
  run_install_sh --cli-dir
  [ "$status" -eq 0 ]
  [[ "$stdout_output" == *"installed iii"* ]]
}

@test "B8: unknown option errors with message" {
  unset VERSION
  run_install_sh --unknown-flag
  [ "$status" -eq 1 ]
  [[ "$stderr_output" == *"unknown option: --unknown-flag"* ]]
}

@test "B9: positional version arg sets VERSION" {
  unset VERSION
  run_install_sh 0.8.0
  [ "$status" -eq 0 ]
  [[ "$stdout_output" == *"installing version: 0.8.0"* ]]
}

@test "B10: VERSION env var is used" {
  export VERSION="0.8.0"
  run_install_sh
  [ "$status" -eq 0 ]
  [[ "$stdout_output" == *"installing version: 0.8.0"* ]]
}

# ===========================================================================
# Section C: Dependency check
# ===========================================================================

@test "C1: missing curl produces error" {
  hide_curl_from_path
  run_install_sh
  [ "$status" -eq 1 ]
  [[ "$stderr_output" == *"curl is required"* ]]
}

# ===========================================================================
# Section D: Platform detection
# ===========================================================================

@test "D1: TARGET env override skips uname" {
  export TARGET="custom-triple"
  # Remove uname mock to prove it's not called for target detection
  rm -f "${MOCK_BIN_DIR}/uname"
  # The asset won't match 'custom-triple' so it'll fail at asset lookup, proving TARGET was used
  run_install_sh
  [ "$status" -eq 1 ]
  [[ "$stderr_output" == *"no release asset found for target: custom-triple"* ]]
}

@test "D2: Darwin + x86_64 resolves to x86_64-apple-darwin" {
  unset TARGET
  create_mock_uname "Darwin" "x86_64"
  run_install_sh
  [ "$status" -eq 0 ]
}

@test "D3: Darwin + arm64 resolves to aarch64-apple-darwin" {
  unset TARGET
  create_mock_uname "Darwin" "arm64"
  setup_curl_download "${SANDBOX}/fake_archive/iii-aarch64-apple-darwin.tar.gz"
  run_install_sh
  [ "$status" -eq 0 ]
}

@test "D4: Linux + x86_64 without III_USE_GLIBC uses musl" {
  unset TARGET III_USE_GLIBC
  create_mock_uname "Linux" "x86_64"
  setup_curl_download "${SANDBOX}/fake_archive/iii-x86_64-unknown-linux-musl.tar.gz"
  run_install_sh
  [ "$status" -eq 0 ]
}

@test "D5: Linux + x86_64 with III_USE_GLIBC and sufficient glibc uses gnu" {
  unset TARGET
  export III_USE_GLIBC=1
  create_mock_uname "Linux" "x86_64"
  create_mock_ldd "2.36"
  setup_curl_download "${SANDBOX}/fake_archive/iii-x86_64-unknown-linux-gnu.tar.gz"
  run_install_sh
  [ "$status" -eq 0 ]
  [[ "$stdout_output" == *"using glibc build"* ]]
}

@test "D6: Linux + x86_64 with III_USE_GLIBC and old glibc falls back to musl" {
  unset TARGET
  export III_USE_GLIBC=1
  create_mock_uname "Linux" "x86_64"
  create_mock_ldd "2.31"
  setup_curl_download "${SANDBOX}/fake_archive/iii-x86_64-unknown-linux-musl.tar.gz"
  run_install_sh
  [ "$status" -eq 0 ]
  [[ "$stderr_output" == *"falling back to musl"* ]]
}

@test "D7: Linux + aarch64 resolves to aarch64-unknown-linux-gnu" {
  unset TARGET
  create_mock_uname "Linux" "aarch64"
  setup_curl_download "${SANDBOX}/fake_archive/iii-aarch64-unknown-linux-gnu.tar.gz"
  run_install_sh
  [ "$status" -eq 0 ]
}

@test "D8: Linux + armv7l resolves to armv7-unknown-linux-gnueabihf" {
  unset TARGET
  create_mock_uname "Linux" "armv7l"
  setup_curl_download "${SANDBOX}/fake_archive/iii-armv7-unknown-linux-gnueabihf.tar.gz"
  run_install_sh
  [ "$status" -eq 0 ]
}

@test "D9: unsupported architecture errors" {
  unset TARGET
  create_mock_uname "Linux" "s390x"
  run_install_sh
  [ "$status" -eq 1 ]
  [[ "$stderr_output" == *"unsupported architecture: s390x"* ]]
}

@test "D10: unsupported OS errors" {
  unset TARGET
  create_mock_uname "FreeBSD" "x86_64"
  run_install_sh
  [ "$status" -eq 1 ]
  [[ "$stderr_output" == *"unsupported OS: FreeBSD"* ]]
}

@test "D11: amd64 maps to x86_64" {
  unset TARGET
  create_mock_uname "Darwin" "amd64"
  run_install_sh
  [ "$status" -eq 0 ]
}

# ===========================================================================
# Section E: Version resolution
# ===========================================================================

@test "E1: specific version with iii/v tag found on first try" {
  export VERSION="0.8.0"
  run_install_sh
  [ "$status" -eq 0 ]
  [[ "$stdout_output" == *"installing version: 0.8.0"* ]]
}

@test "E2: specific version falls back to v tag when iii/v 404s" {
  export VERSION="0.8.0"
  # Make the iii/v tag URL fail, but v tag succeed
  local iii_tag_url="https://api.github.com/repos/iii-hq/iii/releases/tags/iii/v0.8.0"
  local v_tag_url="https://api.github.com/repos/iii-hq/iii/releases/tags/v0.8.0"
  # Remove default and set specific URL responses
  rm -f "${MOCK_CURL_RESPONSES}/_default"
  # The iii/v URL gets no fixture → 404
  setup_curl_api_response "$v_tag_url" "${FIXTURES_DIR}/release_vtag.json"
  run_install_sh
  [ "$status" -eq 0 ]
}

@test "E3: specific version fails when both tags 404" {
  export VERSION="99.99.99"
  rm -f "${MOCK_CURL_RESPONSES}/_default"
  export MOCK_CURL_API_EXIT=22
  run_install_sh
  [ "$status" -eq 1 ]
  [[ "$stderr_output" == *"release tag not found"* ]]
}

@test "E4: prerelease version with jq is rejected" {
  export VERSION="0.9.0-beta.1"
  if ! create_mock_jq 2>/dev/null; then
    skip "jq not available"
  fi
  setup_curl_api_response "*" "${FIXTURES_DIR}/release_prerelease.json"
  run_install_sh
  [ "$status" -eq 1 ]
  [[ "$stderr_output" == *"prerelease"* ]]
}

@test "E5: prerelease version without jq is rejected (no space)" {
  export VERSION="0.9.0-beta.1"
  hide_jq_from_path
  setup_curl_api_response "*" "${FIXTURES_DIR}/release_prerelease.json"
  run_install_sh
  [ "$status" -eq 1 ]
  [[ "$stderr_output" == *"prerelease"* ]]
}

@test "E6: latest version uses /releases/latest when tag is iii/v" {
  unset VERSION
  run_install_sh
  [ "$status" -eq 0 ]
  [[ "$stdout_output" == *"installing latest version"* ]]
}

@test "E7: latest version falls back to listing when /releases/latest has wrong prefix" {
  unset VERSION
  local latest_url="https://api.github.com/repos/iii-hq/iii/releases/latest"
  local list_url="https://api.github.com/repos/iii-hq/iii/releases?per_page=20"
  setup_curl_api_response "$latest_url" "${FIXTURES_DIR}/release_no_iii_prefix.json"
  setup_curl_api_response "$list_url" "${FIXTURES_DIR}/releases_list.json"
  # The list fallback will find iii/v0.8.0 and then fetch that tag
  setup_curl_api_response "*" "${FIXTURES_DIR}/release_single.json"
  run_install_sh
  [ "$status" -eq 0 ]
}

@test "E8: latest version falls back to listing when /releases/latest fails" {
  unset VERSION
  # Make /releases/latest fail (mock returns 22 when no fixture matches)
  rm -f "${MOCK_CURL_RESPONSES}/_default"
  local list_url="https://api.github.com/repos/iii-hq/iii/releases?per_page=20"
  setup_curl_api_response "$list_url" "${FIXTURES_DIR}/releases_list.json"
  # After finding the tag, it fetches the specific release
  local tag_url="https://api.github.com/repos/iii-hq/iii/releases/tags/iii/v0.8.0"
  setup_curl_api_response "$tag_url" "${FIXTURES_DIR}/release_single.json"
  run_install_sh
  [ "$status" -eq 0 ]
}

@test "E9: latest version with no stable release errors (no-jq path)" {
  unset VERSION
  hide_jq_from_path
  rm -f "${MOCK_CURL_RESPONSES}/_default"
  local latest_url="https://api.github.com/repos/iii-hq/iii/releases/latest"
  setup_curl_api_response "$latest_url" "${FIXTURES_DIR}/release_no_iii_prefix.json"
  # List returns only prereleases and non-iii tags
  local list_url="https://api.github.com/repos/iii-hq/iii/releases?per_page=20"
  # Create a fixture with only prereleases
  cat > "${SANDBOX}/all_prerelease.json" <<'JSON'
[{"tag_name":"iii/v0.9.0-beta.1","prerelease":true,"assets":[]},{"tag_name":"console/v1.0.0","prerelease":false,"assets":[]}]
JSON
  setup_curl_api_response "$list_url" "${SANDBOX}/all_prerelease.json"
  run_install_sh
  [ "$status" -eq 1 ]
  [[ "$stderr_output" == *"could not determine latest release"* ]]
}

@test "E10: latest version with no stable release errors (jq path)" {
  unset VERSION
  if ! create_mock_jq 2>/dev/null; then
    skip "jq not available"
  fi
  rm -f "${MOCK_CURL_RESPONSES}/_default"
  local latest_url="https://api.github.com/repos/iii-hq/iii/releases/latest"
  setup_curl_api_response "$latest_url" "${FIXTURES_DIR}/release_no_iii_prefix.json"
  local list_url="https://api.github.com/repos/iii-hq/iii/releases?per_page=20"
  cat > "${SANDBOX}/all_prerelease.json" <<'JSON'
[{"tag_name":"iii/v0.9.0-beta.1","prerelease":true,"assets":[]},{"tag_name":"console/v1.0.0","prerelease":false,"assets":[]}]
JSON
  setup_curl_api_response "$list_url" "${SANDBOX}/all_prerelease.json"
  run_install_sh
  [ "$status" -eq 1 ]
  [[ "$stderr_output" == *"no stable iii release found"* ]]
}

# ===========================================================================
# Section F: Asset URL extraction
# ===========================================================================

@test "F1: asset found for matching target (no-jq path)" {
  hide_jq_from_path
  run_install_sh
  [ "$status" -eq 0 ]
}

@test "F2: asset found for matching target (jq path)" {
  if ! create_mock_jq 2>/dev/null; then
    skip "jq not available"
  fi
  run_install_sh
  [ "$status" -eq 0 ]
}

@test "F3: no matching asset prints available assets and errors" {
  export TARGET="riscv64-unknown-linux-gnu"
  run_install_sh
  [ "$status" -eq 1 ]
  [[ "$stderr_output" == *"available assets:"* ]]
  [[ "$stderr_output" == *"no release asset found for target: riscv64-unknown-linux-gnu"* ]]
}

# ===========================================================================
# Section G: BIN_DIR resolution
# ===========================================================================

@test "G1: BIN_DIR env is used directly" {
  export BIN_DIR="${SANDBOX}/custom_bin"
  mkdir -p "$BIN_DIR"
  run_install_sh
  [ "$status" -eq 0 ]
  [ -f "${SANDBOX}/custom_bin/iii" ]
}

@test "G2: PREFIX env sets bin_dir to PREFIX/bin" {
  unset BIN_DIR
  export PREFIX="${SANDBOX}/prefix"
  run_install_sh
  [ "$status" -eq 0 ]
  [ -f "${SANDBOX}/prefix/bin/iii" ]
}

@test "G3: neither BIN_DIR nor PREFIX defaults to HOME/.local/bin" {
  unset BIN_DIR PREFIX
  run_install_sh
  [ "$status" -eq 0 ]
  [ -f "${HOME}/.local/bin/iii" ]
}

# ===========================================================================
# Section H: Upgrade vs fresh install detection
# ===========================================================================

@test "H1: existing binary triggers upgrade path with upgrade_started event" {
  setup_existing_binary "0.7.0"
  run_install_sh
  [ "$status" -eq 0 ]
  assert_telemetry_event "upgrade_started"
  assert_telemetry_event "upgrade_succeeded"
}

@test "H2: no existing binary triggers install path with install_started event" {
  remove_existing_binary
  run_install_sh
  [ "$status" -eq 0 ]
  assert_telemetry_event "install_started"
  assert_telemetry_event "install_succeeded"
}

@test "H3: upgrade_started payload contains from_version" {
  setup_existing_binary "0.7.0"
  run_install_sh
  [ "$status" -eq 0 ]
  assert_telemetry_payload_contains "upgrade_started" '"from_version":"0.7.0"'
}

@test "H4: install_started payload contains install_method" {
  remove_existing_binary
  run_install_sh
  [ "$status" -eq 0 ]
  assert_telemetry_payload_contains "install_started" '"install_method":"sh"'
}

# ===========================================================================
# Section I: Download and extraction
# ===========================================================================

@test "I1: tar.gz asset is extracted successfully" {
  run_install_sh
  [ "$status" -eq 0 ]
  [[ "$stdout_output" == *"installed iii"* ]]
}

@test "I2: zip asset with unzip is extracted successfully" {
  create_mock_unzip
  # Create a zip archive
  if ! command -v zip >/dev/null 2>&1; then
    skip "zip not available to create test fixture"
  fi
  local zip_fixture="${SANDBOX}/zip_fixture"
  mkdir -p "$zip_fixture"
  create_mock_iii_binary "${zip_fixture}/_staging" "0.8.0"
  (cd "${zip_fixture}/_staging" && zip -q "${zip_fixture}/iii-x86_64-apple-darwin.zip" iii)
  rm -rf "${zip_fixture}/_staging"
  setup_curl_download "${zip_fixture}/iii-x86_64-apple-darwin.zip"
  # Use a fixture that points to .zip
  cat > "${SANDBOX}/release_zip.json" <<'JSON'
{"tag_name":"iii/v0.8.0","prerelease":false,"assets":[{"name":"iii-x86_64-apple-darwin.zip","browser_download_url":"https://example.com/iii-x86_64-apple-darwin.zip"}]}
JSON
  setup_curl_api_response "*" "${SANDBOX}/release_zip.json"
  run_install_sh
  [ "$status" -eq 0 ]
}

@test "I3: zip without unzip installed errors" {
  remove_mock_unzip
  # Force PATH to not include any unzip
  cat > "${SANDBOX}/release_zip.json" <<'JSON'
{"tag_name":"iii/v0.8.0","prerelease":false,"assets":[{"name":"iii-x86_64-apple-darwin.zip","browser_download_url":"https://example.com/iii-x86_64-apple-darwin.zip"}]}
JSON
  setup_curl_api_response "*" "${SANDBOX}/release_zip.json"
  if command -v zip >/dev/null 2>&1; then
    local zip_fixture="${SANDBOX}/zip_fixture"
    mkdir -p "$zip_fixture"
    create_mock_iii_binary "${zip_fixture}/_staging" "0.8.0"
    (cd "${zip_fixture}/_staging" && zip -q "${zip_fixture}/iii-x86_64-apple-darwin.zip" iii)
    rm -rf "${zip_fixture}/_staging"
    setup_curl_download "${zip_fixture}/iii-x86_64-apple-darwin.zip"
  fi
  hide_unzip_from_path
  run_install_sh
  [ "$status" -eq 1 ]
  [[ "$stderr_output" == *"unzip is required"* ]]
}

@test "I4: curl download failure exits with error" {
  export MOCK_CURL_DOWNLOAD_EXIT=22
  run_install_sh
  [ "$status" -ne 0 ]
}

@test "I5: tar extraction failure exits with error" {
  export MOCK_TAR_EXIT=1
  run_install_sh
  [ "$status" -ne 0 ]
}

# ===========================================================================
# Section J: Binary lookup
# ===========================================================================

@test "J1: binary found at tmpdir root" {
  run_install_sh
  [ "$status" -eq 0 ]
  [ -f "${FAKE_BIN_DIR}/iii" ]
}

@test "J2: binary found in nested subdirectory via find" {
  create_fake_archive_nested "${SANDBOX}/nested_archive"
  setup_curl_download "${SANDBOX}/nested_archive/iii-x86_64-apple-darwin.tar.gz"
  run_install_sh
  [ "$status" -eq 0 ]
  [ -f "${FAKE_BIN_DIR}/iii" ]
}

@test "J3: binary not found in archive errors" {
  create_fake_archive_empty "${SANDBOX}/empty_archive"
  setup_curl_download "${SANDBOX}/empty_archive/iii-x86_64-apple-darwin.tar.gz"
  run_install_sh
  [ "$status" -eq 1 ]
  [[ "$stderr_output" == *"binary not found in downloaded asset"* ]]
}

# ===========================================================================
# Section K: Installation method
# ===========================================================================

@test "K1: install command is used when available" {
  create_mock_install_cmd
  run_install_sh
  [ "$status" -eq 0 ]
  [ -f "${FAKE_BIN_DIR}/iii" ]
  [ -x "${FAKE_BIN_DIR}/iii" ]
}

@test "K2: cp+chmod fallback when install command is missing" {
  remove_mock_install_cmd
  run_install_sh
  [ "$status" -eq 0 ]
  [ -f "${FAKE_BIN_DIR}/iii" ]
  [ -x "${FAKE_BIN_DIR}/iii" ]
}

@test "K3: installed binary reports correct version" {
  run_install_sh
  [ "$status" -eq 0 ]
  local result
  result=$("${FAKE_BIN_DIR}/iii" --version 2>/dev/null | awk '{print $NF}')
  [ "$result" = "0.8.0" ]
}

# ===========================================================================
# Section L: Telemetry lifecycle — full flow
# ===========================================================================

@test "L1: fresh install sends install_started then install_succeeded" {
  remove_existing_binary
  run_install_sh
  [ "$status" -eq 0 ]
  assert_telemetry_event "install_started"
  assert_telemetry_event "install_succeeded"
  refute_telemetry_event "upgrade_started"
  refute_telemetry_event "upgrade_succeeded"
}

@test "L2: upgrade sends upgrade_started then upgrade_succeeded" {
  setup_existing_binary "0.7.0"
  run_install_sh
  [ "$status" -eq 0 ]
  assert_telemetry_event "upgrade_started"
  assert_telemetry_event "upgrade_succeeded"
  refute_telemetry_event "install_started"
  refute_telemetry_event "install_succeeded"
}

@test "L3: install_succeeded payload contains installed_version" {
  remove_existing_binary
  run_install_sh
  [ "$status" -eq 0 ]
  assert_telemetry_payload_contains "install_succeeded" '"installed_version"'
}

@test "L4: upgrade_succeeded payload contains from_version and to_version" {
  setup_existing_binary "0.7.0"
  run_install_sh
  [ "$status" -eq 0 ]
  assert_telemetry_payload_contains "upgrade_succeeded" '"from_version":"0.7.0"'
  assert_telemetry_payload_contains "upgrade_succeeded" '"to_version"'
}

@test "L5: install_started payload contains os and arch" {
  remove_existing_binary
  run_install_sh
  [ "$status" -eq 0 ]
  assert_telemetry_payload_contains "install_started" '"os":'
  assert_telemetry_payload_contains "install_started" '"arch":'
}

@test "L6: fresh install download failure does not send install_succeeded" {
  remove_existing_binary
  export MOCK_CURL_DOWNLOAD_EXIT=22
  run_install_sh
  [ "$status" -ne 0 ]
  # install_started was sent before the download, so it should be present
  assert_telemetry_event "install_started"
  refute_telemetry_event "install_succeeded"
}

@test "L7: upgrade download failure does not send upgrade_succeeded" {
  setup_existing_binary "0.7.0"
  export MOCK_CURL_DOWNLOAD_EXIT=22
  run_install_sh
  [ "$status" -ne 0 ]
  assert_telemetry_event "upgrade_started"
  refute_telemetry_event "upgrade_succeeded"
}

@test "L8: telemetry events are skipped when III_TELEMETRY_ENABLED=false" {
  export III_TELEMETRY_ENABLED=false
  run_install_sh
  [ "$status" -eq 0 ]
  local count
  count=$(telemetry_event_count)
  [ "$count" -eq 0 ]
}

@test "L9: telemetry events are skipped when CI=true" {
  export CI=true
  run_install_sh
  [ "$status" -eq 0 ]
  local count
  count=$(telemetry_event_count)
  [ "$count" -eq 0 ]
}

@test "L10: all telemetry payloads contain install_id" {
  remove_existing_binary
  run_install_sh
  [ "$status" -eq 0 ]
  assert_telemetry_payload_contains "install_started" '"install_id"'
  assert_telemetry_payload_contains "install_succeeded" '"install_id"'
}

# ===========================================================================
# Section M: Post-install behavior
# ===========================================================================

@test "M1: iii_export_host_user_id is called (profile gets III_HOST_USER_ID)" {
  touch "${HOME}/.bashrc"
  run_install_sh
  [ "$status" -eq 0 ]
  grep -q "III_HOST_USER_ID" "${HOME}/.bashrc"
}

@test "M2: bin_dir in PATH suppresses 'add to PATH' message" {
  export PATH="${FAKE_BIN_DIR}:${PATH}"
  run_install_sh
  [ "$status" -eq 0 ]
  ! echo "$stdout_output" | grep -q "add .* to your PATH"
}

@test "M3: bin_dir not in PATH prints suggestion" {
  # FAKE_BIN_DIR is not in ORIGINAL_PATH, so the message should appear
  # Ensure BIN_DIR is set to something not in PATH
  export BIN_DIR="${SANDBOX}/not_in_path_bin"
  mkdir -p "$BIN_DIR"
  run_install_sh
  [ "$status" -eq 0 ]
  [[ "$stdout_output" == *"add"*"to your PATH"* ]]
}

@test "M4: quickstart URL is printed" {
  run_install_sh
  [ "$status" -eq 0 ]
  [[ "$stdout_output" == *"https://iii.dev/docs/quickstart"* ]]
}

@test "M5: 'installed iii to' message is printed" {
  run_install_sh
  [ "$status" -eq 0 ]
  [[ "$stdout_output" == *"installed iii to"* ]]
}

# ===========================================================================
# Section N: Error telemetry from deep failures
# ===========================================================================

@test "N1: binary_lookup failure sends error telemetry with install prefix" {
  remove_existing_binary
  create_fake_archive_empty "${SANDBOX}/bad_archive"
  setup_curl_download "${SANDBOX}/bad_archive/iii-x86_64-apple-darwin.tar.gz"
  run_install_sh
  [ "$status" -eq 1 ]
  [[ "$stderr_output" == *"binary not found"* ]]
  # install_started was sent, but install_failed should also appear
  assert_telemetry_event "install_started"
  assert_telemetry_event "install_failed"
}

@test "N2: binary_lookup failure sends error telemetry with upgrade prefix" {
  setup_existing_binary "0.7.0"
  create_fake_archive_empty "${SANDBOX}/bad_archive"
  setup_curl_download "${SANDBOX}/bad_archive/iii-x86_64-apple-darwin.tar.gz"
  run_install_sh
  [ "$status" -eq 1 ]
  assert_telemetry_event "upgrade_started"
  assert_telemetry_event "upgrade_failed"
}

@test "N3: err telemetry payload contains error_stage and error_message" {
  remove_existing_binary
  create_fake_archive_empty "${SANDBOX}/bad_archive"
  setup_curl_download "${SANDBOX}/bad_archive/iii-x86_64-apple-darwin.tar.gz"
  run_install_sh
  [ "$status" -eq 1 ]
  assert_telemetry_payload_contains "install_failed" '"error_stage"'
  assert_telemetry_payload_contains "install_failed" '"error_message"'
}

@test "N4: platform error sends no telemetry (install_event_prefix not yet set)" {
  unset TARGET
  create_mock_uname "FreeBSD" "x86_64"
  run_install_sh
  [ "$status" -eq 1 ]
  local count
  count=$(telemetry_event_count)
  # install_started hasn't been sent yet at this point in the script
  # but install_id and telemetry_id ARE set by then, and install_event_prefix is NOT
  # err() checks install_event_prefix — if empty, no telemetry
  [ "$count" -eq 0 ]
}
