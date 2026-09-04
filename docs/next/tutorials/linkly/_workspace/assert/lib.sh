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

t() { "${III_BIN:-iii}" trigger "$@" 2>&1; }

finish() { return $fails; }
