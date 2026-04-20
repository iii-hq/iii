#!/usr/bin/env sh
set -eu

REPO="${REPO:-iii-hq/iii}"
BIN_NAME="${BIN_NAME:-iii}"

telemetry_emitter=""
install_event_prefix=""
from_version=""
release_version=""
init_install_failed=""
worker_install_failed=""

# ---------------------------------------------------------------------------
# Output helpers
# ---------------------------------------------------------------------------

info() {
  printf '%s\n' "$*"
}

warn() {
  printf 'warning: %s\n' "$*" >&2
}

json_str() {
  printf '%s' "$1" | jq -Rs '.'
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

# ---------------------------------------------------------------------------
# GitHub API
# ---------------------------------------------------------------------------

github_api() {
  if [ -n "${GITHUB_TOKEN:-}" ]; then
    curl -fsSL \
      -H "Accept:application/vnd.github+json" \
      -H "X-GitHub-Api-Version:2022-11-28" \
      -H "Authorization:Bearer $GITHUB_TOKEN" \
      "$1"
  else
    curl -fsSL \
      -H "Accept:application/vnd.github+json" \
      -H "X-GitHub-Api-Version:2022-11-28" \
      "$1"
  fi
}

# Return 0 if the given URL responds with a GitHub rate-limit status.
github_rate_limited() {
  if [ -n "${GITHUB_TOKEN:-}" ]; then
    _code=$(curl -s -o /dev/null -w '%{http_code}' \
      -H "Accept:application/vnd.github+json" \
      -H "Authorization:Bearer $GITHUB_TOKEN" \
      "$1" 2>/dev/null || echo "")
  else
    _code=$(curl -s -o /dev/null -w '%{http_code}' \
      -H "Accept:application/vnd.github+json" \
      "$1" 2>/dev/null || echo "")
  fi
  case "$_code" in
    403|429) return 0 ;;
    *) return 1 ;;
  esac
}

# Emit a clear rate-limit error if that's what's happening; otherwise fall through.
check_rate_limit_or_continue() {
  _url="$1"
  if github_rate_limited "$_url"; then
    err "download" "GitHub API rate limit hit (unauthenticated: 60/hr). Retry later, or set \$GITHUB_TOKEN and pipe with: curl -H \"Authorization: Bearer \$GITHUB_TOKEN\" ..."
  fi
}

# ---------------------------------------------------------------------------
# Resolve a companion binary's download URL from the release JSON.
# Prints URL on stdout (empty if not found).
# ---------------------------------------------------------------------------
resolve_companion_url() {
  _name="$1"; _target="$2"; _assets_json="$3"
  _asset_name="${_name}-${_target}.tar.gz"
  _url=$(printf '%s' "$_assets_json" \
    | jq -r --arg n "$_asset_name" \
      '.assets[] | select(.name == $n) | .browser_download_url' \
    | head -n 1)
  if [ -z "$_url" ] || [ "$_url" = "null" ]; then
    printf ''
  else
    printf '%s' "$_url"
  fi
}

# ---------------------------------------------------------------------------
# Extract an already-downloaded companion tarball and install the binary.
# Returns 0 on success, 1 on failure (caller warns).
# ---------------------------------------------------------------------------
install_companion_from_tarball() {
  _name="$1"; _tarball="$2"; _bin_dir="$3"; _tmp="$4"
  if [ ! -f "$_tarball" ]; then
    warn "$_name: tarball missing ($_tarball)"
    return 1
  fi
  if ! tar -xzf "$_tarball" -C "$_tmp"; then
    warn "$_name: extract failed for $(basename "$_tarball")"
    return 1
  fi
  _bin=$(find "$_tmp" -type f -name "$_name" 2>/dev/null | head -n 1)
  if [ -z "$_bin" ] || [ ! -f "$_bin" ]; then
    warn "$_name: binary not found in tarball"
    return 1
  fi
  if ! install_bin "$_bin" "$_bin_dir/$_name"; then
    warn "$_name: failed to install to $_bin_dir/$_name"
    return 1
  fi
  info "installed $_name to $_bin_dir/$_name"
  return 0
}

# Test-mode hook: when this var is set, stop here so unit tests can source
# the helper functions above without running the installer.
if [ -n "${III_INSTALL_SH_TEST_MODE:-}" ]; then
  # When sourced, `return` works. When executed directly (rare, for debugging),
  # `return` fails outside a function and the fallback `exit` runs.
  # shellcheck disable=SC2317
  return 0 2>/dev/null || exit 0
fi

# ---------------------------------------------------------------------------
# Argument parsing
# ---------------------------------------------------------------------------

