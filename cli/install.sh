#!/usr/bin/env bash
set -euo pipefail

# =============================================================================
# iii-cli installer
# =============================================================================
# Install the iii-cli binary from GitHub releases.
#
# Usage:
#   curl -fsSL https://raw.githubusercontent.com/iii-hq/iii/main/cli/install.sh | bash
#   curl -fsSL https://raw.githubusercontent.com/iii-hq/iii/main/cli/install.sh | bash -s -- -v 0.1.3
# =============================================================================

# --- Constants ----------------------------------------------------------------

REPO="${REPO:-iii-hq/iii}"
BIN_NAME="${BIN_NAME:-iii-cli}"
INSTALL_DIR="${INSTALL_DIR:-$HOME/.local/bin}"
BREW_FORMULA="${BREW_FORMULA:-$BIN_NAME}"

AMPLITUDE_ENDPOINT="https://api2.amplitude.com/2/httpapi"
AMPLITUDE_API_KEY="${III_INSTALL_AMPLITUDE_API_KEY:-e8fb1f8d290a72dbb2d9b264926be4bf}"

# Validate REPO format (owner/repo)
if [[ ! "$REPO" =~ ^[a-zA-Z0-9._-]+/[a-zA-Z0-9._-]+$ ]]; then
  echo "error: REPO must match owner/repo format (got: $REPO)" >&2
  exit 1
fi

# Validate BIN_NAME (no path separators or special characters)
if [[ ! "$BIN_NAME" =~ ^[a-zA-Z0-9_-]+$ ]]; then
  echo "error: BIN_NAME contains invalid characters (got: $BIN_NAME)" >&2
  exit 1
fi

# Validate INSTALL_DIR starts with a safe character (prevent --flag injection)
if [[ ! "$INSTALL_DIR" =~ ^[/~.] ]]; then
  echo "error: INSTALL_DIR must start with /, ~, or . (got: $INSTALL_DIR)" >&2
  exit 1
fi

# Validate INSTALL_DIR contains only safe path characters
if [[ "$INSTALL_DIR" =~ [^a-zA-Z0-9/_.~:@+-] ]]; then
  echo "error: INSTALL_DIR contains invalid characters" >&2
  exit 1
fi

# Validate BREW_FORMULA (allow tap notation like org/tap/formula and @ for versioned formulae)
if [[ ! "$BREW_FORMULA" =~ ^[a-zA-Z0-9_@/.-]+$ ]]; then
  echo "error: BREW_FORMULA contains invalid characters (got: $BREW_FORMULA)" >&2
  exit 1
fi

# Colors
MUTED='\033[0;2m'
RED='\033[0;31m'
ORANGE='\033[38;5;214m'
NC='\033[0m'

# --- Helper functions ---------------------------------------------------------

err() {
  printf "${RED}error:${NC} %s\n" "$*" >&2
  if [[ -n "${install_event_prefix:-}" && -n "${install_id:-}" && -n "${telemetry_id:-}" ]]; then
    local err_msg
    err_msg=$(echo "$*" | tr '"' "'")
    if [[ "$install_event_prefix" == "upgrade" ]]; then
      iii_send_event "upgrade_failed" \
        "\"install_id\":\"${install_id}\",\"from_version\":\"${from_version:-}\",\"install_method\":\"sh\",\"target_binary\":\"${BIN_NAME}\",\"error_stage\":\"${err_msg}\",\"error_message\":\"${err_msg}\"" \
        "$telemetry_id"
    else
      iii_send_event "install_failed" \
        "\"install_id\":\"${install_id}\",\"install_method\":\"sh\",\"target_binary\":\"${BIN_NAME}\",\"error_stage\":\"${err_msg}\",\"error_message\":\"${err_msg}\"" \
        "$telemetry_id"
    fi
    wait
  fi
  exit 1
}

print_message() {
  local level="$1"
  local message="$2"
  local color=""

  case "$level" in
    info)    color="${NC}" ;;
    warning) color="${ORANGE}" ;;
    error)   color="${RED}" ;;
    muted)   color="${MUTED}" ;;
    *)       color="${NC}" ;;
  esac

  printf "${color}%s${NC}\n" "$message"
}

# --- Telemetry helpers --------------------------------------------------------

iii_telemetry_enabled() {
  if [[ "${III_TELEMETRY_ENABLED:-}" == "false" || "${III_TELEMETRY_ENABLED:-}" == "0" ]]; then
    return 1
  fi
  local ci_vars=(CI GITHUB_ACTIONS GITLAB_CI CIRCLECI JENKINS_URL TRAVIS BUILDKITE TF_BUILD CODEBUILD_BUILD_ID BITBUCKET_BUILD_NUMBER DRONE TEAMCITY_VERSION)
  for v in "${ci_vars[@]}"; do
    if [[ -n "${!v:-}" ]]; then
      return 1
    fi
  done
  return 0
}

