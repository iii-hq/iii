#!/usr/bin/env bats
# Tests for engine/registry/install.sh, the iii installer the registry serves.
#
# The registry serves that file after a preamble it renders per version
# (`renderCustomInstallScript` in iii-hq/registry, api/src/lib/install-script.ts).
# fixtures/registry-preamble.sh is that preamble, rendered for all nine iii
# targets with placeholders: @VERSION@, @REQUESTED@, and @sha256:<target>@ for
# each digest. `publish` below fills them in for archives it builds, with
# file:// URLs, so the scripts run here exactly as the registry composes them
# (preamble, a blank line, the body) and need no network.
#
# When the registry changes its preamble, render the fixture again from it.

setup() {
  REPO_ROOT="$(cd "$BATS_TEST_DIRNAME/../.." && pwd)"
  BODY="$REPO_ROOT/engine/registry/install.sh"
  PREAMBLE="$BATS_TEST_DIRNAME/fixtures/registry-preamble.sh"
  ROOT="$BATS_TEST_TMPDIR"
  REGISTRY="$ROOT/registry"

  export HOME="$ROOT/home"
  export BIN_DIR="$ROOT/bin"
  export III_TELEMETRY_ENABLED=false
  unset VERSION TARGET PREFIX III_USE_GLIBC III_RELEASE_TAG III_NON_INTERACTIVE III_INSTALL_REDIRECTED
  mkdir -p "$HOME" "$ROOT/stub"

  # uname answers what each test sets in STUB_UNAME_S / STUB_UNAME_M.
  cat > "$ROOT/stub/uname" <<'SH'
#!/bin/sh
case "$1" in
  -s) echo "${STUB_UNAME_S:-Linux}" ;;
  -m) echo "${STUB_UNAME_M:-x86_64}" ;;
  *) echo "${STUB_UNAME_S:-Linux}" ;;
esac
SH
  chmod +x "$ROOT/stub/uname"
  # No iii from this machine: a fresh install is one that finds none on PATH.
  export PATH="$ROOT/stub:/usr/bin:/bin"
}

# A fake iii of version $1 at $2: prints its version, accepts the setup offer's
# probe, and logs every telemetry call to $ROOT/events.log.
fake_iii() {
  cat > "$2" <<SH
#!/bin/sh
case "\$1" in
  --version) echo "iii $1" ;;
  --install-only-generate-ids) [ \$# -gt 1 ] && echo "$1 \$*" >> "$ROOT/events.log" ;;
esac
exit 0
SH
  chmod +x "$2"
}

# An iii that cannot run on this machine, as a libc or architecture mismatch
# looks from a shell: no output, a failing status.
broken_iii() {
  printf '#!/bin/sh\nexit 126\n' > "$1"
  chmod +x "$1"
}

# publish VERSION REQUESTED TARGET...: builds VERSION's archive for each TARGET,
# then serves the registry's script for REQUESTED (a tag or the version) at
# $REGISTRY/install/iii/REQUESTED.sh, as GET /install/iii/REQUESTED.sh would.
# With BROKEN set, the archives hold an iii that does not run.
publish() {
  _version="$1"; _requested="$2"; shift 2
  _cdn="$ROOT/cdn/$_version"
  mkdir -p "$_cdn" "$REGISTRY/install/iii"
  _sums=""
  for _target in "$@"; do
    _stage="$ROOT/stage/$_version/$_target"
    mkdir -p "$_stage"
    if [ -n "${BROKEN:-}" ]; then broken_iii "$_stage/iii"; else fake_iii "$_version" "$_stage/iii"; fi
    tar -czf "$_cdn/iii-$_target.tar.gz" -C "$_stage" iii
    _sum=$(sha256sum "$_cdn/iii-$_target.tar.gz" | cut -d' ' -f1)
    _sums="$_sums -e s#@sha256:$_target@#$_sum#"
  done
  # shellcheck disable=SC2086
  sed -e "s#https://cdn.iii.cloud/workers/iii/@VERSION@/#file://$_cdn/#g" \
    -e "s#https://iii.dev/registry#file://$REGISTRY#g" \
    -e "s#@VERSION@#$_version#g" -e "s#@REQUESTED@#$_requested#g" \
    $_sums "$PREAMBLE" > "$REGISTRY/install/iii/$_requested.sh"
  { printf '\n'; cat "$BODY"; } >> "$REGISTRY/install/iii/$_requested.sh"
}

