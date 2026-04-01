#!/usr/bin/env bash
# Mock generators for install.sh tests.
# Each create_mock_* writes an executable shell script into $MOCK_BIN_DIR.

# ---------------------------------------------------------------------------
# Mock curl
#
# Three modes based on argument inspection:
#   1. POST request (has -X POST)      → Amplitude telemetry; logs to MOCK_CURL_TELEMETRY_LOG
#   2. -o <file> (not /dev/null)        → Asset download; copies MOCK_CURL_DOWNLOAD_SRC
#   3. Otherwise                        → GitHub API; returns fixture from MOCK_CURL_RESPONSES
#
# Env-var failure injection:
#   MOCK_CURL_EXIT           → force ALL curl calls to fail
#   MOCK_CURL_DOWNLOAD_EXIT  → fail only download calls
#   MOCK_CURL_API_EXIT       → fail only API calls
#   MOCK_CURL_TELEMETRY_EXIT → fail only telemetry POST calls
# ---------------------------------------------------------------------------
create_mock_curl() {
  cat > "${MOCK_BIN_DIR}/curl" <<'MOCKCURL'
#!/usr/bin/env sh
_output_file=""
_url=""
_is_post=0
_data_raw=""
_prev=""

for arg in "$@"; do
  case "$_prev" in
    -o) _output_file="$arg"; _prev=""; continue ;;
    -X) case "$arg" in POST|post) _is_post=1 ;; esac; _prev=""; continue ;;
    -H|--data-raw) case "$_prev" in --data-raw) _data_raw="$arg" ;; esac; _prev=""; continue ;;
  esac
  case "$arg" in
    -o|-X|-H|--data-raw) _prev="$arg"; continue ;;
    -*) continue ;;
    http*) _url="$arg" ;;
  esac
done

# Global failure override
if [ -n "${MOCK_CURL_EXIT:-}" ]; then
  exit "$MOCK_CURL_EXIT"
fi

# --- Telemetry POST ---
if [ "$_is_post" = "1" ]; then
  if [ -n "${MOCK_CURL_TELEMETRY_EXIT:-}" ]; then
    exit "$MOCK_CURL_TELEMETRY_EXIT"
  fi
  if [ -n "${MOCK_CURL_TELEMETRY_LOG:-}" ]; then
    _etype=""
    _etype=$(printf '%s' "$_data_raw" | grep -oE '"event_type":"[^"]*"' | head -1 | cut -d'"' -f4 2>/dev/null || true)
    printf 'event_type=%s url=%s payload=%s\n' "$_etype" "$_url" "$_data_raw" >> "$MOCK_CURL_TELEMETRY_LOG"
  fi
  exit 0
fi

# --- Asset download (-o <file>, not /dev/null) ---
if [ -n "$_output_file" ] && [ "$_output_file" != "/dev/null" ]; then
  if [ -n "${MOCK_CURL_DOWNLOAD_EXIT:-}" ]; then
    exit "$MOCK_CURL_DOWNLOAD_EXIT"
  fi
  if [ -z "${MOCK_CURL_DOWNLOAD_SRC:-}" ] || [ ! -f "$MOCK_CURL_DOWNLOAD_SRC" ]; then
    exit 22
  fi
  cp "$MOCK_CURL_DOWNLOAD_SRC" "$_output_file"
  exit 0
fi

# --- GitHub API (stdout) ---
if [ -n "${MOCK_CURL_API_EXIT:-}" ]; then
  exit "$MOCK_CURL_API_EXIT"
fi
_hash=$(printf '%s' "$_url" | cksum | cut -d' ' -f1)
if [ -f "${MOCK_CURL_RESPONSES}/${_hash}" ]; then
  cat "${MOCK_CURL_RESPONSES}/${_hash}"
elif [ -f "${MOCK_CURL_RESPONSES}/_default" ]; then
  cat "${MOCK_CURL_RESPONSES}/_default"
else
  exit 22
fi
MOCKCURL
  chmod +x "${MOCK_BIN_DIR}/curl"
}