iii_gen_uuid() {
  if command -v uuidgen >/dev/null 2>&1; then
    uuidgen | tr '[:upper:]' '[:lower:]'
  elif [[ -r /proc/sys/kernel/random/uuid ]]; then
    cat /proc/sys/kernel/random/uuid
  else
    od -x /dev/urandom 2>/dev/null | head -1 | awk '{OFS="-"; print $2$3,$4,$5,$6,$7$8$9}' | head -c 36 || echo "00000000-0000-0000-0000-000000000000"
  fi
}

iii_toml_path() {
  echo "${HOME}/.iii/telemetry.toml"
}

iii_read_toml_key() {
  local section="$1"
  local key="$2"
  local toml_file
  toml_file=$(iii_toml_path)
  if [[ ! -f "$toml_file" ]]; then
    echo ""
    return
  fi
  local in_section=0
  while IFS= read -r line || [[ -n "$line" ]]; do
    line="${line#"${line%%[![:space:]]*}"}"
    line="${line%"${line##*[![:space:]]}"}"
    if [[ "$line" == "[$section]" ]]; then
      in_section=1
    elif [[ "$line" =~ ^\[.*\]$ ]]; then
      in_section=0
    elif [[ "$in_section" == "1" ]]; then
      local lkey lval
      lkey="${line%%=*}"
      lkey="${lkey%"${lkey##*[![:space:]]}"}"
      lval="${line#*=}"
      lval="${lval#"${lval%%[![:space:]]*}"}"
      # Strip surrounding TOML quotes
      lval="${lval#\"}"
      lval="${lval%\"}"
      if [[ "$lkey" == "$key" ]]; then
        echo "$lval"
        return
      fi
    fi
  done < "$toml_file"
  echo ""
}

iii_set_toml_key() {
  local section="$1"
  local key="$2"
  local value="$3"
  local toml_file
  toml_file=$(iii_toml_path)
  mkdir -p "$(dirname "$toml_file")"
  local tmp_file="${toml_file}.tmp"
  local in_target=0
  local key_written=0
  : > "$tmp_file"
  if [[ -f "$toml_file" ]]; then
    while IFS= read -r line || [[ -n "$line" ]]; do
      local trimmed="${line#"${line%%[![:space:]]*}"}"
      trimmed="${trimmed%"${trimmed##*[![:space:]]}"}"
      if [[ "$trimmed" == "[$section]" ]]; then
        printf '%s\n' "$trimmed" >> "$tmp_file"
        in_target=1
      elif [[ "$trimmed" =~ ^\[.*\]$ ]]; then
        if [[ "$in_target" == "1" && "$key_written" == "0" ]]; then
          printf '%s = "%s"\n' "$key" "$value" >> "$tmp_file"
          key_written=1
        fi
        in_target=0
        printf '%s\n' "$trimmed" >> "$tmp_file"
      elif [[ "$in_target" == "1" ]]; then
        local lkey="${trimmed%%=*}"
        lkey="${lkey%"${lkey##*[![:space:]]}"}"
        if [[ "$lkey" == "$key" ]]; then
          printf '%s = "%s"\n' "$key" "$value" >> "$tmp_file"
          key_written=1
        else
          printf '%s\n' "$line" >> "$tmp_file"
        fi
      else
        printf '%s\n' "$line" >> "$tmp_file"
      fi
    done < "$toml_file"
  fi
  if [[ "$key_written" == "0" ]]; then
    if [[ "$in_target" == "1" ]]; then
      printf '%s = "%s"\n' "$key" "$value" >> "$tmp_file"
    else
      printf '\n[%s]\n%s = "%s"\n' "$section" "$key" "$value" >> "$tmp_file"
    fi
  fi
  mv "$tmp_file" "$toml_file"
}

iii_get_or_create_telemetry_id() {
  local existing_id
  existing_id=$(iii_read_toml_key "identity" "id")
  if [[ -n "$existing_id" ]]; then
    echo "$existing_id"
    return
  fi

  local legacy_path="${HOME}/.iii/telemetry_id"
  if [[ -f "$legacy_path" ]]; then
    local legacy_id
    legacy_id=$(cat "$legacy_path" 2>/dev/null | tr -d '[:space:]')
    if [[ -n "$legacy_id" ]]; then
      iii_set_toml_key "identity" "id" "$legacy_id"
      echo "$legacy_id"
      return
    fi
  fi

  mkdir -p "${HOME}/.iii"
  local new_id
  new_id=$(iii_gen_uuid)
  iii_set_toml_key "identity" "id" "$new_id"
  echo "$new_id"
}

iii_send_event() {
  local event_type="$1"
  local event_props="$2"
  local device_id="$3"

  if [[ -z "$AMPLITUDE_API_KEY" ]]; then
    return 0
  fi

  if ! iii_telemetry_enabled; then
    return 0
  fi

  local os_name
  os_name=$(uname -s 2>/dev/null | tr '[:upper:]' '[:lower:]' || echo "unknown")
  local ts
  ts=$(date +%s 2>/dev/null || echo "0")
  local ts_ms=$(( ts * 1000 ))
  local insert_id
  insert_id=$(iii_gen_uuid)

  local payload="{\"api_key\":\"${AMPLITUDE_API_KEY}\",\"events\":[{\"device_id\":\"${device_id}\",\"user_id\":\"${device_id}\",\"event_type\":\"${event_type}\",\"event_properties\":{${event_props}},\"platform\":\"install-script\",\"os_name\":\"${os_name}\",\"app_version\":\"script\",\"time\":${ts_ms},\"insert_id\":\"${insert_id}\",\"ip\":\"\$remote\"}]}"

  curl -s -o /dev/null -X POST "$AMPLITUDE_ENDPOINT" \
    -H "Content-Type: application/json" \
    --data-raw "$payload" &
}

iii_export_host_user_id() {
  local huid
  huid=$(iii_read_toml_key "identity" "id")
  if [[ -z "$huid" ]]; then
    return 0
  fi
  local export_line="export III_HOST_USER_ID=\"${huid}\""
  local profile
  for profile in "${HOME}/.bashrc" "${HOME}/.zshrc" "${HOME}/.profile"; do
    if [[ -f "$profile" ]] && ! grep -qF "III_HOST_USER_ID" "$profile" 2>/dev/null; then
      printf '\n# iii host correlation\n%s\n' "$export_line" >> "$profile"
      break
    fi
  done
}

check_homebrew() {
  # Skip if --force was used
  if [[ "$force_install" == "true" ]]; then
    return 0
  fi

  # Only check if brew is available on the system
  if ! command -v brew >/dev/null 2>&1; then
    return 0
  fi

  # Check if the formula is installed via Homebrew
  if brew list --versions "$BREW_FORMULA" >/dev/null 2>&1; then
    local brew_version
    brew_version=$(brew list --versions "$BREW_FORMULA" | awk '{print $NF}')

    printf "\n"
    printf "${ORANGE}%s is installed via Homebrew${NC}" "$BREW_FORMULA"
    if [[ -n "$brew_version" ]]; then
      printf " ${MUTED}(v%s)${NC}" "$brew_version"
    fi
    printf "\n\n"
    printf "${MUTED}Choose one:${NC}\n"
    printf "  ${NC}1. Update via Homebrew:  ${ORANGE}brew upgrade %s${NC}\n" "$BREW_FORMULA"
    printf "  ${NC}2. Switch to manual:    ${ORANGE}brew uninstall --force %s${NC} then re-run this script\n" "$BREW_FORMULA"
    printf "\n"
    exit 1
  fi
}

# --- Usage / help -------------------------------------------------------------

usage() {
  cat <<EOF
iii-cli installer

Install the iii-cli binary from GitHub releases.

USAGE:
    install.sh [OPTIONS]

OPTIONS:
    -h, --help                  Print this help message
    -v, --version <version>     Install a specific version (e.g. 0.1.3)
    -b, --binary <path>         Install from a local binary instead of downloading
    --no-modify-path            Skip adding the install directory to PATH
    --force                     Skip Homebrew conflict check

ENVIRONMENT VARIABLES:
    REPO            GitHub repository          (default: iii-hq/iii)
    BIN_NAME        Binary name                (default: iii-cli)
    INSTALL_DIR     Installation directory      (default: \$HOME/.local/bin)
    TARGET          Override platform target    (e.g. aarch64-apple-darwin)
    VERSION         Version to install          (same as -v/--version)
    BREW_FORMULA    Homebrew formula name       (default: \$BIN_NAME)

EXAMPLES:
    # Install latest version
    curl -fsSL https://raw.githubusercontent.com/iii-hq/iii/main/cli/install.sh | bash

    # Install specific version
    curl -fsSL https://raw.githubusercontent.com/iii-hq/iii/main/cli/install.sh | bash -s -- -v 0.1.3

    # Install to custom directory
    curl -fsSL https://raw.githubusercontent.com/iii-hq/iii/main/cli/install.sh | INSTALL_DIR=/usr/local/bin bash

    # Install from local binary
    ./install.sh -b ./target/release/iii-cli
EOF
  exit 0
}

# --- CLI argument parsing -----------------------------------------------------

requested_version="${VERSION:-}"
no_modify_path=false
binary_path=""
force_install=false

while [[ $# -gt 0 ]]; do
  case "$1" in
    -h|--help)
      usage
      ;;
    -v|--version)
      if [[ -z "${2:-}" ]]; then
        err "--version requires an argument"
      fi
      requested_version="$2"
      shift 2
      ;;
    -b|--binary)
      if [[ -z "${2:-}" ]]; then
        err "--binary requires an argument"
      fi
      binary_path="$2"
      shift 2
      ;;
    --no-modify-path)
      no_modify_path=true
      shift
      ;;
    --force)
      force_install=true
      shift
      ;;
    -*)
      print_message warning "Unknown option: $1 (ignoring)"
      shift
      ;;
    *)
      shift
      ;;
  esac