engine_version="${VERSION:-}"
use_next=false

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
    -h|--help)
      cat <<'USAGE'
Usage: install.sh [OPTIONS] [VERSION]

Install the iii engine (includes CLI commands).

Options:
  -h, --help            Show this help message
  --next                Install the latest "next" pre-release

Environment variables:
  VERSION               Engine version to install (e.g., 0.11.0)
  BIN_DIR               Engine binary installation directory
                        (default: $PREFIX/bin if set, else $HOME/.local/bin)
  PREFIX                Installation prefix (default: $HOME/.local)
  TARGET                Override target triple (e.g., x86_64-apple-darwin,
                        aarch64-unknown-linux-gnu)
  III_USE_GLIBC         Use glibc build on Linux x86_64 (any non-empty value
                        enables; default: musl)
  GITHUB_TOKEN          Authenticate GitHub API calls (raises rate limit
                        from 60/hr to 5000/hr). Any token with public read
                        access works; no scopes required.

Requires: curl, jq, tar (unzip if installing a .zip asset)

Examples:
  curl -fsSL https://iii.dev/install.sh | sh
  curl -fsSL https://iii.dev/install.sh | sh -s -- --next
  VERSION=0.11.0 curl -fsSL https://iii.dev/install.sh | sh
  BIN_DIR=/usr/local/bin curl -fsSL https://iii.dev/install.sh | sh
USAGE
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

VERSION="$engine_version"

# ---------------------------------------------------------------------------
# Dependency checks
# ---------------------------------------------------------------------------

if ! command -v curl >/dev/null 2>&1; then
  err "dependency" "curl is required ($(pkg_manager_hint curl))"
fi

if ! command -v jq >/dev/null 2>&1; then
  err "dependency" "jq is required ($(pkg_manager_hint jq))"
fi

# ---------------------------------------------------------------------------
# Platform / target detection
# ---------------------------------------------------------------------------

uname_s=$(uname -s 2>/dev/null || echo unknown)
uname_m=$(uname -m 2>/dev/null || echo unknown)

if [ -n "${TARGET:-}" ]; then
  target="$TARGET"
  case "$target" in
    x86_64-*) arch="x86_64" ;;
    aarch64-*) arch="aarch64" ;;
    armv7-*) arch="armv7" ;;
    *) arch="" ;;
  esac
else
  case "$uname_m" in
    x86_64|amd64) arch="x86_64" ;;
    arm64|aarch64) arch="aarch64" ;;
    armv7*) arch="armv7" ;;
    *)
      err "platform" "unsupported architecture: $uname_m (set TARGET=<triple> to override, or download a prebuilt binary from https://github.com/$REPO/releases)"
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
      err "platform" "unsupported OS: $uname_s (set TARGET=<triple> to override, or download a prebuilt binary from https://github.com/$REPO/releases)"
      ;;
  esac

  target="$arch-$os"
fi

# ---------------------------------------------------------------------------
# Release selection
# ---------------------------------------------------------------------------

if [ -n "$VERSION" ]; then
  info "installing version: $VERSION"
  _ver="${VERSION#iii/}"
  _ver="${_ver#v}"
  release_version="$_ver"
  _tag="iii/v${_ver}"
  api_url="https://api.github.com/repos/$REPO/releases/tags/${_tag}"
  if ! json=$(github_api "$api_url" 2>/dev/null); then
    _tag="v${_ver}"
    api_url="https://api.github.com/repos/$REPO/releases/tags/${_tag}"
    if ! json=$(github_api "$api_url" 2>/dev/null); then
      check_rate_limit_or_continue "https://api.github.com/repos/$REPO/releases/latest"
      err "download" "release tag not found: $VERSION (tried: iii/v${_ver}, v${_ver}). Browse releases: https://github.com/$REPO/releases"
    fi
  fi

  # Reject prereleases unless --next was passed
  _is_prerelease=$(printf '%s' "$json" | jq -r '.prerelease // false')
  if [ "$_is_prerelease" = "true" ] && [ "$use_next" != "true" ]; then
    err "download" "version $VERSION is a prerelease — use a stable release or pass --next"
  fi
elif [ "$use_next" = "true" ]; then
  info "installing latest next release"
  api_url="https://api.github.com/repos/$REPO/releases?per_page=20"
  if ! json_list=$(github_api "$api_url" 2>/dev/null); then
    check_rate_limit_or_continue "$api_url"
    err "download" "failed to list releases"
  fi
  json=$(printf '%s' "$json_list" | jq -c 'first(.[] | select(.tag_name | test("-next\\.")))')
  if [ "$json" = "null" ] || [ -z "$json" ]; then
    err "download" "no next release found"
  fi
