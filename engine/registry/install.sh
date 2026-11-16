#!/bin/sh
# shellcheck shell=sh
#
# The iii engine's installer on the iii registry:
#
#   curl -fsSL https://iii.dev/registry/install/iii/latest.sh | sh
#
# Every iii release publishes this file with its binaries (`registry publish
# --install-script`, see .github/workflows/_publish-engine-registry.yml). The
# registry serves it after a preamble that pins that version's archives and
# defines, for this script:
#
#   III_REGISTRY_URL        the registry site, e.g. https://iii.dev/registry
#   III_REGISTRY_NAME       iii
#   III_REGISTRY_VERSION    the version this script installs
#   III_REGISTRY_REQUESTED  the tag or version in the URL (latest, rc, 0.27.0)
#   III_REGISTRY_TARGETS    the targets that version has builds for
#   iii_registry_artifact <target>       the pinned archive of a target, or 1
#   iii_registry_detect_target           unused here: the target rules below are iii's own
#   iii_registry_fetch <target> <file>   download it and check its SHA-256, or 1
#
# See https://github.com/iii-hq/registry/blob/main/docs/EXECUTABLES.md#custom-installers
# The tests compose it with engine/tests/fixtures/registry-preamble.sh.
set -eu

BIN_NAME="iii"

telemetry_emitter=""
install_event_prefix=""
from_version=""
release_version="${III_REGISTRY_VERSION:-}"

# curl wrapper that retries transient network failures (504/502/503/timeouts).
# Uses only flags available since curl 7.12.3 (2005); avoids --retry-all-errors
# (7.71) so the script stays portable to old curl. HTTP 5xx is treated as a
# transient error by --retry across all curl versions.
curl_retry() {
  curl --retry 5 --retry-delay 2 --retry-max-time 120 --connect-timeout 10 "$@"
}

# Downloads the URL $1 to the file $2 with curl or wget.
fetch_url() {
  if command -v curl >/dev/null 2>&1; then
    curl_retry -fsSL "$1" -o "$2"
  elif command -v wget >/dev/null 2>&1; then
    wget -qO "$2" "$1"
  else
    return 1
  fi
}

# ---------------------------------------------------------------------------
# Output helpers
# ---------------------------------------------------------------------------

info() {
  printf '%s\n' "$*"
}

warn() {
  printf 'warning: %s\n' "$*" >&2
}

# $1 as a JSON string, for the telemetry payloads. They carry versions, names
# and this script's own error messages, so a control character other than a
# newline becomes a space rather than an escape.
json_str() {
  printf '%s' "$1" \
    | tr '\001-\011\013-\037' ' ' \
    | sed -e 's/\\/\\\\/g' -e 's/"/\\"/g' \
    | awk 'BEGIN { printf "%s", "\"" } NR > 1 { printf "%s", "\\n" } { printf "%s", $0 } END { printf "%s", "\"" }'
}

pkg_manager_hint() {
  _pkg="$1"
  if command -v brew >/dev/null 2>&1; then
    printf 'brew install %s' "$_pkg"
  elif command -v apt-get >/dev/null 2>&1; then
    printf 'apt-get install -y %s' "$_pkg"
  elif command -v dnf >/dev/null 2>&1; then
    printf 'dnf install -y %s' "$_pkg"
  elif command -v yum >/dev/null 2>&1; then
    printf 'yum install -y %s' "$_pkg"
  elif command -v apk >/dev/null 2>&1; then
    printf 'apk add %s' "$_pkg"
  elif command -v pacman >/dev/null 2>&1; then
    printf 'pacman -S --noconfirm %s' "$_pkg"
  else
    printf 'install %s via your package manager' "$_pkg"
  fi
}