done

# --- Input validation ---------------------------------------------------------

# Validate requested_version format if provided (semver-like with optional pre-release)
if [[ -n "$requested_version" ]]; then
  local_ver="${requested_version#iii/}"
  local_ver="${local_ver#v}"
  if [[ ! "$local_ver" =~ ^[0-9]+\.[0-9]+\.[0-9]+(-[a-zA-Z0-9.]+)?$ ]]; then
    err "invalid version format: $requested_version (expected: X.Y.Z or X.Y.Z-pre)"
  fi
  unset local_ver
fi

# --- Dependency checks --------------------------------------------------------

if ! command -v curl >/dev/null 2>&1; then
  err "curl is required but not found"
fi

# --- Version check ------------------------------------------------------------

check_version() {
  local target_version="${1:-}"
  if [[ -z "$target_version" ]]; then
    return 0
  fi
  if command -v "$BIN_NAME" >/dev/null 2>&1; then
    local installed_version
    installed_version=$("$BIN_NAME" --version 2>/dev/null | awk '{print $NF}' || echo "")
    installed_version="${installed_version#v}"

    if [[ -n "$installed_version" ]]; then
      local existing_path
      existing_path=$(command -v "$BIN_NAME" 2>/dev/null || echo "")
      if [[ "$installed_version" == "$target_version" ]]; then
        printf "${MUTED}Version ${NC}%s${MUTED} already installed at ${NC}%s\n" "$target_version" "$existing_path"
        if [[ -n "$existing_path" && "$(dirname "$existing_path")" != "$INSTALL_DIR" ]]; then
          printf "${MUTED}To install to ${NC}%s${MUTED}, first remove the existing binary:${NC}\n" "$INSTALL_DIR"
          printf "  rm %s\n" "$existing_path"
        fi
        exit 0
      else
        printf "${MUTED}Installed version: ${NC}%s${MUTED}. Upgrading...${NC}\n" "$installed_version"
      fi
    fi
  fi
}

