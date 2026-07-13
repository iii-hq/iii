#!/usr/bin/env bash
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$REPO_ROOT"

echo "=== SDK API Docs Generation Pipeline ==="
echo ""

# Step 1: TypeDoc JSON (Node SDK, Browser SDK, Helpers)
echo "[1/4] Extracting TypeDoc JSON (Node, Browser, Helpers)..."
pnpm --filter iii-sdk docs:json
pnpm --filter iii-browser-sdk docs:json
pnpm --filter @iii-dev/helpers docs:json

# Step 2: Python (griffe) for iii and iii_helpers
echo "[2/4] Extracting Python docs (griffe)..."
if command -v uv &>/dev/null; then
  (cd sdk/packages/python/iii && uv sync --extra dev --quiet && uv run --with griffe griffe dump iii iii_helpers -d google > api-docs.json)
  (cd sdk/packages/python/helpers && uv sync --quiet && uv run --with griffe griffe dump iii_helpers -d google > api-docs.json)
else
  echo "  [SKIP] uv not found. Install uv: https://docs.astral.sh/uv/"
fi

# Step 3: Rust (nightly rustdoc JSON) for both crates
echo "[3/4] Extracting Rust docs (rustdoc JSON)..."
if rustup toolchain list | grep -q nightly; then
  cargo +nightly rustdoc -p iii-sdk --all-features -- -Z unstable-options --output-format json || \
    echo "  [WARN] iii-sdk rustdoc JSON generation failed (nightly may be incompatible)"
  cargo +nightly rustdoc -p iii-helpers --all-features -- -Z unstable-options --output-format json || \
    echo "  [WARN] iii-helpers rustdoc JSON generation failed (nightly may be incompatible)"
else
  echo "  [SKIP] Rust nightly not installed. Run: rustup toolchain install nightly"
fi

# Step 4: Generate MDX under docs/next/api-reference/
echo "[4/4] Generating MDX files..."
pnpm tsx docs/next/scripts/generate-api-docs.mts

echo ""
echo "=== Done ==="