err() {
  _stage="$1"; shift
  printf 'error: %s\n' "$*" >&2
  if [ -n "${install_event_prefix:-}" ]; then
    if [ "$install_event_prefix" = "upgrade" ]; then
      payload=$(printf '{"from_version":%s,"to_version":%s,"install_method":"sh","target_binary":%s,"error_stage":%s,"error_message":%s}' \
        "$(json_str "${from_version:-}")" "$(json_str "$release_version")" "$(json_str "$BIN_NAME")" "$(json_str "$_stage")" "$(json_str "$*")")
      iii_emit_event "upgrade_failed" "$payload"
    else
      payload=$(printf '{"install_method":"sh","target_binary":%s,"error_stage":%s,"error_message":%s}' \
        "$(json_str "$BIN_NAME")" "$(json_str "$_stage")" "$(json_str "$*")")
      iii_emit_event "install_failed" "$payload"
    fi
  fi
  exit 1
}

# ---------------------------------------------------------------------------
# Install helpers
# ---------------------------------------------------------------------------

# Install a binary file to a destination with mode 755.
install_bin() {
  _src="$1"; _dst="$2"
  if command -v install >/dev/null 2>&1; then
    install -m 755 "$_src" "$_dst"
  else
    cp "$_src" "$_dst" && chmod 755 "$_dst"
  fi
}

# Detect currently installed version of a binary (empty if not installed).
iii_detect_from_version() {
  _bin="$1"
  if [ -x "$_bin" ]; then
    "$_bin" --version 2>/dev/null | awk '{print $NF}' || echo ""
  elif command -v "$_bin" >/dev/null 2>&1; then
    "$_bin" --version 2>/dev/null | awk '{print $NF}' || echo ""
  else
    echo ""
  fi
}

# Is a working iii already on this machine, in bin_dir ($1) or anywhere on
# PATH? Working means `iii --version` succeeds.
iii_already_installed() {
  "$1/$BIN_NAME" --version >/dev/null 2>&1 || "$BIN_NAME" --version >/dev/null 2>&1 || return 1
}

# The registry ref (a version or a tag) that a VERSION or III_RELEASE_TAG value
# names: `iii/v0.27.0`, `v0.27.0` and `0.27.0` all name 0.27.0, and the GitHub
# tag `iii-alpha/v0.27.0-alpha.1` names 0.27.0-alpha.1.
registry_ref() {
  _ref="${1##*/}"
  printf '%s' "${_ref#v}"
}

# ---------------------------------------------------------------------------
# Telemetry
# ---------------------------------------------------------------------------

iii_emit_event() {
  _event_type="$1"
  _event_props="$2"
  if [ -z "${telemetry_emitter:-}" ] || [ ! -x "$telemetry_emitter" ]; then
    return 0
  fi
  "$telemetry_emitter" \
    --install-only-generate-ids \
    --install-event-type "$_event_type" \
    --install-event-properties "$_event_props" \
    >/dev/null 2>&1 || true
}

# The success path's events go out while the install carries on: the binary
# sends each on its own and gives up after 3 s, and nothing here waits for the
# network. The telemetry IDs exist before the first one (see below), so these
# sends only read them and race nothing. A non-interactive shell runs them
# with SIGINT ignored, so a Ctrl+C at the next prompt does not cut them short.
iii_emit_event_in_background() {
  iii_emit_event "$1" "$2" &
}

# Removes the download directory. Defined out here, not beside the `mktemp`
# that fills `tmpdir`, because the harness setup calls it before its `exec`,
# and with --skip-bin-download there is no download directory for it to name:
# an unset `tmpdir` is a run with nothing to clean, not an error.
cleanup() { rm -rf "${tmpdir:-}"; }

# Test-mode hook: when this var is set, stop here so unit tests can source
# the helper functions above without running the installer.
if [ -n "${III_INSTALL_SH_TEST_MODE:-}" ]; then
  # When sourced, `return` works. When executed directly (rare, for debugging),
  # `return` fails outside a function and the fallback `exit` runs.
  # shellcheck disable=SC2317
  return 0 2>/dev/null || exit 0
fi

# Without the registry's preamble there is nothing to install.
if [ -z "$release_version" ] || ! command -v iii_registry_fetch >/dev/null 2>&1; then
  printf 'error: %s\n' "this is the body of the registry's iii installer; run it from the registry:" >&2
  printf '  %s\n' "curl -fsSL https://iii.dev/registry/install/iii/latest.sh | sh" >&2
  exit 1
fi

# ---------------------------------------------------------------------------
# Argument parsing
# ---------------------------------------------------------------------------