# Register a fixture as the response for a given URL (or "*" for default)
setup_curl_api_response() {
  local url_or_star="$1" fixture_file="$2"
  if [ "$url_or_star" = "*" ]; then
    cp "$fixture_file" "${MOCK_CURL_RESPONSES}/_default"
  else
    local _hash
    _hash=$(printf '%s' "$url_or_star" | cksum | cut -d' ' -f1)
    cp "$fixture_file" "${MOCK_CURL_RESPONSES}/${_hash}"
  fi
}

setup_curl_download() {
  export MOCK_CURL_DOWNLOAD_SRC="$1"
}

# ---------------------------------------------------------------------------
# Mock uname
# ---------------------------------------------------------------------------
create_mock_uname() {
  local os="${1:-Darwin}" arch="${2:-x86_64}"
  cat > "${MOCK_BIN_DIR}/uname" <<MOCKUSERCASE
#!/usr/bin/env sh
case "\$1" in
  -s) echo "${os}" ;;
  -m) echo "${arch}" ;;
  *)  echo "${os}" ;;
esac
MOCKUSERCASE
  chmod +x "${MOCK_BIN_DIR}/uname"
}

# ---------------------------------------------------------------------------
# Mock tar — delegates to real tar, with optional failure injection
# ---------------------------------------------------------------------------
create_mock_tar() {
  cat > "${MOCK_BIN_DIR}/tar" <<'MOCKTAR'
#!/usr/bin/env sh
if [ -n "${MOCK_TAR_EXIT:-}" ]; then
  exit "$MOCK_TAR_EXIT"
fi
_real_tar=""
for _p in /usr/bin/tar /bin/tar; do
  if [ -x "$_p" ]; then _real_tar="$_p"; break; fi
done
if [ -n "$_real_tar" ]; then
  "$_real_tar" "$@"
else
  exit 1
fi
MOCKTAR
  chmod +x "${MOCK_BIN_DIR}/tar"
}

# ---------------------------------------------------------------------------
# Mock unzip — delegates to real unzip, with optional failure injection
# ---------------------------------------------------------------------------
create_mock_unzip() {
  cat > "${MOCK_BIN_DIR}/unzip" <<'MOCKUNZIP'
#!/usr/bin/env sh
if [ -n "${MOCK_UNZIP_EXIT:-}" ]; then
  exit "$MOCK_UNZIP_EXIT"
fi
_real_unzip=""
for _p in /usr/bin/unzip /bin/unzip; do
  if [ -x "$_p" ]; then _real_unzip="$_p"; break; fi
done
if [ -n "$_real_unzip" ]; then
  "$_real_unzip" "$@"
else
  exit 1
fi
MOCKUNZIP
  chmod +x "${MOCK_BIN_DIR}/unzip"
}

remove_mock_unzip() {
  rm -f "${MOCK_BIN_DIR}/unzip"
}

# ---------------------------------------------------------------------------
# Mock install command
# ---------------------------------------------------------------------------
create_mock_install_cmd() {
  cat > "${MOCK_BIN_DIR}/install" <<'MOCKINSTALL'
#!/usr/bin/env sh
if [ -n "${MOCK_INSTALL_EXIT:-}" ]; then
  exit "$MOCK_INSTALL_EXIT"
fi
_src="" _dst="" _mode=""
_prev=""
for arg in "$@"; do
  case "$_prev" in
    -m) _mode="$arg"; _prev=""; continue ;;
  esac
  case "$arg" in
    -m) _prev="-m" ;;
    *)
      if [ -z "$_src" ]; then _src="$arg"
      else _dst="$arg"
      fi ;;
  esac
done
cp "$_src" "$_dst"
chmod "${_mode:-755}" "$_dst"
MOCKINSTALL
  chmod +x "${MOCK_BIN_DIR}/install"
}

remove_mock_install_cmd() {
  rm -f "${MOCK_BIN_DIR}/install"
}

# ---------------------------------------------------------------------------
# Mock ldd (for glibc version detection)
# ---------------------------------------------------------------------------
create_mock_ldd() {
  local version="${1:-2.35}"
  cat > "${MOCK_BIN_DIR}/ldd" <<MOCKLDD
#!/usr/bin/env sh
echo "ldd (GNU libc) ${version}"
MOCKLDD
  chmod +x "${MOCK_BIN_DIR}/ldd"
}

