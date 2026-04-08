#!/usr/bin/env sh
set -eu

REPO="${REPO:-iii-hq/iii}"
BIN_NAME="${BIN_NAME:-iii}"

telemetry_emitter=""
install_event_prefix=""
from_version=""
release_version=""

# Returns a JSON-encoded string (with surrounding quotes).
# Uses jq when available; falls back to awk for escaping.
json_str() {
  if command -v jq >/dev/null 2>&1; then
    printf '%s' "$1" | jq -Rs '.'
    return
  fi
  printf '%s' "$1" | awk '
    BEGIN { ORS=""; printf "\"" }
    {
      gsub(/\\/, "\\\\")
      gsub(/"/, "\\\"")
      gsub(/\t/, "\\t")
      gsub(/\r/, "\\r")
      if (NR > 1) printf "\\n"
      printf "%s", $0
    }
    END { printf "\"" }
  '
}

err() {
  _stage="$1"; shift
  echo "error: $*" >&2
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
# Telemetry helpers
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

iii_detect_from_version() {
  _iii_bin_path="$1"
  if command -v "$_iii_bin_path" >/dev/null 2>&1; then
    "$_iii_bin_path" --version 2>/dev/null | awk '{print $NF}' || echo ""
  elif [ -x "$_iii_bin_path" ]; then
    "$_iii_bin_path" --version 2>/dev/null | awk '{print $NF}' || echo ""
  else
    echo ""
  fi
}

# --- Argument parsing ---
engine_version="${VERSION:-}"
use_next=false

while [ $# -gt 0 ]; do
  case "$1" in
    --no-cli)
      shift
      ;;
    --cli-version)
      if [ $# -ge 2 ] && case "$2" in -*) false;; *) true;; esac; then shift 2; else shift; fi
      ;;
    --cli-dir)
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
  VERSION               Engine version to install
  BIN_DIR               Engine binary installation directory
  PREFIX                Installation prefix (used if BIN_DIR not set)
  TARGET                Override target triple
  III_USE_GLIBC         Use glibc build on Linux x86_64
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

if ! command -v curl >/dev/null 2>&1; then
  err "dependency" "curl is required"
fi

if [ -n "${TARGET:-}" ]; then
  target="$TARGET"
else
  uname_s=$(uname -s 2>/dev/null || echo unknown)
  uname_m=$(uname -m 2>/dev/null || echo unknown)

  case "$uname_m" in
    x86_64|amd64)
      arch="x86_64"
      ;;
    arm64|aarch64)
      arch="aarch64"
      ;;
    armv7*)
      arch="armv7"
      ;;
    *)
      err "platform" "unsupported architecture: $uname_m"
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
              echo "using glibc build (system glibc: $sys_glibc)"
            else
              echo "warning: system glibc $sys_glibc is older than required $required_glibc, falling back to musl" >&2
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
      err "platform" "unsupported OS: $uname_s"
      ;;
  esac

  target="$arch-$os"
fi

api_headers="-H Accept:application/vnd.github+json -H X-GitHub-Api-Version:2022-11-28"
github_api() {
  # shellcheck disable=SC2086
  curl -fsSL $api_headers "$1"
}

if [ -n "$VERSION" ]; then
  echo "installing version: $VERSION"
  _ver="${VERSION#iii/}"
  _ver="${_ver#v}"
  release_version="$_ver"
  _tag="iii/v${_ver}"
  api_url="https://api.github.com/repos/$REPO/releases/tags/${_tag}"
  json=$(github_api "$api_url" 2>/dev/null) || {
    _tag="v${_ver}"
    api_url="https://api.github.com/repos/$REPO/releases/tags/${_tag}"
    json=$(github_api "$api_url") || err "download" "release tag not found: $VERSION (tried tags: iii/v${_ver}, v${_ver})"
  }

  # Reject prereleases even when a specific version is requested
  _is_prerelease=""
  if command -v jq >/dev/null 2>&1; then
    _is_prerelease=$(printf '%s' "$json" | jq -r '.prerelease')
  else
    case "$json" in
      *'"prerelease":true'*|*'"prerelease": true'*) _is_prerelease="true" ;;
    esac
  fi
  if [ "$_is_prerelease" = "true" ] && [ "$use_next" != "true" ]; then
    err "download" "version $VERSION is a prerelease — use a stable release or pass --next"
  fi
elif [ "$use_next" = "true" ]; then
  echo "installing latest next release"
  api_url="https://api.github.com/repos/$REPO/releases?per_page=20"
  json_list=$(github_api "$api_url")
  if command -v jq >/dev/null 2>&1; then
    json=$(printf '%s' "$json_list" \
      | jq -c 'first(.[] | select(.tag_name | test("-next\\.")))')
    if [ "$json" = "null" ] || [ -z "$json" ]; then
      err "download" "no next release found"
    fi
  else
    _tag=$(printf '%s' "$json_list" \
      | tr '{' '\n' \
      | grep -oE '"tag_name"[[:space:]]*:[[:space:]]*"iii/v[^"]*-next\.[^"]*"' \
      | head -n 1 \
      | sed -E 's/.*"([^"]+)".*/\1/')
    if [ -z "$_tag" ]; then
      err "download" "no next release found"
    fi
    api_url="https://api.github.com/repos/$REPO/releases/tags/${_tag}"
    json=$(github_api "$api_url")
  fi