engine_version="${VERSION:-}"
use_next=false
use_rc=false
skip_bin_download=false
# Either form works: the flag, or a non-empty III_NON_INTERACTIVE.
non_interactive=false
non_interactive_flag=false
if [ -n "${III_NON_INTERACTIVE:-}" ]; then
  non_interactive=true
fi

while [ $# -gt 0 ]; do
  case "$1" in
    # Deprecated no-op flags (kept for one release so stale docs don't break).
    # Scheduled for removal in the next minor version.
    --no-cli)
      warn "--no-cli is deprecated and has no effect (scheduled for removal next minor)"
      shift
      ;;
    --cli-version)
      warn "--cli-version is deprecated and has no effect (scheduled for removal next minor)"
      if [ $# -ge 2 ] && case "$2" in -*) false;; *) true;; esac; then shift 2; else shift; fi
      ;;
    --cli-dir)
      warn "--cli-dir is deprecated and has no effect (scheduled for removal next minor)"
      if [ $# -ge 2 ] && case "$2" in -*) false;; *) true;; esac; then shift 2; else shift; fi
      ;;
    --next)
      use_next=true
      shift
      ;;
    --rc)
      use_rc=true
      shift
      ;;
    --skip-bin-download)
      skip_bin_download=true
      shift
      ;;
    --non-interactive)
      non_interactive=true
      non_interactive_flag=true
      shift
      ;;
    -h|--help)
      cat <<'USAGE'
Usage: install.sh [OPTIONS] [VERSION]

Install the iii engine (includes CLI commands) from the iii registry.

Options:
  -h, --help            Show this help message
  --next                Install the latest "next" pre-release
  --rc                  Install the latest release candidate
  --skip-bin-download   Skip the download and install entirely, and run the
                        setup offer against the iii already on this machine.
  --non-interactive     Never wait for input, even when a terminal is
                        attached: install only, and print the setup command
                        instead of running it.
                        A run with no terminal already behaves this way.
                        Same as setting III_NON_INTERACTIVE.

Environment variables:
  VERSION               Engine version to install (e.g., 0.27.0)
  BIN_DIR               Engine binary installation directory
                        (default: $PREFIX/bin if set, else $HOME/.local/bin)
  PREFIX                Installation prefix (default: $HOME/.local)
  TARGET                Override target triple (e.g., x86_64-apple-darwin,
                        aarch64-unknown-linux-gnu)
  III_USE_GLIBC         Use glibc build on Linux x86_64 (any non-empty value
                        enables; default: musl)
  III_NON_INTERACTIVE   Any non-empty value does the same as
                        --non-interactive.
  III_RELEASE_TAG       Install the version of a GitHub release tag
                        (e.g., iii-alpha/v0.27.0-alpha.1)

Requires: curl or wget, sha256sum or shasum, tar (unzip for a .zip build)

The URL picks the version: latest, a release tag (rc, next, alpha) or an
exact version. Archives are checked against the SHA-256 the registry pinned.

Examples:
  curl -fsSL https://iii.dev/registry/install/iii/latest.sh | sh
  curl -fsSL https://iii.dev/registry/install/iii/rc.sh | sh
  curl -fsSL https://iii.dev/registry/install/iii/alpha.sh | sh
  curl -fsSL https://iii.dev/registry/install/iii/0.27.0.sh | sh
  curl -fsSL https://iii.dev/registry/install/iii/latest.sh | sh -s -- --non-interactive
  curl -fsSL https://iii.dev/registry/install/iii/latest.sh | III_NON_INTERACTIVE=1 sh
  curl -fsSL https://iii.dev/registry/install/iii/latest.sh | BIN_DIR=/usr/local/bin sh
USAGE
      printf '\nThis script installs %s v%s (%s).\n' "$BIN_NAME" "$III_REGISTRY_VERSION" "$III_REGISTRY_REQUESTED"
      exit 0
      ;;
    -*)
      err "args" "unknown option: $1 (use --help for usage)"
      ;;
    *)
      if [ -z "$engine_version" ]; then
        engine_version="$1"
      fi
      shift
      ;;
  esac
done