# --- Progress bar functions ---------------------------------------------------

unbuffered_sed() {
  if echo | sed -u -e "" >/dev/null 2>&1; then
    sed -nu "$@"
  elif echo | sed -l -e "" >/dev/null 2>&1; then
    sed -nl "$@"
  else
    local pad
    pad="$(printf "\n%512s" "")"
    sed -ne "s/$/\\${pad}/" "$@"
  fi
}

print_progress() {
  local bytes="$1"
  local length="$2"
  [ "$length" -gt 0 ] || return 0

  local width=50
  local percent=$(( bytes * 100 / length ))
  [ "$percent" -gt 100 ] && percent=100
  local on=$(( percent * width / 100 ))
  local off=$(( width - on ))

  local filled
  filled=$(printf "%*s" "$on" "")
  filled=${filled// /■}
  local empty
  empty=$(printf "%*s" "$off" "")
  empty=${empty// /･}

  printf "\r${ORANGE}%s%s %3d%%${NC}" "$filled" "$empty" "$percent" >&4
}

download_with_progress_supported() {
  command -v mkfifo >/dev/null 2>&1
}

download_with_progress() {
  local url="$1"
  local output="$2"
  local extra_args=("${@:3}")

  # Direct fd 4 to stderr if it's a TTY, otherwise /dev/null
  if [ -t 2 ]; then
    exec 4>&2
  else
    exec 4>/dev/null
  fi

  local fifo_dir
  fifo_dir=$(mktemp -d 2>/dev/null || mktemp -d -t iii-progress)
  local tracefile="$fifo_dir/progress.trace"

  rm -f "$tracefile"
  mkfifo "$tracefile"

  # Hide cursor
  printf "\033[?25l" >&4

  trap "trap - RETURN; rm -rf \"$fifo_dir\"; printf '\033[?25h' >&4; exec 4>&-" RETURN

  (
    trap '' PIPE
    curl --trace-ascii "$tracefile" -f -s -L --connect-timeout 30 --max-time 300 ${extra_args[@]+"${extra_args[@]}"} -o "$output" "$url"
  ) &
  local curl_pid=$!

  unbuffered_sed \
    -e 'y/ACDEGHLNORTV/acdeghlnortv/' \
    -e '/^0000: content-length:/p' \
    -e '/^<= recv data/p' \
    "$tracefile" | \
  {
    local length=0
    local bytes=0

    while IFS=" " read -r -a line; do
      [ "${#line[@]}" -lt 2 ] && continue
      local tag="${line[0]} ${line[1]}"

      if [ "$tag" = "0000: content-length:" ]; then
        length="${line[2]}"
        length=$(echo "$length" | tr -d '\r')
        bytes=0
      elif [ "$tag" = "<= recv" ]; then
        local size="${line[3]}"
        bytes=$(( bytes + size ))
        if [ "$length" -gt 0 ]; then
          print_progress "$bytes" "$length"
        fi
      fi
    done
  }

  wait $curl_pid
  local ret=$?
  echo "" >&4
  return $ret
}

# --- GitHub API helper --------------------------------------------------------

api_headers=(-H "Accept:application/vnd.github+json" -H "X-GitHub-Api-Version:2022-11-28")

github_api() {
  curl -fsSL "${api_headers[@]}" "$1"
}

# --- Variables set by platform detection / release fetching -------------------

target=""
specific_version=""
asset_url=""

# --- Platform detection & release fetching (skip if --binary) -----------------

if [[ -z "$binary_path" ]]; then

  # Check for Homebrew-managed installation before any network calls
  check_homebrew

  # --- Platform detection -----------------------------------------------------

  if [[ -n "${TARGET:-}" ]]; then
    target="$TARGET"
  else
    uname_s=$(uname -s 2>/dev/null || echo unknown)
    uname_m=$(uname -m 2>/dev/null || echo unknown)

    # OS detection
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

    # Architecture detection
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

    # Rosetta 2 detection on macOS
    # If running x86_64 on macOS but under Rosetta translation, switch to aarch64
    if [[ "$os" == "apple-darwin" && "$arch" == "x86_64" ]]; then
      if [[ "$(sysctl -n sysctl.proc_translated 2>/dev/null || echo 0)" == "1" ]]; then
        print_message info "Rosetta 2 detected, using native aarch64 binary"
        arch="aarch64"
      fi
    fi

    # musl detection on Linux
    if [[ "$os" == "unknown-linux-gnu" ]]; then
      is_musl=false
      if [[ -f /etc/alpine-release ]]; then
        is_musl=true
      elif command -v ldd >/dev/null 2>&1; then
        if ldd --version 2>&1 | grep -qi musl; then
          is_musl=true
        fi
      fi
      if [[ "$is_musl" == "true" ]]; then
        os="unknown-linux-musl"
      fi
    fi

    target="${arch}-${os}"
  fi

  # --- Release fetching -------------------------------------------------------

  json=""

  if [[ -n "$requested_version" ]]; then
    _bare="${requested_version#iii/}"
    _bare="${_bare#v}"
    check_version "$_bare"
    printf "${MUTED}Installing ${NC}%s ${MUTED}version: ${NC}%s\n" "$BIN_NAME" "$requested_version"
    _tag="v${_bare}"
    api_url="https://api.github.com/repos/$REPO/releases/tags/$_tag"
    json=$(github_api "$api_url") || err "release tag not found: $requested_version (tried tag: $_tag)"
  else
    printf "${MUTED}Installing ${NC}%s ${MUTED}latest version${NC}\n" "$BIN_NAME"
    api_url="https://api.github.com/repos/$REPO/releases?per_page=20"
    json_list=$(github_api "$api_url") || err "failed to fetch releases from $REPO"
    if command -v jq >/dev/null 2>&1; then
      json=$(printf '%s' "$json_list" \
        | jq -c 'first(.[] | select(.prerelease == false and (.tag_name | startswith("v"))))')
      [[ "$json" == "null" || -z "$json" ]] && err "no stable iii release found"
    else
      _tag=$(printf '%s' "$json_list" \
        | grep -oE '"tag_name"[[:space:]]*:[[:space:]]*"v[^"]+"' \
        | head -n 1 \
        | sed -E 's/.*"(v[^"]+)".*/\1/')
      [[ -z "$_tag" ]] && err "could not determine latest release"
      api_url="https://api.github.com/repos/$REPO/releases/tags/$_tag"
      json=$(github_api "$api_url") || err "failed to fetch release $_tag"
    fi
  fi

  # Extract version from tag_name (strip leading v)
  if command -v jq >/dev/null 2>&1; then
    specific_version=$(printf '%s' "$json" | jq -r '.tag_name // empty')
  else
    specific_version=$(printf '%s' "$json" \
      | grep -oE '"tag_name"[[:space:]]*:[[:space:]]*"[^"]+"' \
      | sed -E 's/.*"([^"]+)".*/\1/' \
      | head -n 1)
  fi
  specific_version="${specific_version#iii/}"
  specific_version="${specific_version#v}"

  if [[ -z "$specific_version" ]]; then
    err "could not determine version from release response"
  fi

  if [[ -z "$requested_version" ]]; then
    check_version "$specific_version"
  fi

  # Extract asset URL for the target (exclude .sha256 checksum files)
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

  if [[ -z "$asset_url" ]]; then
    echo "" >&2
    print_message error "No release asset found for target: $target"
    echo "" >&2
    echo "Available assets:" >&2
    if command -v jq >/dev/null 2>&1; then
      printf '%s' "$json" | jq -r '.assets[].name' >&2
    else
      printf '%s' "$json" \
        | grep -oE '"browser_download_url"[[:space:]]*:[[:space:]]*"[^"]+"' \
        | sed -E 's/.*"([^"]+)".*/\1/' >&2
    fi
    echo "" >&2
    exit 1
  fi
fi

# --- Download and install function --------------------------------------------

download_and_install() {
  local asset_name
  asset_name=$(basename "$asset_url")

  # Create temp directory with idempotent cleanup trap
  local tmpdir
  tmpdir=$(mktemp -d 2>/dev/null || mktemp -d -t iii-cli-install)
  _iii_cleanup_tmpdir="$tmpdir"
  _iii_cleanup_done=false
  cleanup() {
    if [[ "${_iii_cleanup_done:-false}" == "false" ]]; then
      _iii_cleanup_done=true
      rm -rf "${_iii_cleanup_tmpdir:-}"
    fi
  }
  trap cleanup EXIT INT TERM

  printf "\n${MUTED}Downloading ${NC}%s ${MUTED}v${NC}%s\n" "$BIN_NAME" "$specific_version"

  # Download the asset
  if [[ -t 2 ]] && download_with_progress_supported; then
    download_with_progress "$asset_url" "$tmpdir/$asset_name" || \
    curl -# -fSL "$asset_url" -o "$tmpdir/$asset_name"
  else
    curl -fsSL "$asset_url" -o "$tmpdir/$asset_name"
  fi

  # Verify SHA256 checksum if available
  # Checksum files are named without the archive extension (e.g. foo.sha256, not foo.tar.gz.sha256)
  local checksum_url
  checksum_url=$(echo "$asset_url" | sed -E 's/\.(tar\.gz|tgz|zip)$/.sha256/')
  local checksum_file="$tmpdir/${asset_name}.sha256"
  if curl -fsSL -o "$checksum_file" "$checksum_url" 2>/dev/null; then
    local expected_hash
    expected_hash=$(awk '{print $1}' "$checksum_file")
    local actual_hash=""
    if command -v sha256sum >/dev/null 2>&1; then
      actual_hash=$(sha256sum "$tmpdir/$asset_name" | awk '{print $1}')
    elif command -v shasum >/dev/null 2>&1; then
      actual_hash=$(shasum -a 256 "$tmpdir/$asset_name" | awk '{print $1}')
    fi
    if [[ -n "$actual_hash" ]]; then
      if [[ "$actual_hash" != "$expected_hash" ]]; then
        err "checksum verification failed (expected: $expected_hash, got: $actual_hash)"
      fi
      print_message muted "Checksum verified"
    else
      print_message warning "No sha256sum or shasum available, skipping checksum verification"
    fi
  else
    print_message warning "No checksum file available, skipping verification"
  fi

  # Extract the archive
  case "$asset_name" in
    *.tar.gz|*.tgz)
      if ! command -v tar >/dev/null 2>&1; then
        err "tar is required to extract $asset_name"
      fi
      # Check for path traversal entries before extracting
      if tar -tzf "$tmpdir/$asset_name" | grep -qE '(^|/)\.\.(/|$)'; then
        err "archive contains path traversal entries"
      fi
      tar --no-same-owner -xzf "$tmpdir/$asset_name" -C "$tmpdir"
      ;;
    *.zip)
      if ! command -v unzip >/dev/null 2>&1; then
        err "unzip is required to extract $asset_name"
      fi
      unzip -q "$tmpdir/$asset_name" -d "$tmpdir"
      ;;
    *)
      # Assume it's a raw binary
      chmod +x "$tmpdir/$asset_name"
      ;;
  esac

  # Find the binary in extracted files
  local bin_file=""
  if [[ -f "$tmpdir/$BIN_NAME" ]]; then
    bin_file="$tmpdir/$BIN_NAME"
  else
    bin_file=$(find "$tmpdir" -maxdepth 3 -type f \( -name "$BIN_NAME" -o -name "${BIN_NAME}.exe" \) | head -n 1)
  fi

  if [[ -z "${bin_file:-}" || ! -f "$bin_file" ]]; then
    err "binary '$BIN_NAME' not found in downloaded asset"
  fi

  # Reject symlinks to prevent symlink attacks
  if [[ -L "$bin_file" ]]; then
    err "binary is a symlink, refusing to install"
  fi

  # Install the binary
  mkdir -p "$INSTALL_DIR"

  if command -v install >/dev/null 2>&1; then
    install -m 755 "$bin_file" "$INSTALL_DIR/$BIN_NAME"
  else
    cp "$bin_file" "$INSTALL_DIR/$BIN_NAME"
    chmod 755 "$INSTALL_DIR/$BIN_NAME"
  fi
}

