#!/usr/bin/env sh
set -eu

REPO="${REPO:-iii-hq/iii}"
BIN_NAME="${BIN_NAME:-iii}"

telemetry_emitter=""
install_event_prefix=""
from_version=""
release_version=""

err() {
  _stage="$1"; shift
  echo "error: $*" >&2
  if [ -n "${install_event_prefix:-}" ]; then
    _err_msg=$(echo "$*" | tr '"' "'")
    if [ "$install_event_prefix" = "upgrade" ]; then
      iii_emit_event "upgrade_failed" \
        "{\"from_version\":\"${from_version:-}\",\"to_version\":\"${release_version}\",\"install_method\":\"sh\",\"target_binary\":\"${BIN_NAME}\",\"error_stage\":\"${_stage}\",\"error_message\":\"${_err_msg}\"}"
    else
      iii_emit_event "install_failed" \
        "{\"install_method\":\"sh\",\"target_binary\":\"${BIN_NAME}\",\"error_stage\":\"${_stage}\",\"error_message\":\"${_err_msg}\"}"
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
    -h|--help)
      cat <<'USAGE'
Usage: install.sh [OPTIONS] [VERSION]

Install the iii engine (includes CLI commands).

Options:
  -h, --help            Show this help message

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
else
  echo "installing latest version"
  api_url="https://api.github.com/repos/$REPO/releases?per_page=20"
  json_list=$(github_api "$api_url")
  if command -v jq >/dev/null 2>&1; then
    json=$(printf '%s' "$json_list" \
      | jq -c 'first(.[] | select(.prerelease == false and ((.tag_name | startswith("iii/v")) or (.tag_name | startswith("v")))))')
    if [ "$json" = "null" ] || [ -z "$json" ]; then
      err "download" "no stable iii release found"
    fi
  else
    _tag=$(printf '%s' "$json_list" \
      | grep -oE '"tag_name"[[:space:]]*:[[:space:]]*"(iii/v|v)[^"]+"' \
      | head -n 1 \
      | sed -E 's/.*"([^"]+)".*/\1/')
    if [ -z "$_tag" ]; then
      err "download" "could not determine latest release"
    fi
    api_url="https://api.github.com/repos/$REPO/releases/tags/${_tag}"
    json=$(github_api "$api_url")
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
else
  install_event_prefix="install"
fi

mkdir -p "$bin_dir"

tmpdir=$(mktemp -d 2>/dev/null || mktemp -d -t iii-install)
cleanup() {
  rm -rf "$tmpdir"
}
trap cleanup EXIT INT TERM

curl -fsSL -L "$asset_url" -o "$tmpdir/$asset_name"

case "$asset_name" in
  *.tar.gz|*.tgz)
    tar -xzf "$tmpdir/$asset_name" -C "$tmpdir"
    ;;
  *.zip)
    if ! command -v unzip >/dev/null 2>&1; then
      err "extract" "unzip is required to extract $asset_name"
    fi
    unzip -q "$tmpdir/$asset_name" -d "$tmpdir"
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
  iii_emit_event "upgrade_started" \
    "{\"from_version\":\"${from_version}\",\"to_version\":\"${release_version}\",\"install_method\":\"sh\",\"target_binary\":\"${BIN_NAME}\"}"
else
  iii_emit_event "install_started" \
    "{\"install_method\":\"sh\",\"target_binary\":\"${BIN_NAME}\",\"os\":\"$(uname -s 2>/dev/null | tr '[:upper:]' '[:lower:]' || echo unknown)\",\"arch\":\"$(uname -m 2>/dev/null || echo unknown)\"}"
fi

installed_version=""
if command -v install >/dev/null 2>&1; then
  install -m 755 "$bin_file" "$bin_dir/$BIN_NAME"
else
  cp "$bin_file" "$bin_dir/$BIN_NAME"
  chmod 755 "$bin_dir/$BIN_NAME"
fi

installed_version=$("$bin_dir/$BIN_NAME" --version 2>/dev/null | awk '{print $NF}' || echo "")

printf 'installed %s to %s\n' "$BIN_NAME" "$bin_dir/$BIN_NAME"

if [ "$install_event_prefix" = "upgrade" ]; then
  iii_emit_event "upgrade_succeeded" \
    "{\"from_version\":\"${from_version}\",\"to_version\":\"${installed_version}\",\"install_method\":\"sh\",\"target_binary\":\"${BIN_NAME}\"}"
else
  iii_emit_event "install_succeeded" \
    "{\"installed_version\":\"${installed_version}\",\"install_method\":\"sh\",\"target_binary\":\"${BIN_NAME}\"}"
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