if [ "$use_next" = "true" ] && [ "$use_rc" = "true" ]; then
  err "args" "--next and --rc cannot be used together"
fi

# ---------------------------------------------------------------------------
# Release selection
# ---------------------------------------------------------------------------

# The registry pinned one version in this script. Asking for another one
# (VERSION, a positional version, III_RELEASE_TAG, --rc, --next) runs the
# registry's script for it instead, with the same options. That script pins
# what was asked for, so it never goes on to a third.
requested_ref=""
if [ -n "${III_RELEASE_TAG:-}" ]; then
  requested_ref=$(registry_ref "$III_RELEASE_TAG")
elif [ -n "$engine_version" ]; then
  requested_ref=$(registry_ref "$engine_version")
elif [ "$use_rc" = "true" ]; then
  requested_ref="rc"
elif [ "$use_next" = "true" ]; then
  requested_ref="next"
fi

if [ "$skip_bin_download" = false ] && [ -n "$requested_ref" ] \
  && [ "$requested_ref" != "$III_REGISTRY_VERSION" ] && [ "$requested_ref" != "$III_REGISTRY_REQUESTED" ]; then
  if [ -n "${III_INSTALL_REDIRECTED:-}" ]; then
    err "download" "the registry served $BIN_NAME $III_REGISTRY_VERSION for $requested_ref"
  fi
  _url="$III_REGISTRY_URL/install/$III_REGISTRY_NAME/$requested_ref.sh"
  info "installing $BIN_NAME $requested_ref from $III_REGISTRY_URL"
  _script=$(mktemp 2>/dev/null || mktemp -t iii-install)
  if ! fetch_url "$_url" "$_script"; then
    rm -f "$_script"
    err "download" "$BIN_NAME $requested_ref is not in the registry ($_url)"
  fi
  set --
  if [ "$non_interactive_flag" = true ]; then
    set -- --non-interactive
  fi
  # stdin is still this script under `curl ... | sh`, so the other one gets none:
  # its setup offer reads the terminal from /dev/tty.
  _status=0
  III_INSTALL_REDIRECTED=1 VERSION="" III_RELEASE_TAG="" sh "$_script" "$@" </dev/null || _status=$?
  rm -f "$_script"
  exit "$_status"
fi

# ---------------------------------------------------------------------------
# Dependency checks
# ---------------------------------------------------------------------------

# Only the download needs them.
if [ "$skip_bin_download" = false ]; then
  if ! command -v curl >/dev/null 2>&1 && ! command -v wget >/dev/null 2>&1; then
    err "dependency" "curl or wget is required ($(pkg_manager_hint curl))"
  fi
  if ! command -v sha256sum >/dev/null 2>&1 && ! command -v shasum >/dev/null 2>&1; then
    err "dependency" "sha256sum or shasum is required to verify the download ($(pkg_manager_hint coreutils))"
  fi
fi

# ---------------------------------------------------------------------------
# Platform / target detection
# ---------------------------------------------------------------------------

uname_s=$(uname -s 2>/dev/null || echo unknown)
uname_m=$(uname -m 2>/dev/null || echo unknown)

if [ -n "${TARGET:-}" ]; then
  target="$TARGET"
else
  case "$uname_m" in
    x86_64|amd64) arch="x86_64" ;;
    arm64|aarch64) arch="aarch64" ;;
    armv7*) arch="armv7" ;;
    *)
      err "platform" "unsupported architecture: $uname_m (set TARGET=<triple> to override; builds of $BIN_NAME $release_version: $III_REGISTRY_TARGETS)"
      ;;
  esac

  case "$uname_s" in
    Darwin)
      os="apple-darwin"
      ;;
    Linux)
      case "$arch" in
        x86_64)
          if [ -n "${III_USE_GLIBC:-}" ]; then
            sys_glibc=$(ldd --version 2>&1 | head -n 1 | grep -oE '[0-9]+\.[0-9]+$' || echo "0.0")
            required_glibc="2.35"
            if printf '%s\n%s\n' "$required_glibc" "$sys_glibc" | sort -V -C; then
              os="unknown-linux-gnu"
              info "using glibc build (system glibc: $sys_glibc)"
            else
              warn "system glibc $sys_glibc is older than required $required_glibc, falling back to musl"
              os="unknown-linux-musl"
            fi
          else
            os="unknown-linux-musl"
          fi
          ;;
        aarch64)
          os="unknown-linux-gnu"
          ;;
        armv7)
          os="unknown-linux-gnueabihf"
          ;;
      esac
      ;;
    *)
      err "platform" "unsupported OS: $uname_s (set TARGET=<triple> to override; builds of $BIN_NAME $release_version: $III_REGISTRY_TARGETS)"
      ;;
  esac

  target="$arch-$os"