# --- Install from local binary ------------------------------------------------

install_from_binary() {
  if [[ ! -f "$binary_path" ]]; then
    err "binary not found at: $binary_path"
  fi

  mkdir -p "$INSTALL_DIR"

  local dest="$INSTALL_DIR/$BIN_NAME"
  local src
  src=$(cd "$(dirname "$binary_path")" && pwd)/$(basename "$binary_path")

  if [[ "$src" != "$dest" ]]; then
    cp "$binary_path" "$dest"
  fi
  chmod 755 "$dest"

  # Try to extract version from the binary
  specific_version=$("$INSTALL_DIR/$BIN_NAME" --version 2>/dev/null | awk '{print $NF}' || echo "unknown")
  specific_version="${specific_version#v}"

  printf "\n${MUTED}Installing ${NC}%s ${MUTED}from: ${NC}%s\n" "$BIN_NAME" "$binary_path"
}

# --- Telemetry init -----------------------------------------------------------

install_id=$(iii_gen_uuid)
telemetry_id=$(iii_get_or_create_telemetry_id)
install_event_prefix="install"

from_version=""
if [[ -z "$binary_path" ]]; then
  if command -v "$BIN_NAME" >/dev/null 2>&1; then
    from_version=$("$BIN_NAME" --version 2>/dev/null | awk '{print $NF}' || echo "")
    from_version="${from_version#v}"
  fi