# ---------------------------------------------------------------------------
# Mock jq — delegates to real jq
# ---------------------------------------------------------------------------
create_mock_jq() {
  local real_jq=""
  for p in /usr/bin/jq /usr/local/bin/jq /opt/homebrew/bin/jq; do
    if [ -x "$p" ]; then real_jq="$p"; break; fi
  done
  if [ -z "$real_jq" ]; then
    real_jq="$(command -v jq 2>/dev/null || true)"
  fi
  if [ -z "$real_jq" ]; then
    echo "WARNING: real jq not found, mock jq will not work" >&2
    return 1
  fi
  cat > "${MOCK_BIN_DIR}/jq" <<MOCKJQ
#!/usr/bin/env sh
exec "${real_jq}" "\$@"
MOCKJQ
  chmod +x "${MOCK_BIN_DIR}/jq"
}

remove_mock_jq() {
  rm -f "${MOCK_BIN_DIR}/jq"
}

# ---------------------------------------------------------------------------
# Mock uuidgen — returns deterministic UUIDs
# ---------------------------------------------------------------------------
create_mock_uuidgen() {
  cat > "${MOCK_BIN_DIR}/uuidgen" <<'MOCKUUID'
#!/usr/bin/env sh
echo "00000000-0000-0000-0000-000000000001"
MOCKUUID
  chmod +x "${MOCK_BIN_DIR}/uuidgen"
}

# ---------------------------------------------------------------------------
# Mock iii binary — responds to --version
# ---------------------------------------------------------------------------
create_mock_iii_binary() {
  local dest_dir="$1" version="${2:-0.8.0}"
  mkdir -p "$dest_dir"
  cat > "${dest_dir}/iii" <<MOCKIII
#!/usr/bin/env sh
case "\$*" in
  *--version*)
    echo "iii ${version}"
    exit 0
    ;;
esac
exit 0
MOCKIII
  chmod +x "${dest_dir}/iii"
}

# ---------------------------------------------------------------------------
# Fake archive creation — builds a real .tar.gz containing a mock iii binary
# ---------------------------------------------------------------------------
create_fake_archive() {
  local archive_dir="$1" version="${2:-0.8.0}"
  mkdir -p "$archive_dir"

  local staging="${archive_dir}/_staging"
  mkdir -p "$staging"
  create_mock_iii_binary "$staging" "$version"

  # Build archives for every target
  (cd "$staging" && tar czf "${archive_dir}/iii-x86_64-apple-darwin.tar.gz" iii)
  (cd "$staging" && tar czf "${archive_dir}/iii-aarch64-apple-darwin.tar.gz" iii)
  (cd "$staging" && tar czf "${archive_dir}/iii-x86_64-unknown-linux-musl.tar.gz" iii)
  (cd "$staging" && tar czf "${archive_dir}/iii-aarch64-unknown-linux-gnu.tar.gz" iii)
  (cd "$staging" && tar czf "${archive_dir}/iii-armv7-unknown-linux-gnueabihf.tar.gz" iii)
  (cd "$staging" && tar czf "${archive_dir}/iii-x86_64-unknown-linux-gnu.tar.gz" iii)

  if command -v zip >/dev/null 2>&1; then
    (cd "$staging" && zip -q "${archive_dir}/iii-x86_64-apple-darwin.zip" iii)
  fi

  rm -rf "$staging"
}

# Variant: binary nested inside a subdirectory
create_fake_archive_nested() {
  local archive_dir="$1" version="${2:-0.8.0}"
  mkdir -p "$archive_dir"
  local staging="${archive_dir}/_staging/iii-v${version}"
  mkdir -p "$staging"
  create_mock_iii_binary "$staging" "$version"
  (cd "${archive_dir}/_staging" && tar czf "${archive_dir}/iii-x86_64-apple-darwin.tar.gz" "iii-v${version}/iii")
  rm -rf "${archive_dir}/_staging"
}

# Variant: archive with no binary inside
create_fake_archive_empty() {
  local archive_dir="$1"
  mkdir -p "$archive_dir"
  local staging="${archive_dir}/_staging"
  mkdir -p "$staging"
  echo "not a binary" > "${staging}/README.md"
  (cd "$staging" && tar czf "${archive_dir}/iii-x86_64-apple-darwin.tar.gz" README.md)
  rm -rf "$staging"
}