fi

# ---------------------------------------------------------------------------
# Pick the archive for our target
# ---------------------------------------------------------------------------

# Skipped with --skip-bin-download: nothing is fetched.
if [ "$skip_bin_download" = false ]; then
  if [ "$III_REGISTRY_REQUESTED" = "$release_version" ]; then
    info "installing $BIN_NAME v$release_version"
  else
    info "installing $BIN_NAME v$release_version ($III_REGISTRY_REQUESTED)"
  fi

  if ! iii_registry_artifact "$target"; then
    {
      echo ""
      echo "$BIN_NAME v$release_version does not include a prebuilt binary for $target."
      if [ -n "$III_REGISTRY_TARGETS" ]; then
        echo ""
        echo "Available $BIN_NAME targets in v$release_version:"
        for _t in $III_REGISTRY_TARGETS; do
          printf '  - %s\n' "$_t"
        done
      fi
      echo ""
      echo "Try:"
      echo "  - Install a stable release:  curl -fsSL $III_REGISTRY_URL/install/$III_REGISTRY_NAME/latest.sh | sh"
      echo "  - Pin a different version:   curl -fsSL $III_REGISTRY_URL/install/$III_REGISTRY_NAME/<x.y.z>.sh | sh"
      echo "  - Override target triple:    curl -fsSL $III_REGISTRY_URL/install/$III_REGISTRY_NAME/latest.sh | TARGET=<triple> sh"
    } >&2
    err "download" "no release asset found for target: $target"
  fi
  asset_name=$(basename "$III_REGISTRY_ARTIFACT_URL")
fi

# ---------------------------------------------------------------------------
# Resolve install directory and detect upgrade vs fresh install
# ---------------------------------------------------------------------------

if [ -z "${BIN_DIR:-}" ]; then
  if [ -n "${PREFIX:-}" ]; then
    bin_dir="$PREFIX/bin"
  else
    bin_dir="$HOME/.local/bin"
  fi
else
  bin_dir="$BIN_DIR"
fi

from_version=$(iii_detect_from_version "$bin_dir/$BIN_NAME")

# Checked before the install: an upgrade gets no setup offer.
had_iii=false
if iii_already_installed "$bin_dir"; then
  had_iii=true
fi
if [ -n "$from_version" ]; then
  install_event_prefix="upgrade"
  if [ -x "$bin_dir/$BIN_NAME" ]; then
    telemetry_emitter="$bin_dir/$BIN_NAME"
  fi
else
  install_event_prefix="install"
fi

# Everything that downloads, installs, or reports on an install. With
# --skip-bin-download the binary already on this machine is the one the onboarding
# offer below runs.
if [ "$skip_bin_download" = false ]; then
# Idempotency: if already at target version, skip the download and move on.
# Nothing was installed, so nothing is announced: the harness offer and the
# quickstart link below are for someone who has just arrived, not for a
# re-run that changed nothing.
if [ -n "$from_version" ] && [ "$from_version" = "$release_version" ]; then
  info "$BIN_NAME is already at v$release_version — nothing to do"
  exit 0
fi

mkdir -p "$bin_dir"

tmpdir=$(mktemp -d 2>/dev/null || mktemp -d -t iii-install)
trap cleanup EXIT INT TERM

main_tarball="$tmpdir/$asset_name"

# ---------------------------------------------------------------------------
# Download, and check the SHA-256 the registry pinned
# ---------------------------------------------------------------------------

printf 'downloading %s...\n' "$asset_name"
iii_registry_fetch "$target" "$main_tarball" \
  || err "download" "failed to download or verify $asset_name"

