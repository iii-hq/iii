#!/usr/bin/env sh
set -eu

REPO="${REPO:-iii-hq/iii}"
BIN_NAME="${BIN_NAME:-iii}"
CLI_INSTALL_URL="${CLI_INSTALL_URL:-https://install.iii.dev/iii-cli/main/install.sh}"

AMPLITUDE_ENDPOINT="https://api2.amplitude.com/2/httpapi"
AMPLITUDE_API_KEY="${III_INSTALL_AMPLITUDE_API_KEY:-e8fb1f8d290a72dbb2d9b264926be4bf}"

err() {
  echo "error: $*" >&2
  if [ -n "${install_event_prefix:-}" ] && [ -n "${install_id:-}" ] && [ -n "${telemetry_id:-}" ]; then
    _err_msg=$(echo "$*" | tr '"' "'")
    if [ "$install_event_prefix" = "upgrade" ]; then
      iii_send_event "upgrade_failed" \
        "\"install_id\":\"${install_id}\",\"from_version\":\"${from_version:-}\",\"install_method\":\"sh\",\"target_binary\":\"${BIN_NAME}\",\"error_stage\":\"${_err_msg}\"" \
        "$telemetry_id" "$install_id"
    else
      iii_send_event "install_failed" \
        "\"install_id\":\"${install_id}\",\"install_method\":\"sh\",\"target_binary\":\"${BIN_NAME}\",\"error_stage\":\"${_err_msg}\"" \
        "$telemetry_id" "$install_id"
    fi
    wait
  fi
  exit 1
}

# ---------------------------------------------------------------------------
# Telemetry helpers
# ---------------------------------------------------------------------------

iii_telemetry_enabled() {
  case "${III_TELEMETRY_ENABLED:-}" in
    false|0) return 1 ;;
  esac
  for ci_var in CI GITHUB_ACTIONS GITLAB_CI CIRCLECI JENKINS_URL TRAVIS BUILDKITE TF_BUILD CODEBUILD_BUILD_ID BITBUCKET_BUILD_NUMBER DRONE TEAMCITY_VERSION; do
    if [ -n "$(eval "echo \${${ci_var}:-}")" ]; then
      return 1
    fi
  done
  return 0
}

iii_gen_uuid() {
  if command -v uuidgen >/dev/null 2>&1; then
    uuidgen | tr '[:upper:]' '[:lower:]'
  elif [ -r /proc/sys/kernel/random/uuid ]; then
    cat /proc/sys/kernel/random/uuid
  else
    od -x /dev/urandom 2>/dev/null | head -1 | awk '{OFS="-"; print $2$3,$4,$5,$6,$7$8$9}' | head -c 36 || echo "00000000-0000-0000-0000-000000000000"
  fi
}

iii_ini_path() {
  echo "${HOME}/.iii/telemetry.ini"
}

iii_read_ini_key() {
  _ini_section="$1"
  _ini_key="$2"
  _ini_file=$(iii_ini_path)
  if [ ! -f "$_ini_file" ]; then
    echo ""
    return
  fi
  _in_section=0
  while IFS= read -r _line || [ -n "$_line" ]; do
    _line=$(printf '%s' "$_line" | sed 's/^[[:space:]]*//;s/[[:space:]]*$//')
    case "$_line" in
      "[$_ini_section]") _in_section=1 ;;
      "["*"]") _in_section=0 ;;
      *)
        if [ "$_in_section" = "1" ]; then
          case "$_line" in
            "$_ini_key ="*|"$_ini_key= "*|"$_ini_key=")
              _val=$(printf '%s' "$_line" | cut -d'=' -f2- | sed 's/^[[:space:]]*//')
              echo "$_val"
              return
              ;;
          esac
        fi
        ;;
    esac
  done < "$_ini_file"
  echo ""
}

iii_set_ini_key() {
  _ini_section="$1"
  _ini_key="$2"
  _ini_value="$3"
  _ini_file=$(iii_ini_path)
  mkdir -p "$(dirname "$_ini_file")"
  _tmp_file="${_ini_file}.tmp"
  _written=0
  _in_target=0
  _key_written=0
  : > "$_tmp_file"
  if [ -f "$_ini_file" ]; then
    while IFS= read -r _line || [ -n "$_line" ]; do
      _trimmed=$(printf '%s' "$_line" | sed 's/^[[:space:]]*//;s/[[:space:]]*$//')
      case "$_trimmed" in
        "[$_ini_section]")
          printf '%s\n' "$_trimmed" >> "$_tmp_file"
          _in_target=1
          ;;
        "["*"]")
          if [ "$_in_target" = "1" ] && [ "$_key_written" = "0" ]; then
            printf '%s = %s\n' "$_ini_key" "$_ini_value" >> "$_tmp_file"
            _key_written=1
          fi
          _in_target=0
          printf '%s\n' "$_trimmed" >> "$_tmp_file"
          ;;
        "$_ini_key ="*|"$_ini_key= "*|"$_ini_key=")
          if [ "$_in_target" = "1" ]; then
            printf '%s = %s\n' "$_ini_key" "$_ini_value" >> "$_tmp_file"
            _key_written=1
          else
            printf '%s\n' "$_line" >> "$_tmp_file"
          fi
          ;;
        "")
          printf '\n' >> "$_tmp_file"
          ;;
        *)
          printf '%s\n' "$_line" >> "$_tmp_file"
          ;;
      esac
    done < "$_ini_file"
  fi
  if [ "$_key_written" = "0" ]; then
    if [ "$_in_target" = "1" ]; then
      printf '%s = %s\n' "$_ini_key" "$_ini_value" >> "$_tmp_file"
    else
      printf '\n[%s]\n%s = %s\n' "$_ini_section" "$_ini_key" "$_ini_value" >> "$_tmp_file"
    fi
  fi
  mv "$_tmp_file" "$_ini_file"
}