else
  echo "installing latest version"

  # Try /releases/latest first — single API call, GitHub guarantees non-prerelease/non-draft
  api_url="https://api.github.com/repos/$REPO/releases/latest"
  json=$(github_api "$api_url" 2>/dev/null) || json=""

  # Validate the tag matches our expected prefix (iii/v*)
  if [ -n "$json" ]; then
    if command -v jq >/dev/null 2>&1; then
      _latest_tag=$(printf '%s' "$json" | jq -r '.tag_name // ""')
    else
      _latest_tag=$(printf '%s' "$json" \
        | grep -oE '"tag_name"[[:space:]]*:[[:space:]]*"[^"]+"' \
        | head -n 1 \
        | sed -E 's/.*"([^"]+)".*/\1/')
    fi

    case "$_latest_tag" in
      iii/v*) ;; # Tag matches expected prefix, use this release
      *)
        # Latest release doesn't match our prefix — fall back to listing
        json=""
        ;;
    esac
  fi

  # Fallback: list releases and filter by prefix + non-prerelease
  if [ -z "$json" ]; then
    api_url="https://api.github.com/repos/$REPO/releases?per_page=20"
    json_list=$(github_api "$api_url")
    if command -v jq >/dev/null 2>&1; then
      json=$(printf '%s' "$json_list" \
        | jq -c 'first(.[] | select(.prerelease == false and (.tag_name | startswith("iii/v"))))')
      if [ "$json" = "null" ] || [ -z "$json" ]; then
        err "download" "no stable iii release found"
      fi
    else
      # No-jq path: filter for iii/v* tags from non-prerelease entries
      _tag=$(printf '%s' "$json_list" \
        | tr '{' '\n' \
        | grep -v '"prerelease"[[:space:]]*:[[:space:]]*true' \
        | grep -oE '"tag_name"[[:space:]]*:[[:space:]]*"iii/v[^"]+"' \
        | head -n 1 \
        | sed -E 's/.*"([^"]+)".*/\1/')
      if [ -z "$_tag" ]; then
        err "download" "could not determine latest release"
      fi
      api_url="https://api.github.com/repos/$REPO/releases/tags/${_tag}"
      json=$(github_api "$api_url")
    fi
  fi
fi

if [ -z "$release_version" ]; then
  if command -v jq >/dev/null 2>&1; then
    release_version=$(printf '%s' "$json" | jq -r '.tag_name' | sed -E 's#^(iii/)?v##')
  else
    release_version=$(printf '%s' "$json" \
      | grep -oE '"tag_name"[[:space:]]*:[[:space:]]*"[^"]+"' \
      | head -n 1 \
      | sed -E 's/.*"([^"]+)".*/\1/' \
      | sed -E 's#^(iii/)?v##')
  fi
fi

if [ "$use_next" = "true" ] && [ -n "$release_version" ]; then
  echo "installing $BIN_NAME v$release_version"
fi

if command -v jq >/dev/null 2>&1; then
  asset_url=$(printf '%s' "$json" \
    | jq -r --arg bn "$BIN_NAME" --arg target "$target" \
      '.assets[] | select((.name | startswith($bn + "-" + $target)) and (.name | test("\\.(tar\\.gz|tgz|zip)$"))) | .browser_download_url' \
    | head -n 1)
else
  asset_url=$(printf '%s' "$json" \
    | grep -oE '"browser_download_url"[[:space:]]*:[[:space:]]*"[^"]+"' \
    | sed -E 's/.*"([^"]+)".*/\1/' \
    | grep -F "$BIN_NAME-$target" \
    | grep -E '\.(tar\.gz|tgz|zip)$' \
    | head -n 1)
fi

if [ -z "$asset_url" ]; then
  echo "available assets:" >&2
  printf '%s' "$json" \
    | grep -oE '"browser_download_url"[[:space:]]*:[[:space:]]*"[^"]+"' \
    | sed -E 's/.*"([^"]+)".*/\1/' >&2
  err "download" "no release asset found for target: $target"
fi

asset_name=$(basename "$asset_url")

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
  # Use the existing binary for telemetry until the new one is extracted
  if [ -x "$bin_dir/$BIN_NAME" ]; then
    telemetry_emitter="$bin_dir/$BIN_NAME"
  fi
else
  install_event_prefix="install"
fi

mkdir -p "$bin_dir"