# ---------------------------------------------------------------------------
# Extract main binary
# ---------------------------------------------------------------------------

case "$III_REGISTRY_ARTIFACT_FORMAT" in
  tar.gz)
    tar -xzf "$main_tarball" -C "$tmpdir" \
      || err "extract" "failed to extract $asset_name"
    ;;
  zip)
    if ! command -v unzip >/dev/null 2>&1; then
      err "extract" "unzip is required to extract $asset_name ($(pkg_manager_hint unzip))"
    fi
    unzip -q "$main_tarball" -d "$tmpdir" \
      || err "extract" "failed to extract $asset_name"
    ;;
  *)
    err "extract" "cannot install a $III_REGISTRY_ARTIFACT_FORMAT build ($asset_name)"
    ;;
esac

if [ -f "$tmpdir/$BIN_NAME" ]; then
  bin_file="$tmpdir/$BIN_NAME"
else
  bin_file=$(find "$tmpdir" -type f \( -name "$BIN_NAME" -o -name "${BIN_NAME}.exe" \) 2>/dev/null | head -n 1)
fi

if [ -z "${bin_file:-}" ] || [ ! -f "$bin_file" ]; then
  err "binary_lookup" "binary not found in downloaded asset"
fi

# ---------------------------------------------------------------------------
# Install: main binary
# ---------------------------------------------------------------------------

telemetry_emitter="$bin_file"
# Best-effort: have the binary initialize its telemetry IDs, in the foreground
# and with no network, before any event is sent beside the install.
# Older binaries won't have this flag — silently skip.
"$bin_file" --install-only-generate-ids >/dev/null 2>&1 || true
if [ "$install_event_prefix" = "upgrade" ]; then
  payload=$(printf '{"from_version":%s,"to_version":%s,"install_method":"sh","target_binary":%s}' \
    "$(json_str "$from_version")" "$(json_str "$release_version")" "$(json_str "$BIN_NAME")")
  iii_emit_event_in_background "upgrade_started" "$payload"
else
  payload=$(printf '{"install_method":"sh","target_binary":%s,"os":%s,"arch":%s}' \
    "$(json_str "$BIN_NAME")" \
    "$(json_str "$(echo "$uname_s" | tr '[:upper:]' '[:lower:]')")" \
    "$(json_str "$uname_m")")
  iii_emit_event_in_background "install_started" "$payload"
fi

install_bin "$bin_file" "$bin_dir/$BIN_NAME" \
  || err "install" "failed to install binary to $bin_dir/$BIN_NAME"

installed_version=$("$bin_dir/$BIN_NAME" --version 2>/dev/null | awk '{print $NF}' || echo "")

if [ "$install_event_prefix" = "upgrade" ]; then
  printf 'upgraded %s: v%s -> v%s\n' "$BIN_NAME" "$from_version" "${installed_version:-$release_version}"
else
  printf 'installed %s v%s to %s\n' "$BIN_NAME" "${installed_version:-$release_version}" "$bin_dir/$BIN_NAME"
fi

# ---------------------------------------------------------------------------
# Telemetry: success
# ---------------------------------------------------------------------------

# Sent from the installed binary: the one in $tmpdir goes with the EXIT trap,
# maybe before a job started this late has run it. A `*_started` job was
# started before the copy, and the only early exit after it (`err`) first
# runs a synchronous event from that same binary.
telemetry_emitter="$bin_dir/$BIN_NAME"
if [ "$install_event_prefix" = "upgrade" ]; then
  payload=$(printf '{"from_version":%s,"to_version":%s,"install_method":"sh","target_binary":%s}' \
    "$(json_str "$from_version")" "$(json_str "$installed_version")" "$(json_str "$BIN_NAME")")
  iii_emit_event_in_background "upgrade_succeeded" "$payload"
else
  payload=$(printf '{"installed_version":%s,"install_method":"sh","target_binary":%s}' \
    "$(json_str "$installed_version")" "$(json_str "$BIN_NAME")")
  iii_emit_event_in_background "install_succeeded" "$payload"
fi

