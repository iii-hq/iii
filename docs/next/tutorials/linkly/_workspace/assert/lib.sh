# Shared helpers for the chapter assertions.
fails=0

check() {
  local label=$1 expected=$2 actual=$3
  if [[ "$actual" == *"$expected"* ]]; then
    echo "  ok   $label"
  else
    echo "  FAIL $label: expected '$expected' in '$actual'" >&2
    fails=1
  fi
}

# Project functions live in the project namespace (`default` unless the run
# moved it); -n reaches them there. engine::* builtins always answer in default.
t() {
  local ns="${NAMESPACE:-default}"
  [[ "$1" == engine::* ]] && ns=default
  "${III_BIN:-iii}" call -n "$ns" --engine "${III_URL:-ws://127.0.0.1:49134}" "$@" 2>&1
}

# POST /links with a code that already exists: 409 and link::create's own
# message, with no SDK prefix and no database error text.
check_duplicate() {
  local code=$1 actual
  actual="$(curl -s -w ' %{http_code}' -X POST "$HTTP/links" -H 'Content-Type: application/json' \
    -d "{\"url\":\"https://example.com\",\"code\":\"$code\"}")"
  check "POST /links duplicate $code: 409 with a clean body" \
    "{\"error\":\"code \`$code\` is already taken\"} 409" "$actual"
  echo "       body and status: $actual"
}

# POST /links without a url: 400 from http::create, before link::create runs.
check_missing_url() {
  local actual
  actual="$(curl -s -w ' %{http_code}' -X POST "$HTTP/links" -H 'Content-Type: application/json' \
    -d '{"code":"nourl"}')"
  check "POST /links without url: 400" '{"error":"missing \"url\""} 400' "$actual"
  echo "       body and status: $actual"
}

finish() { return $fails; }