# run_install REQUESTED [ARGS...]: what `curl -fsSL .../install/iii/REQUESTED.sh | sh -s -- ARGS` runs.
run_install() {
  _requested="$1"; shift
  run sh "$REGISTRY/install/iii/$_requested.sh" "$@" </dev/null
}

# ─────────────────────────────────────────────────────────────
# Helpers, sourced in test mode
# ─────────────────────────────────────────────────────────────

helpers() {
  set +e
  III_INSTALL_SH_TEST_MODE=1 . "$BODY"
  set -e
}

@test "json_str escapes quotes, backslashes and newlines without jq" {
  helpers
  [ "$(json_str 'plain')" = '"plain"' ]
  [ "$(json_str 'say "hi"')" = '"say \"hi\""' ]
  [ "$(json_str 'a\b')" = '"a\\b"' ]
  [ "$(json_str "$(printf 'one\ntwo')")" = '"one\ntwo"' ]
  [ "$(json_str '')" = '""' ]
  [ "$(json_str "$(printf 'tab\there')")" = '"tab here"' ]
}

@test "registry_ref reads versions and GitHub release tags" {
  helpers
  [ "$(registry_ref 0.27.0)" = "0.27.0" ]
  [ "$(registry_ref v0.27.0)" = "0.27.0" ]
  [ "$(registry_ref iii/v0.27.0-rc.1)" = "0.27.0-rc.1" ]
  [ "$(registry_ref iii-alpha/v0.27.0-alpha.1)" = "0.27.0-alpha.1" ]
  [ "$(registry_ref rc)" = "rc" ]
}

# ─────────────────────────────────────────────────────────────
# The composed script
# ─────────────────────────────────────────────────────────────

@test "the body alone points at the registry and installs nothing" {
  run sh "$BODY" </dev/null
  [ "$status" -eq 1 ]
  [[ "$output" == *"curl -fsSL https://iii.dev/registry/install/iii/latest.sh | sh"* ]]
  [ ! -e "$BIN_DIR/iii" ]
}

@test "the composed script parses as sh" {
  publish 0.27.0 latest x86_64-unknown-linux-musl
  sh -n "$REGISTRY/install/iii/latest.sh"
}

@test "--help documents the registry URLs and names the pinned version" {
  publish 0.27.0 latest x86_64-unknown-linux-musl
  run_install latest --help
  [ "$status" -eq 0 ]
  [[ "$output" == *"https://iii.dev/registry/install/iii/rc.sh"* ]]
  [[ "$output" == *"--skip-bin-download"* ]]
  [[ "$output" == *"This script installs iii v0.27.0 (latest)."* ]]
  [[ "$output" != *"jq"* ]]
  [[ "$output" != *"GITHUB_TOKEN"* ]]
}

@test "installs the musl build on Linux x86_64 and checks its digest" {
  publish 0.27.0 latest x86_64-unknown-linux-gnu x86_64-unknown-linux-musl
  run_install latest
  [ "$status" -eq 0 ]
  [[ "$output" == *"installing iii v0.27.0 (latest)"* ]]
  [[ "$output" == *"downloading iii-x86_64-unknown-linux-musl.tar.gz..."* ]]
  [[ "$output" == *"installed iii v0.27.0 to $BIN_DIR/iii"* ]]
  [[ "$output" == *"iii project init --learn-iii"* ]]
  [ "$("$BIN_DIR/iii" --version)" = "iii 0.27.0" ]
}

@test "picks the gnu build on Linux aarch64 and the native build on a Mac" {
  publish 0.27.0 latest aarch64-unknown-linux-gnu aarch64-apple-darwin
  STUB_UNAME_M=aarch64 run_install latest
  [ "$status" -eq 0 ]
  [[ "$output" == *"downloading iii-aarch64-unknown-linux-gnu.tar.gz..."* ]]

  rm -rf "$BIN_DIR"
  STUB_UNAME_S=Darwin STUB_UNAME_M=arm64 run_install latest
  [ "$status" -eq 0 ]
  [[ "$output" == *"downloading iii-aarch64-apple-darwin.tar.gz..."* ]]
}

@test "a target the version has no build for lists the ones it has" {
  publish 0.27.0 latest x86_64-unknown-linux-musl
  TARGET=riscv64gc-unknown-linux-gnu run_install latest
  [ "$status" -eq 1 ]
  [[ "$output" == *"iii v0.27.0 does not include a prebuilt binary for riscv64gc-unknown-linux-gnu."* ]]
  [[ "$output" == *"  - x86_64-unknown-linux-musl"* ]]
  [[ "$output" == *"error: no release asset found for target: riscv64gc-unknown-linux-gnu"* ]]
  [ ! -e "$BIN_DIR/iii" ]
}

