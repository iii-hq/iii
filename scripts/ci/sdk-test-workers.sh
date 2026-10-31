#!/usr/bin/env bash
# Workers for the SDK integration tests, built from source.
#
#   scripts/ci/sdk-test-workers.sh build
#   scripts/ci/sdk-test-workers.sh fixtures
#
# build
#   Fetches github.com/iii-hq/workers at the commit named in
#   sdk/fixtures/workers-ref and builds the state, http, pubsub, cron and queue
#   workers (cargo debug profile) against this checkout's Rust SDK crates:
#   a [patch.crates-io] override points iii-sdk, iii-helpers, iii-observability
#   and iii-filter at the paths in this repository. The binaries land in
#   $SDK_TEST_WORKERS_DIR/bin, with bin/workers-ref recording the commit they
#   were built from. Network access is needed for the fetch and for crates.io.
#
# fixtures
#   Writes $SDK_TEST_WORKERS_DIR/fixtures/<name>.yaml for every
#   sdk/fixtures/<name>.yaml. Each package://api.workers.iii.dev/<worker>
#   container becomes a path:// container that runs bin/<worker> from its own
#   empty working directory under $SDK_TEST_WORKERS_DIR/run/<name>/<key>; the
#   engine section and the configuration names are copied unchanged, and the
#   fixture's <name>.configuration directory is copied beside the variant.
#   Start a variant with scripts/start-iii.sh exactly like the committed
#   fixture:
#
#     bash scripts/start-iii.sh \
#       --config "$SDK_TEST_WORKERS_DIR/fixtures/config-test.yaml" --port 49199
#
#   Fails when bin/ is missing a worker, when bin/workers-ref differs from
#   sdk/fixtures/workers-ref, or when a fixture names a package worker that
#   build does not produce.
#
# Environment
#   SDK_TEST_WORKERS_DIR  output directory; default <repo>/target/sdk-test-workers
#   WORKERS_REPO          workers repository; default https://github.com/iii-hq/workers.git
#   CARGO_TARGET_DIR      cargo target directory for the workers;
#                         default $SDK_TEST_WORKERS_DIR/cargo-target
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
DIR="${SDK_TEST_WORKERS_DIR:-$ROOT/target/sdk-test-workers}"
mkdir -p "$DIR"
DIR="$(cd "$DIR" && pwd)"
WORKERS_REPO="${WORKERS_REPO:-https://github.com/iii-hq/workers.git}"
REF_FILE="$ROOT/sdk/fixtures/workers-ref"
REGISTRY_PREFIX="package://api.workers.iii.dev/"
# Worker package name == directory in the workers repository == binary name.
WORKERS=(state http pubsub cron queue)

usage() {
  sed -n '2,5p' "${BASH_SOURCE[0]}" | sed 's/^# \{0,1\}//' >&2
  exit 2
}

workers_ref() {
  local ref
  ref="$(tr -d '[:space:]' < "$REF_FILE")"
  if [[ ! "$ref" =~ ^[0-9a-f]{40}$ ]]; then
    echo "sdk-test-workers: $REF_FILE must hold one 40-character commit SHA, found '$ref'" >&2
    exit 1
  fi
  printf '%s' "$ref"
}

checkout_workers() {
  local ref="$1" src="$2"
  if [[ -d "$src/.git" ]] && [[ "$(git -C "$src" rev-parse HEAD 2>/dev/null || true)" == "$ref" ]]; then
    echo "workers checkout at $ref: $src"
    return
  fi
  rm -rf "$src"
  mkdir -p "$src"
  git -C "$src" init -q
  git -C "$src" remote add origin "$WORKERS_REPO"
  # Only the built workers and the shared crates they path-depend on;
  # top-level files (rust-toolchain.toml) come with cone mode.
  git -C "$src" sparse-checkout set --cone crates "${WORKERS[@]}"
  git -C "$src" fetch -q --depth 1 --filter=blob:none origin "$ref"
  git -C "$src" checkout -q --detach FETCH_HEAD
  echo "workers checkout at $ref: $src"
}