fi

if [[ -n "$from_version" ]]; then
  install_event_prefix="upgrade"
  iii_send_event "upgrade_started" \
    "\"install_id\":\"${install_id}\",\"from_version\":\"${from_version}\",\"install_method\":\"sh\",\"target_binary\":\"${BIN_NAME}\"" \
    "$telemetry_id"
else
  iii_send_event "install_started" \
    "\"install_id\":\"${install_id}\",\"install_method\":\"sh\",\"target_binary\":\"${BIN_NAME}\",\"os\":\"$(uname -s 2>/dev/null | tr '[:upper:]' '[:lower:]' || echo unknown)\",\"arch\":\"$(uname -m 2>/dev/null || echo unknown)\"" \
    "$telemetry_id"
fi

# --- Main dispatch ------------------------------------------------------------

if [[ -n "$binary_path" ]]; then
  install_from_binary
else
  download_and_install
fi

installed_version=$("$INSTALL_DIR/$BIN_NAME" --version 2>/dev/null | awk '{print $NF}' || echo "")
installed_version="${installed_version#v}"

if [[ "$install_event_prefix" == "upgrade" ]]; then
  iii_send_event "upgrade_succeeded" \
    "\"install_id\":\"${install_id}\",\"from_version\":\"${from_version}\",\"to_version\":\"${installed_version}\",\"install_method\":\"sh\",\"target_binary\":\"${BIN_NAME}\"" \
    "$telemetry_id"
