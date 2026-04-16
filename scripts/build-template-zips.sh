#!/bin/bash
# Build template zip files using the CLIs
# This ensures identical zip generation between local dev and CI

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(dirname "$SCRIPT_DIR")"

cd "$REPO_ROOT"

# Build motia template zips
if [ -f "target/release/motia-tools" ]; then
    ./target/release/motia-tools build-zips --template-dir=./templates/motia
elif [ -f "target/debug/motia-tools" ]; then
    ./target/debug/motia-tools build-zips --template-dir=./templates/motia
else
    cargo run --quiet -p motia-tools -- build-zips --template-dir=./templates/motia
fi

# Build iii template zips
if [ -f "target/release/iii-tools" ]; then
    ./target/release/iii-tools build-zips --template-dir=./templates/iii
elif [ -f "target/debug/iii-tools" ]; then
    ./target/debug/iii-tools build-zips --template-dir=./templates/iii
else
    cargo run --quiet -p iii-tools -- build-zips --template-dir=./templates/iii
fi

# Stage the zip files for git
git add templates/motia/*.zip 2>/dev/null || true
git add templates/iii/*.zip 2>/dev/null || true

echo "Template zips staged for commit"
