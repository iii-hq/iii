#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
BATS_DIR="${SCRIPT_DIR}/.bats"

# Install bats-core locally if not present
if [ ! -x "${BATS_DIR}/bin/bats" ]; then
  echo "Installing bats-core locally..."
  rm -rf "$BATS_DIR"
  git clone --depth 1 https://github.com/bats-core/bats-core.git "$BATS_DIR" 2>/dev/null
fi

echo "Running install.sh test suite..."
"${BATS_DIR}/bin/bats" "${SCRIPT_DIR}/install.bats" "$@"
