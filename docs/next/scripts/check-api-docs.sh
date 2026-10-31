#!/usr/bin/env bash
# Drift check for the generated SDK API reference.
#
# Regenerates docs/next/reference/sdk-*.mdx and helpers-*.mdx from the SDK
# sources with the steps of .github/workflows/generate-api-docs.yml, then fails
# when a page differs from the one that was there before. The regenerated pages
# stay in place, ready to commit. Every extraction step is required: a missing
# tool or JSON dump fails the check.
#
#   docs/next/scripts/check-api-docs.sh            # regenerate, fail on a diff
#   docs/next/scripts/check-api-docs.sh --write    # regenerate only
#
# Needs, from the repo root:
#   - pnpm, with the workspace installed (pnpm install --frozen-lockfile);
#     PNPM names another command (PNPM="npx -y pnpm@10.19.0")
#   - uv
#   - cargo with a toolchain that emits rustdoc JSON: nightly by default;
#     RUSTDOC_TOOLCHAIN names another (RUSTDOC_TOOLCHAIN= with
#     RUSTC_BOOTSTRAP=1 uses the default one). rustdoc writes under
#     CARGO_TARGET_DIR (default target/), which the generator reads too.
#   - Go 1.25 or later; GO names the go command
#
# After editing a page's source doc comments, regenerate (--write) and render
# each changed page's skill artifact with
# `iii-skill-render --write docs/next/reference/<page>.mdx`.
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/../../.." && pwd)"
cd "$REPO_ROOT"

WRITE_ONLY=0
case "${1:-}" in
  --write) WRITE_ONLY=1 ;;
  "") ;;
  *) echo "usage: $0 [--write]" >&2; exit 2 ;;
esac

PNPM_CMD="${PNPM:-pnpm}"
GO_CMD="${GO:-go}"
RUSTDOC_TC="${RUSTDOC_TOOLCHAIN-+nightly}"
# PNPM, GO and RUSTDOC_TOOLCHAIN may hold a command with arguments.
# shellcheck disable=SC2086
pnpm_() { $PNPM_CMD "$@"; }
# shellcheck disable=SC2086
go_() { $GO_CMD "$@"; }

PAGES_DIR="docs/next/reference"
BEFORE="$(mktemp -d)"
trap 'rm -rf "$BEFORE"' EXIT
cp "$PAGES_DIR"/sdk-*.mdx "$PAGES_DIR"/helpers-*.mdx "$BEFORE"/

echo "=== SDK API reference drift check ==="

echo "[1/5] TypeDoc JSON (Node, Browser, Helpers)..."
# iii-sdk imports @iii-dev/helpers by subpath; typedoc resolves those types
# from the helpers dist.
pnpm_ --filter @iii-dev/observability build
pnpm_ --filter @iii-dev/helpers build
pnpm_ --filter iii-sdk docs:json
pnpm_ --filter iii-browser-sdk docs:json
pnpm_ --filter @iii-dev/helpers docs:json

echo "[2/5] Python (griffe)..."
(cd sdk/packages/python/iii && uv sync --extra dev --quiet && uv run --with griffe griffe dump iii iii_helpers -d google > api-docs.json)
(cd sdk/packages/python/helpers && uv sync --quiet && uv run --with griffe griffe dump iii_helpers -d google > api-docs.json)

echo "[3/5] Rust (rustdoc JSON)..."
# shellcheck disable=SC2086
cargo $RUSTDOC_TC rustdoc -p iii-sdk --all-features -- -Z unstable-options --output-format json
# shellcheck disable=SC2086
cargo $RUSTDOC_TC rustdoc -p iii-helpers --all-features -- -Z unstable-options --output-format json

echo "[4/5] Go (go/doc)..."
(cd docs/next/scripts/godoc-json && go_ run . -root "$REPO_ROOT/sdk/packages/go/iii" -out "$REPO_ROOT/sdk/packages/go/iii/api-docs.json")

echo "[5/5] MDX..."
pnpm_ tsx docs/next/scripts/generate-api-docs.mts --strict

if [[ "$WRITE_ONLY" == 1 ]]; then
  echo "=== Regenerated $PAGES_DIR/sdk-*.mdx and helpers-*.mdx ==="
  exit 0
fi

stale=0
for page in "$PAGES_DIR"/sdk-*.mdx "$PAGES_DIR"/helpers-*.mdx; do
  name="$(basename "$page")"
  if [[ ! -f "$BEFORE/$name" ]]; then
    echo "::error::$page is new: commit it"
    stale=1
  elif ! diff -q "$BEFORE/$name" "$page" > /dev/null; then
    echo "::error::$page is stale"
    diff -u "$BEFORE/$name" "$page" | head -60 || true
    stale=1
  fi
done

if [[ "$stale" == 1 ]]; then
  echo "The generated SDK reference differs from the SDK sources. The regenerated pages are in place: render their skill artifacts and commit both."
  exit 1
fi
echo "=== SDK API reference matches the SDK sources ==="