else
  info "installing latest version"

  # Try /releases/latest first (single API call, non-prerelease guaranteed by GitHub)
  api_url="https://api.github.com/repos/$REPO/releases/latest"
  json=$(github_api "$api_url" 2>/dev/null) || json=""

  # Validate the tag matches our expected prefix
  if [ -n "$json" ]; then
    _latest_tag=$(printf '%s' "$json" | jq -r '.tag_name // ""')
    case "$_latest_tag" in
      iii/v*) ;;  # good
      *) json="" ;;  # fall back to listing
    esac
  fi

  if [ -z "$json" ]; then
    api_url="https://api.github.com/repos/$REPO/releases?per_page=20"
    if ! json_list=$(github_api "$api_url" 2>/dev/null); then
      check_rate_limit_or_continue "$api_url"
      err "download" "failed to list releases"
    fi
    json=$(printf '%s' "$json_list" | jq -c 'first(.[] | select(.prerelease == false and (.tag_name | startswith("iii/v"))))')
    if [ "$json" = "null" ] || [ -z "$json" ]; then
      err "download" "no stable iii release found"
    fi
  fi
fi

if [ -z "$release_version" ]; then
  release_version=$(printf '%s' "$json" | jq -r '.tag_name' | sed -E 's#^(iii/)?v##')
fi

if [ "$use_next" = "true" ] && [ -n "$release_version" ]; then
  info "installing $BIN_NAME v$release_version"
fi

# ---------------------------------------------------------------------------
# Pick the asset for our target
# ---------------------------------------------------------------------------

asset_url=$(printf '%s' "$json" \
  | jq -r --arg bn "$BIN_NAME" --arg target "$target" \
    '.assets[] | select((.name | startswith($bn + "-" + $target)) and (.name | test("\\.(tar\\.gz|tgz|zip)$"))) | .browser_download_url' \
  | head -n 1)

if [ -z "$asset_url" ]; then
  # Extract only main-binary targets (exclude .sha256, iii-init-*, iii-worker-*, libkrunfw-*).
  # Match $BIN_NAME directly followed by an architecture prefix.
  _available_targets=$(printf '%s' "$json" \
    | jq -r --arg bn "$BIN_NAME" \
      '.assets[]
       | .name
       | select(test("^" + $bn + "-(x86_64|aarch64|armv7)[^.]*\\.(tar\\.gz|tgz|zip)$"))
       | sub("^" + $bn + "-"; "")
       | sub("\\.(tar\\.gz|tgz|zip)$"; "")' \
    | sort -u)
  if [ -n "$release_version" ]; then
    _release_label="v$release_version"
  else
    _release_label=$(printf '%s' "$json" | jq -r '.tag_name // "this release"')
  fi

  {
    echo ""
    echo "$BIN_NAME $_release_label does not include a prebuilt binary for $target."
    if [ -n "$_available_targets" ]; then
      echo ""
      echo "Available $BIN_NAME targets in $_release_label:"
      printf '%s\n' "$_available_targets" | while IFS= read -r _t; do
        [ -n "$_t" ] && printf '  - %s\n' "$_t"
      done
    fi
    echo ""
    echo "Try:"
    echo "  - Install a stable release:  curl -fsSL https://iii.dev/install.sh | sh"
    echo "  - Pin a different version:   VERSION=<x.y.z> curl -fsSL https://iii.dev/install.sh | sh"
    echo "  - Override target triple:    TARGET=<triple> curl -fsSL https://iii.dev/install.sh | sh"
    echo "  - Browse all releases:       https://github.com/$REPO/releases"
  } >&2
  err "download" "no release asset found for target: $target"
fi

asset_name=$(basename "$asset_url")

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
if [ -n "$from_version" ]; then
  install_event_prefix="upgrade"
  if [ -x "$bin_dir/$BIN_NAME" ]; then
    telemetry_emitter="$bin_dir/$BIN_NAME"
  fi
else
  install_event_prefix="install"
fi

# Idempotency: if already at target version, skip the download and move on.
if [ -n "$from_version" ] && [ "$from_version" = "$release_version" ]; then
  info "$BIN_NAME is already at v$release_version — nothing to do"
  echo ""
  echo "If you're new to iii, get started quickly here: https://iii.dev/docs/quickstart"
  exit 0
fi

mkdir -p "$bin_dir"

tmpdir=$(mktemp -d 2>/dev/null || mktemp -d -t iii-install)
cleanup() { rm -rf "$tmpdir"; }
trap cleanup EXIT INT TERM

# ---------------------------------------------------------------------------
# Determine companion targets up front, so we can pre-resolve all URLs and
# download everything in one parallel curl call (single aggregated progress).
# ---------------------------------------------------------------------------

# iii-init: Linux ELF that runs inside VMs; macOS hosts also need it for libkrun guests.
init_target=""
case "$uname_s" in
  Linux)
    case "$arch" in
      x86_64)  init_target="x86_64-unknown-linux-musl" ;;
      aarch64) init_target="aarch64-unknown-linux-gnu" ;;
    esac
    ;;
  Darwin)
    case "$arch" in
      x86_64)  init_target="x86_64-apple-darwin" ;;
      aarch64) init_target="aarch64-apple-darwin" ;;
    esac
    ;;
