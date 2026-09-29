#!/usr/bin/env bash
# Regenerate the committed CLI reference page from the clap definitions.
#
# Each user-facing binary in this repo (iii and iii-console) exposes
# a hidden `gen-cli-docs` subcommand that renders its own clap tree as MDX via
# crates/iii-clap-docs. The engine emits the full page (frontmatter + intro);
# console emits a fragment, concatenated below it as a sibling `##`
# sections of one combined page. The output is committed at
# docs/next/cli-reference/index.mdx and the cli-docs-built CI job regenerates
# + diffs it, so the docs can never drift from the CLI. (iii-cloud lives
# outside this repo and is not covered.)
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$REPO_ROOT"

OUT_DIR="docs/next/cli-reference"
OUT_FILE="$OUT_DIR/index.mdx"

TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

echo "=== CLI Reference Generation ==="

echo "[1/3] iii (engine)..."
cargo run --quiet -p iii -- gen-cli-docs --out "$TMP/iii.mdx"

echo "[2/3] iii console..."
# Placeholder assets are fine; gen-cli-docs never serves the frontend.
SKIP_FRONTEND_BUILD=1 cargo run --quiet -p iii-console -- gen-cli-docs --out "$TMP/iii-console.mdx"

mkdir -p "$OUT_DIR"
# The Telemetry section is hand-authored prose, not a clap tree, so it is
# appended here after both generated fragments to sit at the bottom of the
# page. Keep it in sync with the engine and CLI/Compose product-usage gates
# (`workers::telemetry::check_disabled` and `cli::telemetry::is_telemetry_disabled`).
{
  cat "$TMP/iii.mdx"
  echo
  cat "$TMP/iii-console.mdx"
  cat <<'TELEMETRY_MDX'

## Telemetry

The engine sends anonymous usage data by default. This data helps to improve iii. It contains no personal information unless you choose to enter your email address when you sign up. In that case, the engine attaches that email address to your usage profile.

`iii compose` also reports its own usage data, such as whether a run succeeded, how long it took, how many containers it managed, and a fixed error code if it failed. These reports never include file paths, container names, worker references, or error messages.

To turn the usage data off, do one of these:

- Set `III_TELEMETRY_ENABLED` to `false`, `0`, `no`, or `off` before you start `iii`. Letter case does not matter, and leading or trailing spaces are ignored. Any other value, or no value, keeps the usage data on. This disables both engine and `iii compose` product-usage reports.
- Create the file `~/.iii/telemetry_dev_optout`. The engine and `iii compose` read this marker whenever the process starts.
- Set `telemetry.enabled: false` in the engine configuration. This setting applies only to engine telemetry.

The engine and `iii compose` also turn their product-usage data off automatically if they detect that they are running in a CICD environment. When engine telemetry is off, the engine discards usage reports from workers instead of storing them.

This setting controls product-usage data only. It does not change OpenTelemetry observability (traces, metrics, and logs) for your own monitoring of your iii system.
TELEMETRY_MDX
} > "$OUT_FILE"

# Re-render the per-doc skill artifact (<page>.mdx.skill.md) that the
# skill-check workflow verifies. Optional locally; CI's skill-check job is
# the authority.
echo "[3/3] skill artifact..."
if command -v iii-skill-render &>/dev/null; then
  iii-skill-render --write "$OUT_FILE"
else
  echo "  [SKIP] iii-skill-render not found; skill-check CI will report if the artifact is stale"
fi

echo "=== Done: $OUT_FILE ==="
