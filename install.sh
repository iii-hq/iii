#!/usr/bin/env sh
set -eu

REPO="${REPO:-MotiaDev/iii-engine}"
BIN_NAME="${BIN_NAME:-iii}"
VERSION="${VERSION:-${1:-}}"

err() {
  echo "error: $*" >&2
  exit 1
}

if ! command -v curl >/dev/null 2>&1; then
  err "curl is required"
fi

if [ -n "${TARGET:-}" ]; then
  target="$TARGET"
else
  uname_s=$(uname -s 2>/dev/null || echo unknown)
  uname_m=$(uname -m 2>/dev/null || echo unknown)

  case "$uname_s" in
    Darwin)
      os="apple-darwin"
      ;;
    Linux)
      os="unknown-linux-gnu"
      ;;
    *)
      err "unsupported OS: $uname_s"
      ;;
  esac

  case "$uname_m" in
    x86_64|amd64)
      arch="x86_64"
      ;;
    arm64|aarch64)
      arch="aarch64"
      ;;
    *)
      err "unsupported architecture: $uname_m"
      ;;
  esac

  target="$arch-$os"
fi

api_headers="-H Accept:application/vnd.github+json -H X-GitHub-Api-Version:2022-11-28"
github_api() {
  if [ -n "${GITHUB_TOKEN:-}" ]; then
    curl -fsSL $api_headers -H "Authorization: Bearer $GITHUB_TOKEN" "$1"
  else
    curl -fsSL $api_headers "$1"
  fi
}

if [ -n "$VERSION" ]; then
  echo "installing version: $VERSION"
  api_url="https://api.github.com/repos/$REPO/releases/tags/$VERSION"
  json=$(github_api "$api_url") || {
    if [ "${VERSION#v}" = "$VERSION" ]; then
      api_url="https://api.github.com/repos/$REPO/releases/tags/v$VERSION"
      json=$(github_api "$api_url") || err "release tag not found: $VERSION"
    else
      err "release tag not found: $VERSION"
    fi
  }
else
  echo "installing latest version"
  api_url="https://api.github.com/repos/$REPO/releases/latest"
  json=$(github_api "$api_url")
fi

if command -v jq >/dev/null 2>&1; then
  asset_url=$(printf '%s' "$json" \
    | jq -r --arg target "$target" '.assets[] | select(.name | test($target)) | .browser_download_url' \
    | head -n 1)
  asset_id=$(printf '%s' "$json" \
    | jq -r --arg target "$target" '.assets[] | select(.name | test($target)) | .id' \
    | head -n 1)
else
  asset_url=$(printf '%s' "$json" \
    | grep -oE '"browser_download_url"[[:space:]]*:[[:space:]]*"[^"]+"' \
    | sed -E 's/.*"([^"]+)".*/\1/' \
    | grep "$target" \
    | head -n 1)
  asset_id=$(printf '%s' "$json" | awk -v target="$target" '
    /"id"[[:space:]]*:/ {
      line=$0
      gsub(/[^0-9]/, "", line)
      if (line != "") last_id=line
    }
    /"name"[[:space:]]*:/ && $0 ~ target {
      if (last_id != "") { print last_id; exit }
    }
  ')
fi

if [ -z "$asset_url" ]; then
  echo "available assets:" >&2
  printf '%s' "$json" \
    | grep -oE '"browser_download_url"[[:space:]]*:[[:space:]]*"[^"]+"' \
    | sed -E 's/.*"([^"]+)".*/\1/' >&2
  err "no release asset found for target: $target"
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

mkdir -p "$bin_dir"

tmpdir=$(mktemp -d 2>/dev/null || mktemp -d -t iii-install)
cleanup() {
  rm -rf "$tmpdir"
}
trap cleanup EXIT INT TERM

if [ -n "${GITHUB_TOKEN:-}" ] && [ -n "${asset_id:-}" ] && [ "$asset_id" != "null" ]; then
  asset_api_url="https://api.github.com/repos/$REPO/releases/assets/$asset_id"
  curl -fsSL -H "Accept: application/octet-stream" -H "Authorization: Bearer $GITHUB_TOKEN" \
    -H "X-GitHub-Api-Version: 2022-11-28" -L "$asset_api_url" -o "$tmpdir/$asset_name"
else
  curl -fsSL -L "$asset_url" -o "$tmpdir/$asset_name"
fi

case "$asset_name" in
  *.tar.gz|*.tgz)
    tar -xzf "$tmpdir/$asset_name" -C "$tmpdir"
    ;;
  *.zip)
    if ! command -v unzip >/dev/null 2>&1; then
      err "unzip is required to extract $asset_name"
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
  err "binary not found in downloaded asset"
fi

if command -v install >/dev/null 2>&1; then
  install -m 755 "$bin_file" "$bin_dir/$BIN_NAME"
else
  cp "$bin_file" "$bin_dir/$BIN_NAME"
  chmod 755 "$bin_dir/$BIN_NAME"
fi

printf 'installed %s to %s\n' "$BIN_NAME" "$bin_dir/$BIN_NAME"

case ":$PATH:" in
  *":$bin_dir:"*)
    ;;
  *)
    printf 'add %s to your PATH if needed\n' "$bin_dir"
    ;;
 esac
