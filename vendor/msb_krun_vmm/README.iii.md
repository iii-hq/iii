# Temporary zstd security patch

This is the crates.io source of `msb_krun_vmm 0.1.16`, not a new VMM fork.
The only change to the published files is the normalized `Cargo.toml` dependency
on `zstd`: `0.13` becomes `0.14.3`. All Rust source and the EFI image are unchanged.
`Cargo.toml.orig` is retained verbatim as provenance; Cargo uses `Cargo.toml`.
The upstream package's nested `Cargo.lock` is omitted: the workspace lockfile is
the authoritative dependency graph and must not retain a second vulnerable graph.

## Provenance

- Archive: https://static.crates.io/crates/msb_krun_vmm/msb_krun_vmm-0.1.16.crate
- Archive SHA-256: `bb70c044c60e011e6f43c2e336687850b0910853a0efe37010e9649eff1395fe`
- Upstream commit: `c9584df73cb1d3ea9dfb7a3dac3fde8295d00e14`
- Upstream directory: `src/vmm`
- `LICENSE` and `THIRD-PARTY` are copied from that commit in
  https://github.com/zerocore-ai/libkrun.
  The crate declares `Apache-2.0 AND BSD-3-Clause`; original copyright and BSD
  notices in the source files are preserved.

## Why this is necessary

The pinned `msb_krun 0.1.16` depends on a VMM that requires `zstd 0.13`.
There is no patched 0.13 release; even `msb_krun_vmm 0.1.42` still requires it.
A lockfile-only update cannot select the fixed high-level API.

The root `[patch.crates-io]` redirects only this VMM crate. The lockfile selects
`zstd 0.14.3`, `zstd-safe 8.2.0`, and `zstd-sys 2.1.2+zstd.1.5.7`, fixing
RUSTSEC-2026-0391, RUSTSEC-2026-0398, RUSTSEC-2026-0403, RUSTSEC-2026-0410,
and the related `zstd-safe` unsoundness notices. No advisory is suppressed.
The VMM's only zstd call is `zstd::stream::copy_decode`, whose signature remains
compatible. Keeping the rest of libkrun at its existing versions avoids coupling
this security fix to a VMM upgrade or musl compatibility changes.

## Verification and removal

```sh
python3 scripts/check-vmm-security-patch.py
cargo audit
cargo deny check advisories bans
cargo check --locked -p iii-worker -p iii-filesystem -p iii-network -p iii-loadtest
```

The Python check verifies the upstream archive checksum, every retained file,
the single manifest edit, and the fixed versions in `Cargo.lock`. It can run
offline with `--archive /path/to/msb_krun_vmm-0.1.16.crate`.

Remove this directory, its workspace exclusion and `[patch.crates-io]` entry,
and the verification script once a compatible upstream VMM release uses the
fixed zstd chain. Regenerate `Cargo.lock` and rerun the advisory checks, worker
unit tests, and native Alpine/musl build before removing the patch.