else
  iii_send_event "install_succeeded" \
    "\"install_id\":\"${install_id}\",\"installed_version\":\"${installed_version}\",\"install_method\":\"sh\",\"target_binary\":\"${BIN_NAME}\"" \
    "$telemetry_id"
fi

iii_export_host_user_id

# --- PATH modification --------------------------------------------------------

add_to_path() {
  local config_file="$1"
  local path_command="$2"

  # Check if already present
  if grep -qF "$INSTALL_DIR" "$config_file" 2>/dev/null; then
    return 0
  fi

  if [[ -w "$config_file" ]]; then
    {
      echo ""
      echo "# iii-cli"
      echo "$path_command"
    } >> "$config_file"
    print_message info "Added $INSTALL_DIR to \$PATH in $config_file"
  else
    print_message warning "Could not write to $config_file. Manually add:"
    print_message info "  $path_command"
  fi
}

if [[ "$no_modify_path" != "true" ]]; then
  XDG_CONFIG_HOME="${XDG_CONFIG_HOME:-$HOME/.config}"
  current_shell=$(basename "${SHELL:-sh}")

  # Only include XDG paths when XDG_CONFIG_HOME differs from the default
  _xdg_extra=""
  if [[ "$XDG_CONFIG_HOME" != "$HOME/.config" ]]; then
    _xdg_extra=true
  fi

  case "$current_shell" in
    fish)
      config_files="$HOME/.config/fish/config.fish"
      [[ -n "$_xdg_extra" ]] && config_files="$config_files $XDG_CONFIG_HOME/fish/config.fish"
      ;;
    zsh)
      config_files="$HOME/.zshrc $HOME/.zshenv"
      [[ -n "$_xdg_extra" ]] && config_files="$config_files $XDG_CONFIG_HOME/zsh/.zshrc $XDG_CONFIG_HOME/zsh/.zshenv"
      ;;
    bash)
      config_files="$HOME/.bashrc $HOME/.bash_profile $HOME/.profile"
      [[ -n "$_xdg_extra" ]] && config_files="$config_files $XDG_CONFIG_HOME/bash/.bashrc $XDG_CONFIG_HOME/bash/.bash_profile"
      ;;
    ash)
      config_files="$HOME/.ashrc $HOME/.profile /etc/profile"
      ;;
    sh)
      config_files="$HOME/.profile /etc/profile"
      ;;
    *)
      config_files="$HOME/.bashrc $HOME/.bash_profile $HOME/.profile"
      ;;
  esac
  unset _xdg_extra

  config_file=""
  # shellcheck disable=SC2086  # Intentional word-splitting for file list
  for file in $config_files; do
    if [[ -f "$file" ]]; then
      config_file="$file"
      break
    fi
  done

  if [[ ":$PATH:" != *":$INSTALL_DIR:"* ]]; then
    if [[ -n "$config_file" ]]; then
      case "$current_shell" in
        fish)
          add_to_path "$config_file" "fish_add_path $INSTALL_DIR"
          ;;
        *)
          add_to_path "$config_file" "export PATH=\"$INSTALL_DIR:\$PATH\""
          ;;
      esac
    else
      print_message warning "No shell config file found. Manually add to your PATH:"
      case "$current_shell" in
        fish)
          print_message info "  fish_add_path $INSTALL_DIR"
          ;;
        *)
          print_message info "  export PATH=\"$INSTALL_DIR:\$PATH\""
          ;;
      esac
    fi
  fi

  # GitHub Actions: append to $GITHUB_PATH
  if [[ -n "${GITHUB_ACTIONS:-}" && "${GITHUB_ACTIONS}" == "true" ]]; then
    if [[ -n "${GITHUB_PATH:-}" ]]; then
      echo "$INSTALL_DIR" >> "$GITHUB_PATH"
      print_message info "Added $INSTALL_DIR to \$GITHUB_PATH"
    fi
  fi
fi

# --- Post-install branding ----------------------------------------------------

if [[ -x "$INSTALL_DIR/$BIN_NAME" ]]; then
  printf "\n"
  printf "${MUTED}▀ ▀ ▀  ${NC}█▀▀▀ █    ▀█▀\n"
  printf "${MUTED}█ █ █  ${NC}█    █     █\n"
  printf "${MUTED}▀ ▀ ▀  ${NC}▀▀▀▀ ▀▀▀▀ ▀▀▀\n"
  printf "\n"
  printf "${MUTED}Get started:${NC}\n"
  printf "\n"
  printf "  iii-cli console          ${MUTED}# Launch iii-console${NC}\n"
  printf "  iii-cli create           ${MUTED}# Create new project${NC}\n"
  printf "  iii-cli sdk motia        ${MUTED}# Run motia CLI${NC}\n"
  printf "  iii-cli --help           ${MUTED}# See all commands${NC}\n"
  printf "\n"
  printf "${MUTED}Installed to: ${NC}%s/%s\n" "$INSTALL_DIR" "$BIN_NAME"
  printf "\n"
  printf "\n"
else
  err "installation failed: binary not executable at $INSTALL_DIR/$BIN_NAME"
fi