iii_get_or_create_telemetry_id() {
  _existing_id=$(iii_read_ini_key "identity" "id")
  if [ -n "$_existing_id" ]; then
    echo "$_existing_id"
    return
  fi

  _legacy_path="${HOME}/.iii/telemetry_id"
  if [ -f "$_legacy_path" ]; then
    _legacy_id=$(cat "$_legacy_path" 2>/dev/null | tr -d '[:space:]')
    if [ -n "$_legacy_id" ]; then
      iii_set_ini_key "identity" "id" "$_legacy_id"
      echo "$_legacy_id"
      return
    fi
  fi

  mkdir -p "${HOME}/.iii"
  _new_id=$(iii_gen_uuid)
  iii_set_ini_key "identity" "id" "$_new_id"
  echo "$_new_id"
}

iii_send_event() {
  _event_type="$1"
  _event_props="$2"
  _telemetry_id="$3"
  _install_id="$4"

  if [ -z "$AMPLITUDE_API_KEY" ]; then
    return 0
  fi

  if ! iii_telemetry_enabled; then
    return 0
  fi

  _os=$(uname -s 2>/dev/null | tr '[:upper:]' '[:lower:]' || echo "unknown")
  _arch=$(uname -m 2>/dev/null || echo "unknown")
  _ts=$(date +%s 2>/dev/null || echo "0")
  _ts_ms=$(( _ts * 1000 ))

  _payload="{\"api_key\":\"${AMPLITUDE_API_KEY}\",\"events\":[{\"device_id\":\"${_telemetry_id}\",\"user_id\":\"${_telemetry_id}\",\"event_type\":\"${_event_type}\",\"event_properties\":{${_event_props}},\"platform\":\"install-script\",\"os_name\":\"${_os}\",\"app_version\":\"script\",\"time\":${_ts_ms},\"insert_id\":\"$(iii_gen_uuid)\",\"ip\":\"\$remote\"}]}"

  curl -s -o /dev/null -X POST "$AMPLITUDE_ENDPOINT" \
    -H "Content-Type: application/json" \
    --data-raw "$_payload" &
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

iii_export_host_user_id() {
  _huid=$(iii_read_ini_key "identity" "id")
  if [ -z "$_huid" ]; then
    return 0
  fi
  _export_line="export III_HOST_USER_ID=\"${_huid}\""
  for _profile in "${HOME}/.bashrc" "${HOME}/.zshrc" "${HOME}/.profile"; do
    if [ -f "$_profile" ] && ! grep -qF "III_HOST_USER_ID" "$_profile" 2>/dev/null; then
      printf '\n# iii host correlation\n%s\n' "$_export_line" >> "$_profile"
      break
    fi
  done
}

install_cli() {
  echo ""
  echo "installing iii cli..."

  cli_tmpdir=$(mktemp -d 2>/dev/null || mktemp -d -t iii-cli-install)
  cli_script="$cli_tmpdir/install-cli.sh"

  if ! curl -fsSL "$CLI_INSTALL_URL" -o "$cli_script"; then
    echo "warning: failed to download CLI installer, skipping" >&2
    rm -rf "$cli_tmpdir"
    return 1
  fi

  chmod +x "$cli_script"

  cli_sh_args=""
  if [ -n "$cli_version" ]; then
    cli_sh_args="-v $cli_version"
  fi

  if [ -n "$cli_dir" ]; then
    # shellcheck disable=SC2086
    INSTALL_DIR="$cli_dir" sh "$cli_script" $cli_sh_args
  else
    # shellcheck disable=SC2086
    sh "$cli_script" $cli_sh_args
  fi

  cli_exit=$?
  rm -rf "$cli_tmpdir"
  return $cli_exit
}

# --- Argument parsing ---
no_cli=false
cli_version=""
cli_dir=""
engine_version="${VERSION:-}"

while [ $# -gt 0 ]; do
  case "$1" in
    --no-cli)
      no_cli=true
      shift
      ;;
    --cli-version)
      if [ $# -lt 2 ]; then
        err "--cli-version requires a value"
      fi
      cli_version="$2"
      shift 2
      ;;
    --cli-dir)
      if [ $# -lt 2 ]; then
        err "--cli-dir requires a value"
      fi
      cli_dir="$2"
      shift 2
      ;;
    -h|--help)
      cat <<'USAGE'
