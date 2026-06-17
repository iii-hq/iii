#!/usr/bin/env bash
# Regenerate the embedded golden ext4 upper image:
# crates/iii-worker/vendor/upper-golden.iiu.gz
#
# The golden is an EMPTY ext4 filesystem that every worker's writable overlay
# upper is stamped from (see crates/iii-worker/src/cli/upper.rs). It is
# arch-independent (ext4's on-disk format is little-endian regardless of the
# CPU that formats it), so this one artifact serves every guest architecture.
#
# Pipeline:
#   1. Docker (any arch) builds a sparse, empty ext4 image via mke2fs.
#   2. The `gen-golden` example extracts only the non-zero blocks and writes
#      the compressed sparse-extent format that upper.rs reads at runtime.
#
# Requirements: Docker (for mke2fs only) + a Rust toolchain. No host ext4/zstd
# tooling needed; consuming the artifact needs neither (pure-Rust decode).
#
# Bump SIZE_GIB only with care: it fixes the upper's capacity (ext4 can't grow
# without resize2fs). 16 GiB sparse costs nothing until written.
set -euo pipefail

SIZE_GIB="${SIZE_GIB:-16}"
HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
CRATE_DIR="$(cd "$HERE/.." && pwd)"
RAW="$HERE/raw-golden.ext4"
OUT="$HERE/upper-golden.iiu.gz"

cleanup() { rm -f "$RAW"; }
trap cleanup EXIT

echo "==> building empty ${SIZE_GIB} GiB ext4 via Docker (mke2fs)"
docker run --rm -v "$HERE:/out" alpine:3.20 sh -c "
  apk add --no-cache e2fsprogs >/dev/null 2>&1 &&
  rm -f /out/raw-golden.ext4 &&
  truncate -s ${SIZE_GIB}G /out/raw-golden.ext4 &&
  mke2fs -F -q -t ext4 -L iii-upper -m 0 /out/raw-golden.ext4
"

echo "==> extracting non-zero blocks -> $OUT"
cargo run -q --manifest-path "$CRATE_DIR/Cargo.toml" --example gen-golden -- "$RAW" "$OUT"

echo "==> done"
ls -l "$OUT"