@test "an archive that does not match its pinned digest is not installed" {
  publish 0.27.0 latest x86_64-unknown-linux-musl
  printf 'tampered' >> "$ROOT/cdn/0.27.0/iii-x86_64-unknown-linux-musl.tar.gz"
  run_install latest
  [ "$status" -eq 1 ]
  [[ "$output" == *"error: checksum mismatch for x86_64-unknown-linux-musl"* ]]
  [[ "$output" == *"error: failed to download or verify iii-x86_64-unknown-linux-musl.tar.gz"* ]]
  [ ! -e "$BIN_DIR/iii" ]
}

@test "a failed upgrade is reported by the iii already installed" {
  publish 0.27.0 latest x86_64-unknown-linux-musl
  mkdir -p "$BIN_DIR"
  fake_iii 0.26.0 "$BIN_DIR/iii"
  printf 'tampered' >> "$ROOT/cdn/0.27.0/iii-x86_64-unknown-linux-musl.tar.gz"
  run_install latest
  [ "$status" -eq 1 ]
  grep -q '^0.26.0 --install-only-generate-ids --install-event-type upgrade_failed .*"from_version":"0.26.0","to_version":"0.27.0"' "$ROOT/events.log"
  [ "$("$BIN_DIR/iii" --version)" = "iii 0.26.0" ]
}

@test "an iii already at the version is left alone" {
  publish 0.27.0 latest x86_64-unknown-linux-musl
  mkdir -p "$BIN_DIR"
  fake_iii 0.27.0 "$BIN_DIR/iii"
  run_install latest
  [ "$status" -eq 0 ]
  [[ "$output" == *"iii is already at v0.27.0 — nothing to do"* ]]
  [[ "$output" != *"downloading"* ]]
}

@test "an upgrade says so and makes no setup offer" {
  publish 0.27.0 latest x86_64-unknown-linux-musl
  mkdir -p "$BIN_DIR"
  fake_iii 0.26.0 "$BIN_DIR/iii"
  run_install latest
  [ "$status" -eq 0 ]
  [[ "$output" == *"upgraded iii: v0.26.0 -> v0.27.0"* ]]
  [[ "$output" != *"project init"* ]]
  [ "$("$BIN_DIR/iii" --version)" = "iii 0.27.0" ]
}

@test "a build that does not run here never replaces a working engine" {
  BROKEN=1 publish 0.27.0 latest x86_64-unknown-linux-musl
  mkdir -p "$BIN_DIR"
  fake_iii 0.26.0 "$BIN_DIR/iii"
  run_install latest
  [ "$status" -eq 1 ]
  [[ "$output" == *"error: the downloaded iii v0.27.0 (x86_64-unknown-linux-musl) does not run on this system; $BIN_DIR/iii v0.26.0 is unchanged"* ]]
  [[ "$output" != *"upgraded"* ]]
  [ "$("$BIN_DIR/iii" --version)" = "iii 0.26.0" ]
  grep -q '^0.26.0 --install-only-generate-ids --install-event-type upgrade_failed .*"error_stage":"verify"' "$ROOT/events.log"
}

@test "a build that does not run here is not installed" {
  BROKEN=1 publish 0.27.0 latest x86_64-unknown-linux-musl
  run_install latest
  [ "$status" -eq 1 ]
  [[ "$output" == *"error: the downloaded iii v0.27.0 (x86_64-unknown-linux-musl) does not run on this system; nothing was installed"* ]]
  [ ! -e "$BIN_DIR/iii" ]
}

@test "an explicit TARGET installs another machine's build, with a warning" {
  BROKEN=1 publish 0.27.0 latest aarch64-unknown-linux-gnu
  TARGET=aarch64-unknown-linux-gnu run_install latest
  [ "$status" -eq 0 ]
  [[ "$output" == *"installed iii v0.27.0 to $BIN_DIR/iii"* ]]
  [[ "$output" == *"warning: iii installed but 'iii --version' produced no output: the aarch64-unknown-linux-gnu build may not run on this system"* ]]
  [ -x "$BIN_DIR/iii" ]
}

# ─────────────────────────────────────────────────────────────
# Another version than the one pinned
# ─────────────────────────────────────────────────────────────