# ---------------------------------------------------------------------------
# Post-install verification
# ---------------------------------------------------------------------------

if [ -z "$installed_version" ]; then
  warn "$BIN_NAME installed but 'iii --version' produced no output — the binary may not run on this system (Gatekeeper? libc mismatch? try: $bin_dir/$BIN_NAME --version)"
fi
fi

# ---------------------------------------------------------------------------
# PATH guidance
# ---------------------------------------------------------------------------

case ":$PATH:" in
  *":$bin_dir:"*)
    # bin_dir already in PATH — nothing to do
    ;;
  *)
    _shell_name=$(basename "${SHELL:-sh}")
    # The $PATH references below are meant to stay literal so the user's shell
    # rc file expands them at startup. shellcheck is correct that they don't
    # expand here — that's the point.
    # shellcheck disable=SC2016
    case "$_shell_name" in
      bash)
        if [ "$uname_s" = "Darwin" ]; then _rc="$HOME/.bash_profile"; else _rc="$HOME/.bashrc"; fi
        echo ""
        echo "add $bin_dir to your PATH. For bash:"
        echo "  echo 'export PATH=\"$bin_dir:\$PATH\"' >> $_rc"
        echo "  source $_rc"
        ;;
      zsh)
        _rc="${ZDOTDIR:-$HOME}/.zshrc"
        echo ""
        echo "add $bin_dir to your PATH. For zsh:"
        echo "  echo 'export PATH=\"$bin_dir:\$PATH\"' >> $_rc"
        echo "  source $_rc"
        ;;
      fish)
        echo ""
        echo "add $bin_dir to your PATH. For fish:"
        echo "  fish_add_path $bin_dir"
        ;;
      *)
        echo ""
        echo "add $bin_dir to your PATH. For example:"
        echo "  export PATH=\"$bin_dir:\$PATH\""
        ;;
    esac
    ;;
esac

# ---------------------------------------------------------------------------
# Onboarding: start the harness.
# stdin is the script itself under `curl ... | sh`, so wait for Enter on
# /dev/tty. Skip silently when no terminal is attached (CI, Dockerfiles).
# ---------------------------------------------------------------------------

# An upgrade already has iii set up, so it gets no setup offer.
# --skip-bin-download asks for the offer against the iii already here.
if [ "$had_iii" = true ] && [ "$skip_bin_download" = false ]; then
  exit 0
fi

start_cmd="$BIN_NAME project init --learn-iii"
quickstart_url="https://iii.dev/docs/quickstart"

# The binary the setup runs: the one in bin_dir, or with --skip-bin-download
# and none there, the one on PATH.
iii_bin="$bin_dir/$BIN_NAME"
if [ "$skip_bin_download" = true ] && [ ! -x "$iii_bin" ]; then
  iii_bin=$(command -v "$BIN_NAME" 2>/dev/null) || iii_bin="$bin_dir/$BIN_NAME"
fi

# Does the binary we just installed know `--learn-iii`? Ask the parser rather
# than reading the help text: the help is a rendered table whose column widths
# follow the longest flag, so whether a given flag survives as one string is a
# property of the other flags beside it. `--help` short-circuits in the parser,
# so an accepted flag prints help and scaffolds nothing.
if ! "$iii_bin" project init --learn-iii --help >/dev/null 2>&1; then
  # Never offer what this binary cannot run, and never name the command.
  echo ""
  echo "If you're new to iii, get started quickly here: $quickstart_url"
elif [ "$non_interactive" = false ] && [ -t 2 ] && [ -r /dev/tty ] && [ -w /dev/tty ]; then
  echo ""
  printf "Now let's setup your first iii project [Press Enter to Continue] " >/dev/tty
  # Ctrl-C here stops the install with iii already in place; EOF does the same.
  read -r _ </dev/tty || exit 0
  # stdin is the script itself under `curl ... | sh`, and `exec` would hand
  # that pipe to init, so give it the terminal instead. `exec` skips the
  # EXIT trap, so clean up first.
  cleanup
  exec "$iii_bin" project init --learn-iii </dev/tty
else
  echo ""
  echo "To start the iii harness and see what iii can do, run:"
  echo "  $start_cmd"
fi