esac

# iii-worker: needs glibc on Linux (KVM/libkrun); not available for x86_64-apple-darwin.
worker_target=""
case "$uname_s" in
  Linux)
    case "$arch" in
      x86_64)  worker_target="x86_64-unknown-linux-gnu" ;;
      aarch64) worker_target="aarch64-unknown-linux-gnu" ;;
    esac
    ;;
  Darwin)
    case "$arch" in
      aarch64) worker_target="aarch64-apple-darwin" ;;
    esac
    ;;
esac

# Pre-resolve companion URLs so we know what we're downloading before we start.
main_tarball="$tmpdir/$asset_name"

init_url=""
init_tarball=""
if [ -n "$init_target" ]; then
  init_url=$(resolve_companion_url "iii-init" "$init_target" "$json")
  if [ -n "$init_url" ]; then
    init_tarball="$tmpdir/iii-init-${init_target}.tar.gz"
  else
    warn "iii-init: asset 'iii-init-${init_target}.tar.gz' not found in release (this release may not include iii-init for your platform)"
    init_install_failed="1"
  fi
fi

worker_url=""
worker_tarball=""
if [ -n "$worker_target" ]; then
  worker_url=$(resolve_companion_url "iii-worker" "$worker_target" "$json")
  if [ -n "$worker_url" ]; then
    worker_tarball="$tmpdir/iii-worker-${worker_target}.tar.gz"
  else
    warn "iii-worker: asset 'iii-worker-${worker_target}.tar.gz' not found in release (this release may not include iii-worker for your platform)"
    worker_install_failed="1"
  fi
fi

# ---------------------------------------------------------------------------
# Download: single parallel curl call (aggregated progress) if supported and
# we have 2+ downloads; otherwise sequential with per-file progress bars.
# ---------------------------------------------------------------------------

# Count total downloads for [N/M] progress labels.
total=1
[ -n "$init_url" ] && total=$((total + 1))
[ -n "$worker_url" ] && total=$((total + 1))
idx=1

# Main binary (critical)
printf '[%d/%d] downloading %s...\n' "$idx" "$total" "$asset_name"
if [ -t 2 ]; then
  curl -fL --progress-bar "$asset_url" -o "$main_tarball" \
    || err "download" "failed to download $asset_url"
else
  curl -fsSL "$asset_url" -o "$main_tarball" \
    || err "download" "failed to download $asset_url"
fi

# iii-init (best-effort)
if [ -n "$init_url" ]; then
  idx=$((idx + 1))
  printf '[%d/%d] downloading %s...\n' "$idx" "$total" "$(basename "$init_tarball")"
  if [ -t 2 ]; then
    curl -fL --progress-bar "$init_url" -o "$init_tarball" \
      || { warn "iii-init: download failed"; init_install_failed="1"; rm -f "$init_tarball"; init_tarball=""; }
  else
    curl -fsSL "$init_url" -o "$init_tarball" \
      || { warn "iii-init: download failed"; init_install_failed="1"; rm -f "$init_tarball"; init_tarball=""; }
  fi
fi

# iii-worker (best-effort)
if [ -n "$worker_url" ]; then
  idx=$((idx + 1))
  printf '[%d/%d] downloading %s...\n' "$idx" "$total" "$(basename "$worker_tarball")"
  if [ -t 2 ]; then
    curl -fL --progress-bar "$worker_url" -o "$worker_tarball" \
      || { warn "iii-worker: download failed"; worker_install_failed="1"; rm -f "$worker_tarball"; worker_tarball=""; }
  else
    curl -fsSL "$worker_url" -o "$worker_tarball" \
      || { warn "iii-worker: download failed"; worker_install_failed="1"; rm -f "$worker_tarball"; worker_tarball=""; }
  fi
fi

# Validate: main is critical, companions warn on failure.
if [ ! -f "$main_tarball" ]; then
  err "download" "failed to download $asset_url"
