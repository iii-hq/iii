#!/usr/bin/env bash
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$REPO_ROOT"

# Mirrors .github/workflows/generate-api-docs.yml — keep the two in sync.

echo "=== SDK API Docs Generation Pipeline ==="
echo ""

# Step 1: TypeDoc JSON (Node SDK, Browser SDK, Helpers)
echo "[1/4] Extracting Node / Browser / Helpers docs (TypeDoc)..."
pnpm --filter iii-sdk docs:json
pnpm --filter iii-browser-sdk docs:json
pnpm --filter @iii-dev/helpers docs:json
echo "  Done: sdk/packages/node/{iii,iii-browser,helpers}/api-docs.json"

# Step 2: Python SDK + Helpers (griffe)
echo "[2/4] Extracting Python docs (griffe)..."
if command -v uv &>/dev/null; then
  cd sdk/packages/python/iii
  uv sync --extra dev --quiet
  # Dump iii_helpers too so types the SDK re-exports from it (e.g.
  # EnqueueResult) resolve and get documented on the SDK page.
  uv run --with griffe griffe dump iii iii_helpers -d google > api-docs.json
  cd "$REPO_ROOT/sdk/packages/python/helpers"
  uv sync --quiet
  uv run --with griffe griffe dump iii_helpers -d google > api-docs.json
  cd "$REPO_ROOT"
  echo "  Done: sdk/packages/python/{iii,helpers}/api-docs.json"
else
  echo "  [SKIP] uv not found. Install uv (https://docs.astral.sh/uv/)."
fi

# Step 3: Rust SDK + Helpers (nightly rustdoc JSON)
echo "[3/4] Extracting Rust docs (rustdoc JSON)..."
if rustup toolchain list | grep -q nightly; then
  cargo +nightly rustdoc -p iii-sdk --all-features -- -Z unstable-options --output-format json || \
    echo "  [WARN] rustdoc JSON generation failed for iii-sdk (nightly may be incompatible)"
  cargo +nightly rustdoc -p iii-helpers --all-features -- -Z unstable-options --output-format json || \
    echo "  [WARN] rustdoc JSON generation failed for iii-helpers (nightly may be incompatible)"
  echo "  Done: target/doc/{iii_sdk,iii_helpers}.json"
else
  echo "  [SKIP] Rust nightly not installed. Run: rustup toolchain install nightly"
fi

# Step 4: Generate MDX (writes to docs/next/api-reference/)
echo "[4/4] Generating MDX files..."
pnpm tsx docs/next/scripts/generate-api-docs.mts

echo ""
echo "=== Done ==="