build() {
  local ref src target patch w
  ref="$(workers_ref)"
  src="$DIR/src"
  target="${CARGO_TARGET_DIR:-$DIR/cargo-target}"
  patch="$DIR/cargo-patch.toml"

  checkout_workers "$ref" "$src"

  cat > "$patch" <<EOF
# Written by scripts/ci/sdk-test-workers.sh: resolve the iii crates the
# workers take from crates.io to this iii checkout.
[patch.crates-io]
iii-sdk = { path = "$ROOT/sdk/packages/rust/iii" }
iii-helpers = { path = "$ROOT/sdk/packages/rust/helpers" }
iii-observability = { path = "$ROOT/sdk/packages/rust/observability" }
iii-filter = { path = "$ROOT/crates/iii-filter" }
EOF

  # Workers with an injectable console UI embed ui/dist at compile time. The
  # SDK tests never load it, so a minimal module and stylesheet stand in for
  # the pnpm build. Files are rewritten only when their content differs, so
  # an unchanged checkout stays fresh for cargo.
  for w in "${WORKERS[@]}"; do
    if [[ -f "$src/$w/ui/package.json" ]]; then
      mkdir -p "$src/$w/ui/dist"
      write_if_changed "$src/$w/ui/dist/page.js" "export {};"
      write_if_changed "$src/$w/ui/dist/styles.css" "[data-iii-ui=\"$w\"] {}"
    fi
  done

  # The workers repository pins its toolchain in rust-toolchain.toml; cargo
  # run from inside the checkout uses it.
  local channel
  channel="$(sed -n 's/^channel[[:space:]]*=[[:space:]]*"\(.*\)"/\1/p' "$src/rust-toolchain.toml" 2>/dev/null || true)"
  if [[ -n "$channel" ]] && command -v rustup >/dev/null 2>&1; then
    rustup toolchain install "$channel" --profile minimal
  fi

  mkdir -p "$DIR/bin"
  rm -f "$DIR/bin/workers-ref"
  for w in "${WORKERS[@]}"; do
    echo "::group::cargo build $w"
    (
      cd "$src/$w"
      CARGO_TARGET_DIR="$target" \
      SKIP_UI_BUILD=1 \
      CARGO_INCREMENTAL="${CARGO_INCREMENTAL:-0}" \
      CARGO_PROFILE_DEV_DEBUG="${CARGO_PROFILE_DEV_DEBUG:-0}" \
        cargo build --config "$patch" --bin "$w"
    )
    echo "::endgroup::"
    install -m 0755 "$target/debug/$w" "$DIR/bin/$w"
  done
  printf '%s\n' "$ref" > "$DIR/bin/workers-ref"
  echo "built ${WORKERS[*]} at $ref into $DIR/bin"
}

write_if_changed() {
  local file="$1" content="$2"
  if [[ ! -f "$file" ]] || [[ "$(cat "$file")" != "$content" ]]; then
    printf '%s\n' "$content" > "$file"
  fi
}

fixtures() {
  local ref built w fixture name out run
  ref="$(workers_ref)"
  for w in "${WORKERS[@]}"; do
    if [[ ! -f "$DIR/bin/$w" ]]; then
      echo "sdk-test-workers: $DIR/bin/$w is missing; run '$0 build' first" >&2
      exit 1
    fi
    # Artifact downloads drop the executable bit.
    chmod +x "$DIR/bin/$w"
  done
  built="$(tr -d '[:space:]' < "$DIR/bin/workers-ref" 2>/dev/null || true)"
  if [[ "$built" != "$ref" ]]; then
    echo "sdk-test-workers: $DIR/bin was built from workers '${built:-unknown}', sdk/fixtures/workers-ref names $ref" >&2
    exit 1
  fi

  mkdir -p "$DIR/fixtures"
  for fixture in "$ROOT"/sdk/fixtures/*.yaml; do
    name="$(basename "$fixture" .yaml)"
    out="$DIR/fixtures/$name.yaml"
    run="$DIR/run/$name"
    rm -rf "$run" "$DIR/fixtures/$name.configuration"
    awk -v prefix="$REGISTRY_PREFIX" -v bin="$DIR/bin" -v run="$run" \
        -v built=" ${WORKERS[*]} " -v fixture="$fixture" \
        -f <(rewrite_program) "$fixture" > "$out"
    if [[ -d "$ROOT/sdk/fixtures/$name.configuration" ]]; then
      cp -R "$ROOT/sdk/fixtures/$name.configuration" "$DIR/fixtures/$name.configuration"
    fi
    # One empty working directory per container: a path:// worker runs in
    # its directory, and an empty one carries no iii.worker.yaml.
    sed -n 's|^    worker: path://||p' "$out" | while read -r dir; do mkdir -p "$dir"; done
    echo "wrote $out"
  done
}

# Rewrites the containers section of one fixture. A container block is the
# two-space key line plus its four-space (and deeper) lines. In a block whose
# worker is a built package, the worker line becomes a path:// source with a
# scripts.run that execs the binary, and the version line is dropped.
rewrite_program() {
  cat <<'AWK'
function flush(   i, line, pkg) {
  if (n == 0) return
  if (worker != "") {
    pkg = substr(worker, length(prefix) + 1)
    if (index(built, " " pkg " ") == 0) {
      printf "%s: container %s uses %s%s, which scripts/ci/sdk-test-workers.sh does not build\n", fixture, key, prefix, pkg > "/dev/stderr"
      failed = 1
    }
  }
  for (i = 1; i <= n; i++) {
    line = block[i]
    if (worker != "" && line ~ /^    worker:/) {
      print "    worker: path://" run "/" key
      print "    scripts:"
      print "      run: exec '" bin "/" pkg "'"
      continue
    }
    if (worker != "" && line ~ /^    version:/) continue
    print line
  }
  n = 0; worker = ""; key = ""
}
/^[^ #]/ { flush(); in_containers = ($0 ~ /^containers:[ \t]*$/); print; next }
in_containers && /^  [^ #]/ {
  flush()
  key = $1; sub(/:$/, "", key)
}
in_containers && key != "" {
  block[++n] = $0
  if ($0 ~ /^    worker:[ \t]*/) {
    w = $0; sub(/^    worker:[ \t]*/, "", w); gsub(/["']/, "", w)
    if (index(w, prefix) == 1) worker = w
  }
  next
}
{ print }
END { flush(); if (failed) exit 1 }
AWK
}

case "${1:-}" in
  build) build ;;
  fixtures) fixtures ;;
  *) usage ;;
esac
