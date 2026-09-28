#!/usr/bin/env bash
# Build on a native Linux runner of the requested architecture. Do not use a
# fully static musl worker: msb_krun needs dlopen to load the guest firmware.
set -euo pipefail

root=$(cd "$(dirname "$0")/.." && pwd)
target=${1:?usage: build-worker-musl.sh <x86_64|aarch64>-unknown-linux-musl}
case "$target:$(uname -m)" in
  x86_64-unknown-linux-musl:x86_64|aarch64-unknown-linux-musl:aarch64) ;;
  *) echo "musl worker builds require a native runner for $target" >&2; exit 1 ;;
esac

# Both releases and PR checks use the same toolchain and Alpine baseline.
image=rust:1.98.1-alpine3.22
mkdir -p "$root/target/worker-musl/cargo"
docker run --rm \
  -v "$root:/src" -w /src \
  -v "$root/target/worker-musl/cargo:/usr/local/cargo/registry" \
  -e BUILD_TARGET="$target" \
  -e CARGO_BUILD_JOBS="${CARGO_BUILD_JOBS:-4}" \
  "$image" sh -ec '
    apk add --no-cache build-base linux-headers pkgconf libcap-ng-dev libcap-ng-static jq
    # Override the workspace rust-lld linker for native Alpine proc macros too.
    case "$BUILD_TARGET" in
      x86_64-*) export CARGO_TARGET_X86_64_UNKNOWN_LINUX_MUSL_LINKER=cc ;;
      aarch64-*) export CARGO_TARGET_AARCH64_UNKNOWN_LINUX_MUSL_LINKER=cc ;;
    esac
    # Guest init remains static; only the host worker needs dynamic musl.
    cargo build --locked -p iii-init --release --target "$BUILD_TARGET"
    export CARGO_TARGET_DIR=/src/target/worker-musl/build
    # msb_krun uses statx, exposed by libc only with the musl >=1.2.3 ABI.
    # Alpine 3.22 ships musl 1.2.5; older musl is not a supported baseline.
    export RUST_LIBC_UNSTABLE_MUSL_V1_2_3=1
    export LIBCAPNG_LINK_TYPE=static
    export LIBCAPNG_LIB_PATH=/usr/lib
    # Rust std explicitly requests -lgcc_s even with -static-libgcc. Resolve
    # that name to the toolchain-provided static unwinder before system libs.
    unwind_dir="$CARGO_TARGET_DIR/unwind"
    mkdir -p "$unwind_dir"
    ln -sf "$(rustc --print sysroot)/lib/rustlib/$BUILD_TARGET/lib/self-contained/libunwind.a" \
      "$unwind_dir/libgcc_s.a"
    export RUSTFLAGS="-C target-feature=-crt-static -L native=$unwind_dir"
    cargo build --locked -p iii-worker --release --target "$BUILD_TARGET" \
      --features embed-init,embed-libkrunfw
    cargo test --locked -p iii-worker --release --target "$BUILD_TARGET" \
      --features embed-init,embed-libkrunfw --test musl_firmware --no-run \
      --message-format=json > "$CARGO_TARGET_DIR/test-artifacts.json"
    out=/src/target/worker-musl/$BUILD_TARGET
    mkdir -p "$out"
    cp "$CARGO_TARGET_DIR/$BUILD_TARGET/release/iii-worker" "$out/iii-worker"
    # Select the current executable, not a stale hash left in the cache.
    test_binary=$(jq -er '\''select(.reason == "compiler-artifact" and .target.name == "musl_firmware" and .executable != null) | .executable'\'' \
      "$CARGO_TARGET_DIR/test-artifacts.json")
    cp "$test_binary" "$out/musl_firmware"
    # Only musl itself may remain dynamic: no glibc, cap-ng or libgcc runtime.
    readelf -l "$out/iii-worker" | grep "ld-musl-"
    needed=$(readelf -d "$out/iii-worker" | grep "(NEEDED)")
    printf "%s\n" "$needed"
    test "$(printf "%s\n" "$needed" | wc -l)" -eq 1
    printf "%s\n" "$needed" | grep -F "[libc.musl-"
    chown -R '"$(id -u):$(id -g)"' /src/target
  '

out="$root/target/worker-musl/$target"
# A pristine Alpine runtime, not the builder with all its development packages.
# The firmware test uses the same flags as the shipped worker and needs no KVM.
docker run --rm --network none -v "$out:/test:ro" alpine:3.22 sh -ec '
  /test/iii-worker --version
  /test/iii-worker --help >/dev/null
  /test/musl_firmware --nocapture
'
archive="iii-worker-$target"
tar -czf "$out/$archive.tar.gz" -C "$out" iii-worker
(cd "$out" && sha256sum "$archive.tar.gz" > "$archive.sha256")
printf 'Built and tested %s\n' "$out/$archive.tar.gz"