Usage: install.sh [OPTIONS] [VERSION]

Install the iii engine and CLI.

Options:
  --no-cli              Skip CLI installation
  --cli-version VER     Install specific CLI version
  --cli-dir DIR         Install CLI to specific directory
  -h, --help            Show this help message

Environment variables:
  VERSION               Engine version to install
  BIN_DIR               Engine binary installation directory
  PREFIX                Installation prefix (used if BIN_DIR not set)
  TARGET                Override target triple
  III_USE_GLIBC         Use glibc build on Linux x86_64
  CLI_INSTALL_URL       Override CLI install script URL
USAGE
      exit 0
      ;;
    -*)
      err "unknown option: $1 (use --help for usage)"
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
  err "curl is required"
fi

install_id=$(iii_gen_uuid)
telemetry_id=$(iii_get_or_create_telemetry_id)

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
      err "unsupported architecture: $uname_m"
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
      err "unsupported OS: $uname_s"
      ;;
  esac

  target="$arch-$os"
fi

api_headers="-H Accept:application/vnd.github+json -H X-GitHub-Api-Version:2022-11-28"
github_api() {
  # shellcheck disable=SC2086
  curl -fsSL $api_headers "$1"
}

TAG_PREFIX="v"

if [ -n "$VERSION" ]; then
  echo "installing version: $VERSION"
  _ver="${VERSION#iii/}"
  _ver="${_ver#v}"
  _tag="${TAG_PREFIX}${_ver}"
  api_url="https://api.github.com/repos/$REPO/releases/tags/${_tag}"
  json=$(github_api "$api_url") || err "release tag not found: $VERSION (tried tag: ${_tag})"
else
  echo "installing latest version"
  api_url="https://api.github.com/repos/$REPO/releases?per_page=20"
  json_list=$(github_api "$api_url")
  if command -v jq >/dev/null 2>&1; then
    json=$(printf '%s' "$json_list" \
      | jq -c 'first(.[] | select(.prerelease == false and (.tag_name | startswith("v"))))')
    if [ "$json" = "null" ] || [ -z "$json" ]; then
      err "no stable iii release found"
    fi
  else
    _tag=$(printf '%s' "$json_list" \
      | grep -oE '"tag_name"[[:space:]]*:[[:space:]]*"v[^"]+"' \
      | head -n 1 \
      | sed -E 's/.*"(v[^"]+)".*/\1/')
    if [ -z "$_tag" ]; then
      err "could not determine latest release"
    fi
    api_url="https://api.github.com/repos/$REPO/releases/tags/${_tag}"
    json=$(github_api "$api_url")
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

from_version=$(iii_detect_from_version "$bin_dir/$BIN_NAME")
if [ -n "$from_version" ]; then
  install_event_prefix="upgrade"
  iii_send_event "upgrade_started" \
    "\"install_id\":\"${install_id}\",\"from_version\":\"${from_version}\",\"install_method\":\"sh\",\"target_binary\":\"${BIN_NAME}\"" \
    "$telemetry_id" "$install_id"
else
  install_event_prefix="install"
  iii_send_event "install_started" \
    "\"install_id\":\"${install_id}\",\"install_method\":\"sh\",\"target_binary\":\"${BIN_NAME}\"" \
    "$telemetry_id" "$install_id"
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
  iii_send_event "upgrade_succeeded" \
    "\"install_id\":\"${install_id}\",\"from_version\":\"${from_version}\",\"to_version\":\"${installed_version}\",\"install_method\":\"sh\",\"target_binary\":\"${BIN_NAME}\"" \
    "$telemetry_id" "$install_id"
else
  iii_send_event "install_succeeded" \
    "\"install_id\":\"${install_id}\",\"installed_version\":\"${installed_version}\",\"install_method\":\"sh\",\"target_binary\":\"${BIN_NAME}\"" \
    "$telemetry_id" "$install_id"
fi

iii_export_host_user_id

# Install CLI unless --no-cli was passed
if [ "$no_cli" = false ]; then
  if ! install_cli; then
    echo "warning: CLI installation failed, but engine was installed successfully" >&2
  fi
fi

case ":$PATH:" in
  *":$bin_dir:"*)
    ;;
  *)
    printf 'add %s to your PATH if needed\n' "$bin_dir"
    ;;
 esac

echo ""
echo "If you're new to iii, get started quickly here: https://iii.dev/docs/tutorials/quickstart"