@test "--rc runs the registry's script for the rc tag" {
  publish 0.27.0 latest x86_64-unknown-linux-musl
  publish 0.28.0-rc.1 rc x86_64-unknown-linux-musl
  run_install latest --rc
  [ "$status" -eq 0 ]
  [[ "$output" == *"installing iii rc from file://$REGISTRY"* ]]
  [[ "$output" == *"installing iii v0.28.0-rc.1 (rc)"* ]]
  [ "$("$BIN_DIR/iii" --version)" = "iii 0.28.0-rc.1" ]
}

@test "--rc on the rc script installs it without another download" {
  publish 0.28.0-rc.1 rc x86_64-unknown-linux-musl
  run_install rc --rc
  [ "$status" -eq 0 ]
  [[ "$output" != *"installing iii rc from"* ]]
  [ "$("$BIN_DIR/iii" --version)" = "iii 0.28.0-rc.1" ]
}

@test "VERSION, a positional version and III_RELEASE_TAG each pin that version" {
  publish 0.27.0 latest x86_64-unknown-linux-musl
  publish 0.26.0 0.26.0 x86_64-unknown-linux-musl
  publish 0.28.0-alpha.1 0.28.0-alpha.1 x86_64-unknown-linux-musl

  VERSION=v0.26.0 run_install latest
  [ "$status" -eq 0 ]
  [ "$("$BIN_DIR/iii" --version)" = "iii 0.26.0" ]

  rm -rf "$BIN_DIR"
  run_install latest iii/v0.26.0
  [ "$status" -eq 0 ]
  [ "$("$BIN_DIR/iii" --version)" = "iii 0.26.0" ]

  rm -rf "$BIN_DIR"
  III_RELEASE_TAG=iii-alpha/v0.28.0-alpha.1 run_install latest
  [ "$status" -eq 0 ]
  [ "$("$BIN_DIR/iii" --version)" = "iii 0.28.0-alpha.1" ]
}

@test "VERSION naming the pinned version installs it from this script" {
  publish 0.27.0 latest x86_64-unknown-linux-musl
  VERSION=0.27.0 run_install latest
  [ "$status" -eq 0 ]
  [[ "$output" != *"from file://"* ]]
  [ "$("$BIN_DIR/iii" --version)" = "iii 0.27.0" ]
}

@test "a version the registry does not have is an error" {
  publish 0.27.0 latest x86_64-unknown-linux-musl
  VERSION=9.9.9 run_install latest
  [ "$status" -eq 1 ]
  [[ "$output" == *"error: iii 9.9.9 is not in the registry (file://$REGISTRY/install/iii/9.9.9.sh)"* ]]
  [ ! -e "$BIN_DIR/iii" ]
}

@test "--next and --rc together are refused" {
  publish 0.27.0 latest x86_64-unknown-linux-musl
  run_install latest --next --rc
  [ "$status" -eq 1 ]
  [[ "$output" == *"--next and --rc cannot be used together"* ]]
}

@test "an unknown option is refused" {
  publish 0.27.0 latest x86_64-unknown-linux-musl
  run_install latest --bogus
  [ "$status" -eq 1 ]
  [[ "$output" == *"unknown option: --bogus"* ]]
}

# ─────────────────────────────────────────────────────────────
# --skip-bin-download
# ─────────────────────────────────────────────────────────────

@test "--skip-bin-download downloads nothing and offers the setup" {
  publish 0.27.0 latest x86_64-unknown-linux-musl
  mkdir -p "$BIN_DIR"
  fake_iii 0.26.0 "$BIN_DIR/iii"
  run_install latest --skip-bin-download --rc
  [ "$status" -eq 0 ]
  [[ "$output" != *"downloading"* ]]
  [[ "$output" != *"installing"* ]]
  [[ "$output" == *"iii project init --learn-iii"* ]]
  [ "$("$BIN_DIR/iii" --version)" = "iii 0.26.0" ]
}

@test "--skip-bin-download needs no build for this machine" {
  # An iii built here, for a platform the registry has no build for.
  publish 0.27.0 latest x86_64-unknown-linux-musl
  mkdir -p "$BIN_DIR"
  fake_iii 0.28.0-dev "$BIN_DIR/iii"
  STUB_UNAME_M=riscv64 run_install latest --skip-bin-download
  [ "$status" -eq 0 ]
  [[ "$output" != *"unsupported architecture"* ]]
  [[ "$output" == *"iii project init --learn-iii"* ]]

  STUB_UNAME_S=FreeBSD run_install latest --skip-bin-download
  [ "$status" -eq 0 ]
  [[ "$output" != *"unsupported OS"* ]]
}