tmpdir=$(mktemp -d 2>/dev/null || mktemp -d -t iii-install)
cleanup() {
  rm -rf "$tmpdir"
}
trap cleanup EXIT INT TERM

curl -fsSL -L "$asset_url" -o "$tmpdir/$asset_name" \
  || err "download" "failed to download $asset_url"

case "$asset_name" in
  *.tar.gz|*.tgz)
    tar -xzf "$tmpdir/$asset_name" -C "$tmpdir" \
      || err "extract" "failed to extract $asset_name"
    ;;
  *.zip)
    if ! command -v unzip >/dev/null 2>&1; then
      err "extract" "unzip is required to extract $asset_name"
    fi
    unzip -q "$tmpdir/$asset_name" -d "$tmpdir" \
      || err "extract" "failed to extract $asset_name"
    ;;
  *)
    ;;
 esac

if [ -f "$tmpdir/$BIN_NAME" ]; then
  bin_file="$tmpdir/$BIN_NAME"
else
  bin_file=$(find "$tmpdir" -type f \( -name "$BIN_NAME" -o -name "${BIN_NAME}.exe" \) | head -n 1)
fi

if [ -z "${bin_file:-}" ] || [ ! -f "$bin_file" ]; then
  err "binary_lookup" "binary not found in downloaded asset"
fi

telemetry_emitter="$bin_file"
if [ "$install_event_prefix" = "upgrade" ]; then
  payload=$(printf '{"from_version":%s,"to_version":%s,"install_method":"sh","target_binary":%s}' \
    "$(json_str "$from_version")" "$(json_str "$release_version")" "$(json_str "$BIN_NAME")")
  iii_emit_event "upgrade_started" "$payload"
else
  payload=$(printf '{"install_method":"sh","target_binary":%s,"os":%s,"arch":%s}' \
    "$(json_str "$BIN_NAME")" "$(json_str "$(uname -s 2>/dev/null | tr '[:upper:]' '[:lower:]' || echo unknown)")" "$(json_str "$(uname -m 2>/dev/null || echo unknown)")")
  iii_emit_event "install_started" "$payload"
fi

installed_version=""
if command -v install >/dev/null 2>&1; then
  install -m 755 "$bin_file" "$bin_dir/$BIN_NAME" \
    || err "install" "failed to install binary to $bin_dir/$BIN_NAME"
else
  { cp "$bin_file" "$bin_dir/$BIN_NAME" && chmod 755 "$bin_dir/$BIN_NAME"; } \
    || err "install" "failed to copy binary to $bin_dir/$BIN_NAME"
fi

installed_version=$("$bin_dir/$BIN_NAME" --version 2>/dev/null | awk '{print $NF}' || echo "")

printf 'installed %s to %s\n' "$BIN_NAME" "$bin_dir/$BIN_NAME"

# ---------------------------------------------------------------------------
# Install iii-init (VM init binary for sandbox workers)
# The init binary is a Linux ELF that runs inside VMs, but macOS hosts also
# need it for libkrun/Hypervisor.framework guests.
# ---------------------------------------------------------------------------
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

if [ -n "$init_target" ]; then
  init_asset_name="iii-init-${init_target}.tar.gz"

  if command -v jq >/dev/null 2>&1; then
    init_asset_url=$(printf '%s' "$json" \
      | jq -r --arg name "$init_asset_name" \
        '.assets[] | select(.name == $name) | .browser_download_url' \
      | head -n 1)
  else
    init_asset_url=$(printf '%s' "$json" \
      | grep -oE '"browser_download_url"[[:space:]]*:[[:space:]]*"[^"]+"' \
      | sed -E 's/.*"([^"]+)".*/\1/' \
      | grep -F "$init_asset_name" \
      | head -n 1)
  fi

  if [ -n "$init_asset_url" ]; then
    curl -fsSL -L "$init_asset_url" -o "$tmpdir/$init_asset_name" 2>/dev/null
    if [ $? -eq 0 ]; then
      tar -xzf "$tmpdir/$init_asset_name" -C "$tmpdir" 2>/dev/null
      init_bin_file=$(find "$tmpdir" -type f -name "iii-init" | head -n 1)
      if [ -n "$init_bin_file" ] && [ -f "$init_bin_file" ]; then
        if command -v install >/dev/null 2>&1; then
          install -m 755 "$init_bin_file" "$bin_dir/iii-init"
        else
          cp "$init_bin_file" "$bin_dir/iii-init"
          chmod 755 "$bin_dir/iii-init"
        fi
        printf 'installed %s to %s\n' "iii-init" "$bin_dir/iii-init"
      fi
    fi
  fi
fi

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

case ":$PATH:" in
  *":$bin_dir:"*)
    ;;
  *)
    printf 'add %s to your PATH if needed\n' "$bin_dir"
    ;;
 esac

echo ""
echo "If you're new to iii, get started quickly here: https://iii.dev/docs/quickstart"
