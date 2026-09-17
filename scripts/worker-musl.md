# Alpine/musl worker releases

The bundled `iii-worker` has separate GNU and musl Linux artifacts. The shell
installer and `iii update iii-worker` select the worker by the host musl loader,
not by the engine's compile target. The default static musl engine also runs on
glibc hosts, where the worker must still use the GNU artifact.

## Build and smoke-test

On a Linux machine with Docker, run the command matching the host architecture:

```sh
bash scripts/build-worker-musl.sh x86_64-unknown-linux-musl
# On a native ARM64 Linux runner:
bash scripts/build-worker-musl.sh aarch64-unknown-linux-musl
```

The same script runs in pull-request CI and stable/RC/alpha release jobs. It
uses the pinned Rust Alpine builder and Cargo.lock. Outputs are under
`target/worker-musl/<target>/`: the binary, `.tar.gz` archive and `.sha256` file.

## Linking requirements

- Alpine 3.22 (musl 1.2.5) is the build and runtime-test baseline.
- The host worker uses **dynamic musl** so `dlopen` can load embedded libkrunfw.
  Do not enable `crt-static` for this binary: it can pass `--version` while
  breaking firmware loading.
- `libcap-ng` and Rust's unwinder are linked statically. The build resolves
  Rust std's explicit `-lgcc_s` to the toolchain's bundled `libunwind.a`.
  No `gcompat`, glibc loader, or `libgcc_s.so.1` is required.
- `RUST_LIBC_UNSTABLE_MUSL_V1_2_3=1` exposes the `statx` ABI used by msb_krun
  in the locked libc crate. This is valid for the Alpine baseline, but not a
  promise of support for older musl versions.
- The embedded guest `iii-init` remains statically linked.

The script inspects the ELF loader/dependencies and runs `--version`, `--help`
and a firmware `dlopen`/symbol-resolution test in a clean, network-disabled
Alpine runtime with no development packages or compatibility libraries.
This catches both the missing-glibc-loader failure and accidentally static
musl builds. It does not boot a VM: actual sandbox execution still requires
KVM (`/dev/kvm`) and an appropriate guest rootfs. WSL must provide KVM support.

This changes the worker artifacts only; engine and console platform support
remain governed by their own release matrices.
