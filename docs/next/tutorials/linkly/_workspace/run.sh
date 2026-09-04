#!/usr/bin/env bash
# End-to-end test for one Linkly chapter, or for every chapter with `all`.
#
#   ./run.sh 1
#   ./run.sh all
#
# TEMPLATES_DIR points at the `iii` directory of a templates checkout.
set -uo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO="$(cd "$HERE/../../../../.." && pwd)"
TEMPLATES_DIR="${TEMPLATES_DIR:-$REPO/../templates-linkly/iii}"
# The queue provider is namespace-scoped from 0.23.0 on. Point III_BIN
# at a build that has it when the installed iii is older.
III_BIN="${III_BIN:-$REPO/target/release/iii}"
[[ -x "$III_BIN" ]] || III_BIN="$(command -v iii)"
NAMESPACE=default
HTTP_PORT=3111
ENGINE_PORT=49134

if [[ ! -d "$TEMPLATES_DIR/linkly" ]]; then
  echo "error: no linkly template at $TEMPLATES_DIR/linkly" >&2
  exit 2
fi

port_open() { nc -z 127.0.0.1 "$1" >/dev/null 2>&1; }

wait_for_port() {
  local port=$1 tries=${2:-120}
  for ((i = 0; i < tries; i++)); do
    port_open "$port" && return 0
    sleep 1
  done
  return 1
}

run_chapter() {
  local chapter=$1
  local work
  work="$(mktemp -d)"
  local project="$work/linkly"
  local log="$work/compose.log"
  local rc=0

  echo "== Ch. $chapter =="

  # A per-run state directory keeps one chapter's engine configuration out of
  # the next one's. The package cache stays shared.
  export III_COMPOSE_STATE_DIR="$work/compose-state"
  mkdir -p "$III_COMPOSE_STATE_DIR"
  mkdir -p "$HOME/.iii/compose/packages"
  ln -s "$HOME/.iii/compose/packages" "$III_COMPOSE_STATE_DIR/packages"

  (cd "$work" && "$III_BIN" project init linkly -t linkly --skip-iii --template-dir "$TEMPLATES_DIR") \
    >"$work/init.log" 2>&1 </dev/null || {
    echo "  FAIL: scaffold" >&2
    sed -n '1,40p' "$work/init.log" >&2
    rm -rf "$work"
    return 1
  }

  python3 "$HERE/enable.py" "$project" "$chapter" || {
    rm -rf "$work"
    return 1
  }

  if ((chapter >= 7)); then
    cp -R "$HERE/browser-stand-in" "$project/"
  fi

  if grep -q "^[^#[:space:]]" "$project/analytics/src/main.py"; then
    (cd "$project/analytics" && python3 -m venv .venv &&
      .venv/bin/pip install --quiet -r requirements.txt) >>"$work/pip.log" 2>&1 || {
      echo "  FAIL: pip install in analytics" >&2
      tail -20 "$work/pip.log" >&2
      rm -rf "$work"
      return 1
    }
  fi

  local entry dir source
  for entry in link:src/index.ts click-streamer:src/index.ts \
    bulk-importer:src/index.ts auth:src/index.ts channel-client:import-links.js \
    browser-stand-in:confirm.js; do
    dir="${entry%%:*}"
    source="${entry#*:}"
    [[ -f "$project/$dir/$source" ]] || continue
    grep -q "^[^/[:space:]]" "$project/$dir/$source" || continue
    (cd "$project/$dir" && npm install --silent) >>"$work/npm.log" 2>&1 || {
      echo "  FAIL: npm install in $dir" >&2
      tail -20 "$work/npm.log" >&2
      rm -rf "$work"
      return 1
    }
  done

  (cd "$project" && exec "$III_BIN" compose --up --file worker-compose.yaml) >"$log" 2>&1 &
  local compose_pid=$!

  if ! wait_for_port "$ENGINE_PORT"; then
    echo "  FAIL: engine never listened on $ENGINE_PORT" >&2
    tail -30 "$log" >&2
    rc=1
  elif ! wait_for_port "$HTTP_PORT"; then
    echo "  FAIL: http worker never listened on $HTTP_PORT" >&2
    tail -30 "$log" >&2
    rc=1
  else
    sleep 5
    PROJECT="$project" NAMESPACE="$NAMESPACE" HTTP="http://127.0.0.1:$HTTP_PORT" \
      III_BIN="$III_BIN" LOG="$log" bash "$HERE/assert/ch$chapter.sh"
    rc=$?
  fi

  kill -TERM "$compose_pid" 2>/dev/null
  local waited=0
  while kill -0 "$compose_pid" 2>/dev/null && ((waited < 60)); do
    sleep 1
    ((waited++))
  done

  if kill -0 "$compose_pid" 2>/dev/null; then
    echo "  FAIL: compose daemon $compose_pid still alive after SIGTERM" >&2
    kill -KILL "$compose_pid" 2>/dev/null
    rc=1
  fi
  wait "$compose_pid" 2>/dev/null

  local port
  for port in "$ENGINE_PORT" "$HTTP_PORT" 3110; do
    if port_open "$port"; then
      echo "  FAIL: port $port still open after shutdown" >&2
      rc=1
    fi
  done

  if ((rc != 0)) && grep -q "queue provider\|queue_provider_namespace_unsupported" "$log" \
    "$III_COMPOSE_STATE_DIR"/*/*/logs/*.log 2>/dev/null; then
    echo "  note: the published queue worker is not namespace-aware; chapters 4-7" >&2
    echo "        need a queue release built against a namespace-aware SDK." >&2
  fi

  if ((rc == 0)); then
    echo "  PASS"
    rm -rf "$work"
  else
    echo "  work kept at $work" >&2
  fi
  return $rc
}

chapters=()
if [[ "${1:-}" == all ]]; then
  chapters=(1 2 3 4 5 6 7)
elif [[ -n "${1:-}" ]]; then
  chapters=("$1")
else
  echo "usage: $0 <chapter|all>" >&2
  exit 2
fi

failed=0
for chapter in "${chapters[@]}"; do
  run_chapter "$chapter" || failed=1
done
exit $failed