fi
if [ -n "$init_url" ] && [ ! -f "$init_tarball" ]; then
  warn "iii-init: download failed"
  init_install_failed="1"
  init_tarball=""
fi
if [ -n "$worker_url" ] && [ ! -f "$worker_tarball" ]; then
  warn "iii-worker: download failed"
  worker_install_failed="1"
  worker_tarball=""
fi

# ---------------------------------------------------------------------------
# Extract main binary
# ---------------------------------------------------------------------------

case "$asset_name" in
  *.tar.gz|*.tgz)
    tar -xzf "$main_tarball" -C "$tmpdir" \
      || err "extract" "failed to extract $asset_name"
    ;;
  *.zip)
    if ! command -v unzip >/dev/null 2>&1; then
      err "extract" "unzip is required to extract $asset_name ($(pkg_manager_hint unzip))"
    fi
    unzip -q "$main_tarball" -d "$tmpdir" \
      || err "extract" "failed to extract $asset_name"
    ;;
  *)
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
if [ "$install_event_prefix" = "upgrade" ]; then
  payload=$(printf '{"from_version":%s,"to_version":%s,"install_method":"sh","target_binary":%s}' \
    "$(json_str "$from_version")" "$(json_str "$release_version")" "$(json_str "$BIN_NAME")")
  iii_emit_event "upgrade_started" "$payload"
else
  payload=$(printf '{"install_method":"sh","target_binary":%s,"os":%s,"arch":%s}' \
    "$(json_str "$BIN_NAME")" \
    "$(json_str "$(echo "$uname_s" | tr '[:upper:]' '[:lower:]')")" \
    "$(json_str "$uname_m")")
  iii_emit_event "install_started" "$payload"
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
# Extract + install companions from already-downloaded tarballs.
# Targets were detected and URLs pre-resolved before the parallel download.
# ---------------------------------------------------------------------------

if [ -n "$init_tarball" ] && [ -f "$init_tarball" ]; then
  if ! install_companion_from_tarball "iii-init" "$init_tarball" "$bin_dir" "$tmpdir"; then
    init_install_failed="1"
  fi
fi

if [ -n "$worker_tarball" ] && [ -f "$worker_tarball" ]; then
  if ! install_companion_from_tarball "iii-worker" "$worker_tarball" "$bin_dir" "$tmpdir"; then
    worker_install_failed="1"
  fi
fi

# ---------------------------------------------------------------------------
# Telemetry: success
# ---------------------------------------------------------------------------

if [ "$install_event_prefix" = "upgrade" ]; then
  payload=$(printf '{"from_version":%s,"to_version":%s,"install_method":"sh","target_binary":%s}' \
    "$(json_str "$from_version")" "$(json_str "$installed_version")" "$(json_str "$BIN_NAME")")
  iii_emit_event "upgrade_succeeded" "$payload"
else
  payload=$(printf '{"installed_version":%s,"install_method":"sh","target_binary":%s}' \
    "$(json_str "$installed_version")" "$(json_str "$BIN_NAME")")
  iii_emit_event "install_succeeded" "$payload"
fi

# Best-effort: have the binary initialize its telemetry IDs.
# Older binaries won't have this flag — silently skip.
"$bin_dir/$BIN_NAME" --install-only-generate-ids >/dev/null 2>&1 || true

# ---------------------------------------------------------------------------
# Post-install verification
# ---------------------------------------------------------------------------

if [ -z "$installed_version" ]; then
  warn "$BIN_NAME installed but 'iii --version' produced no output — the binary may not run on this system (Gatekeeper? libc mismatch? try: $bin_dir/$BIN_NAME --version)"
fi

# ---------------------------------------------------------------------------
# Post-install notes (platform-specific gotchas)
# ---------------------------------------------------------------------------

if [ "$uname_s" = "Darwin" ] && [ "$arch" = "x86_64" ] && [ -z "$worker_target" ]; then
  echo ""
  echo "note: iii-worker is not available on macOS Intel yet."
  echo "      VM isolation features (sandboxed workers) will not work on this machine."
fi

if [ -n "$init_install_failed" ]; then
  echo ""
  echo "note: iii-init failed to install. VM-based sandbox workers will be unavailable"
  echo "      until you re-run install.sh or install iii-init manually from:"
  echo "      https://github.com/$REPO/releases"
fi

if [ -n "$worker_install_failed" ]; then
  echo ""
  echo "note: iii-worker failed to install. VM isolation features will be unavailable"
  echo "      until you re-run install.sh or install iii-worker manually from:"
  echo "      https://github.com/$REPO/releases"
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

echo ""
echo "If you're new to iii, get started quickly here: https://iii.dev/docs/quickstart"
