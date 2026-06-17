# Spike: shared read-only rootfs + per-worker overlay (block-image model)

Branch: `worktree-shared-rootfs-spike` (from origin/main @ 202068284).
Goal: prove iii can drop the per-worker rootfs clone by serving a shared read-only
base image (block device) + a per-worker writable ext4 upper, assembled with
overlayfs in iii-init — and along the way kill the empty-dep-folder host pollution.

## Findings so far

### Kernel capabilities (libkrunfw 5.x, via `msb_krun` 0.1.x)
- iii ships `~/.iii/lib/libkrunfw.5.dylib`; `msb_krun` is pinned with `features=["net","blk"]`
  (`crates/iii-worker/Cargo.toml:33`) — **the block-device disk API is already compiled in.**
- libkrunfw 5.x enables **CONFIG_EROFS_FS** (upstream). microsandbox runs
  **EROFS(ro) + ext4(rw) + overlayfs + virtio-blk** on this exact stack, so the kernel
  has EROFS + OVERLAY_FS + EXT4 + VIRTIO_BLK. The overlay model is proven viable here.
- **CONFIG_SQUASHFS: CONFIRMED PRESENT.** Live `/proc/filesystems` from a booted worker VM:
  `ext2 ext3 ext4 squashfs vfat fuseblk fuse virtiofs overlay xfs erofs btrfs`. So squashfs,
  erofs, overlay, ext4 are all in-kernel. **Writer decision locked: `backhand`/squashfs.**
- Could not introspect the kernel offline (no IKCFG magic in the dylib; kernel payload not
  gzip on aarch64). Empirical probe via a booted worker VM is the reliable route; local
  engine friction (see below) made this slow.

### Writer landscape — producing the read-only base image, decoupled from microsandbox
The user requirement: pure-Rust, cross-platform (macOS+Linux), **not** coupled to
microsandbox's `image`/`erofs` crate.
- **squashfs + `backhand`** (crates.io, MIT/Apache, mature, latest 0.25.x): pure-Rust
  *writer* (`FilesystemWriter`). Clean and decoupled — **viable only if the guest kernel
  has CONFIG_SQUASHFS.**
- **EROFS pure-Rust writers**: none mature. `erofs-rs` (Dreamacro) is **read-only** —
  building is an unimplemented TODO (13★, 15 commits, no release). The `erofs` org crate is
  similarly read-focused. So there is **no drop-in decoupled EROFS *writer*.**
- If the kernel is EROFS-only, the EROFS production options are: (a) shell to `mkfs.erofs`
  (erofs-utils, C — not on stock macOS; bundle it or run it **in a Linux builder VM**),
  (b) write our own minimal EROFS writer, or (c) use microsandbox's crate (rejected).

### Decision tree for the writer
- **Kernel has squashfs** → use `backhand`. Mature, decoupled, pure Rust. Done.
- **Kernel is EROFS-only** → no clean pure-Rust path; pick among mkfs.erofs-in-guest /
  own-writer / reconsider. This is the open call for the user.

### Architecture (unchanged from the migration spec)
- Host: one shared `~/.iii/cache/<ro-image>` + per-worker `~/.iii/managed/<name>/upper.ext4`
  (sparse). No per-worker rootfs clone.
- Boot: libkrun attaches base(ro, virtio-blk) + upper(rw, virtio-blk); iii-init mounts
  lower+upper, overlays, `pivot_root` (replaces the existing tmpfs-pivot virtiofs-readdir
  workaround). Upper formatted in-guest on first boot (`mkfs.ext4`, no host tooling).
- Workspace: W1 (copy-in) — deps live in the upper, the `DEPS_ROOT` bind loop is deleted,
  no host `mkdir` → empty-folder bug gone.

## Environment friction hit during the spike (for the record)
- Engine wasn't running; `--use-default-config` engine lacks `sandbox::create`.
- Harness-config engine (`/Users/andersonleal/projetos/iii/tmp/harness`) ran, but the
  `iii-worker-manager` worker never registered `sandbox::create`, and registry workers
  (coder/database/iii-directory) expose no `shell.sock` for `exec`.
- `todo-app` (local worker, has shell.sock) is the viable probe target; first boot is slow
  (npm install) — needs a patient wait, not an early kill.

## Validated so far
1. [DONE] squashfs/erofs/overlay/ext4/virtio-blk all in-kernel (live probe).
2. [DONE] **Pure-Rust `backhand` builds a kernel-valid squashfs from the extracted node rootfs.**
   Prototype: `$CLAUDE_JOB_DIR/tmp/sqfs-spike` (standalone crate, dep: `backhand=0.25`,`walkdir`).
   Result on `~/.iii/cache/docker.io-iiidev-node-latest-*`:
   - BUILD: 2768 dirs, 14861 files, 1279 symlinks, 0 skipped (no device/fifo/socket nodes).
   - Output: 220 MB squashfs v4.0 zlib; `file(1)` recognizes it ("Squashfs filesystem ... 18909 inodes").
   - READBACK (backhand reader): 18909 nodes; **`/bin -> usr/bin` symlink preserved**; `/usr/bin/sh` present.
   - Takeaway: writer is proven; symlink farm intact; 0 errors. (gzip ratio is mediocre at 220 MB —
     switch to zstd/xz later for size; not a correctness concern.)

## Boot-path changes — IMPLEMENTED + COMPILING (both sides), live boot pending

Milestone A scope: squashfs lower (`/dev/vda`) + **tmpfs upper** (no in-guest mkfs) + overlay
+ pivot. Reuses the existing virtiofs rootfs only as the boot trampoline for `/init.krun`;
iii-init pivots onto the overlay. (Eliminating the per-worker clone + persistent ext4 upper
is the next milestone — base image ships `mkfs.ext4`, so format-in-guest via chroot-into-lower
is viable; microsandbox instead formats ext4 on the host with a pure-Rust formatter.)

Env contract (host -> guest): `III_BLOCK_ROOT_LOWER=/dev/vda`,
`III_BLOCK_ROOT_LOWER_FSTYPE=squashfs`, `III_BLOCK_ROOT_UPPER=tmpfs`.

Files changed (uncommitted, branch `worktree-shared-rootfs-spike`):
- `crates/iii-init/src/root_pivot.rs` — `overlay_root_requested()` + `overlay_root()` (mount
  lower ro + tmpfs upper, assemble overlay at /new-root, relocate /workspace, pivot, detach).
- `crates/iii-init/src/main.rs` — branch to `overlay_root()` when requested.
- `crates/iii-worker/src/cli/vm_boot.rs` — when `III_ROOTFS_SQUASHFS` is set: attach that
  squashfs as a Raw read-only virtio-blk disk and set the `III_BLOCK_ROOT_*` exec env.

Compile status: `cargo build -p iii-init --target aarch64-unknown-linux-musl --release` OK;
`cargo build -p iii-worker` OK.

### Boot-test recipe (the remaining validation)
1. `make sandbox` (builds the musl iii-init with my changes, then `iii` + worker with
   `embed-init,embed-libkrunfw`) so the VM runs the modified `/init.krun`.
2. Start the engine with `III_ROOTFS_SQUASHFS=<.../base.squashfs>` exported (base.squashfs is
   the node image built in task 2; matches a node-based worker).
3. Add/boot a node local worker; watch `~/.iii/managed/<name>/logs/` — iii-init writes
   `iii-init: <err>` to stderr there, so an overlay/pivot failure is diagnosable (boot isn't blind).
4. Success = worker reaches ready on the overlay (squashfs base + tmpfs upper), `/workspace`
   still live. This is the first in-kernel MOUNT of the squashfs.

Risk/caveat: VM boots here are slow (minutes) and `sandbox::create`/`exec` are flaky in this
local setup, so iteration is minutes-per-cycle; init stderr in the worker log is the diagnosis path.

## LIVE BOOT — PROVEN ✅

Booted a worker in overlay mode end-to-end. Worker stdout/stderr:
```
✓ Found iii-init at ~/.iii/lib/iii-init                        (modified init used)
iii: overlay rootfs base (squashfs) -> /dev/vda: .../base2.squashfs   (vm_boot disk attach)
Booting VM (2 vCPUs, 2048 MiB RAM)...
iii-init: overlay root mode: lower=/dev/vda (squashfs) upper=tmpfs
iii-init: overlay root assembled and pivoted (lower=squashfs)  ← squashfs mount + overlay + pivot OK
iii: workspace ready; deps mounted VM-local from /var/iii/deps ← worker boot script ran ON the overlay
```
The `assembled and pivoted` line prints only after the squashfs lower mounts ro, the tmpfs
upper mounts, overlayfs assembles, and pivot_root into it all succeed. So the shared-RO-base +
overlay + pivot boot model works in-kernel via msb_krun virtio-blk, driven by the modified
iii-init + vm_boot. (The worker's own `/opt/iii/dev-run.sh` then hit a syntax error — a stale
dev-harness wrapper baked into the test squashfs — downstream of and unrelated to the rootfs path.)

How the modified init got into the VM (no embed-init needed): built the musl iii-init and placed
it at `~/.iii/lib/iii-init` (the `resolve_init_binary` location) so the engine copies it in; the
rebuilt `iii-worker` was placed at `~/.local/bin/iii-worker` (engine's first binary lookup); the
overlay trigger came from `~/.iii/overlay-base` (file fallback, since the engine's worker-mgmt
spawn chain doesn't forward arbitrary env to __vm-boot).

## IMAGE-INDEPENDENCE — PROVEN ✅ (backhand integrated into iii)

Goal: the sandbox must not depend on what's inside the base image. The in-guest
`mke2fs` idea is OUT (alpine/distroless/scratch ship no e2fsprogs). The answer is a
HOST-SIDE squashfs build via `backhand`, now a real iii integration:
- `crates/iii-worker/Cargo.toml` — `backhand = { version="0.25", default-features=false, features=["gzip"] }` (gzip-only → no xz/zstd C deps).
- `crates/iii-worker/src/cli/squashfs.rs` — `build_squashfs(src,out)` + `ensure_base_squashfs(dir)`: packs an extracted rootfs into a read-only squashfs purely on the host (recursive, preserves files/dirs/symlinks; skips device/fifo/socket). No `walkdir` dep.
- `crates/iii-worker/src/cli/vm_boot.rs` — overlay mode builds the base squashfs from a rootfs DIR (`III_ROOTFS_BASE_DIR` / `~/.iii/overlay-base-dir`) and attaches it as `/dev/vda`.

Proof boot — base image with `mke2fs` REMOVED (simulating a minimal/distroless image):
```
iii: building read-only base squashfs (host-side, image-independent) from .../minimal-base...
iii: overlay base squashfs -> /dev/vda: .../minimal-base.sqfs   (220.9M)
iii-init: overlay root assembled and pivoted (lower=squashfs)   ← booted with NO mke2fs in the base
node@v24.14.1                                                   ← node ran from the squashfs overlay
```
Host check confirmed the base had no `mke2fs`. Conclusion: any base image works —
iii packs it host-side (backhand), the upper is tmpfs, the init is static; the boot
never uses a tool from inside the image.

## MIGRATION & SAFETY LAYER — implemented

`msb_krun` bumped 0.1.9 → **0.1.16** (whole family; compiles clean — runtime boot
smoke-test still advised for a VMM bump).

Update-safety for "user updates iii with sandboxes running":
- **Running sandboxes are unaffected** — managed VMs are spawned detached
  (`setsid`/adopt-orphan, `libkrun.rs:132`); they hold the old binary in memory and
  survive the engine restart, staying on the old model until they restart.
- **Capability handshake** (`crates/iii-worker/src/cli/overlay.rs::overlay_active`):
  overlay activates only when iii-init is EMBEDDED (`has_init()`). An embedded init is
  compiled with this binary → always version-matched and overlay-capable. A stale
  cached `~/.iii/lib/iii-init` (independently versioned by the updater) would ignore
  `III_BLOCK_ROOT_*` and break — so requiring the embedded init makes updates safe.
  → answers "embed iii-init inside iii-worker": the `embed-init` feature; overlay
  *requires* it.
- **Feature flag, default ON** (`overlay_enabled`): `III_ROOTFS_MODE=legacy|off|0`
  reverts to the legacy per-worker-clone boot. Default (unset) = overlay.
- **Per-worker layout marker** + **GC of orphaned legacy artifacts** (`migrate_to_overlay`):
  on first overlay boot of a worker, the legacy `var/iii/deps` cache + `var/.iii-prepared`
  marker (dead weight under overlay) are removed to reclaim disk, then `.iii-layout=overlay`
  is stamped (idempotent).

Wiring: `vm_boot.rs` gates the overlay disk-attach on `overlay_active()` and runs
`migrate_to_overlay` before building/attaching the squashfs. Builds verified both ways:
default (overlay gracefully OFF — no embedded init) and `--features embed-init` (overlay
ON). Live boot confirmed the embedded-init handshake passes, the flag defaults on, and the
`.iii-layout=overlay` marker is written.

### Remaining for a real implementation (beyond the spike)
- Persistent ext4 upper instead of tmpfs (format-in-guest via chroot into the mounted lower —
  base image ships mkfs.ext4 — or a host-side ext4 formatter); then drop the per-worker clone.
- Build the base squashfs from the prepared rootfs (incl. iii's /opt/iii) as part of rootfs prep,
  cached per base image; switch zlib→zstd for size.
- Proper trigger plumbing (thread overlay mode through the worker-mgmt spawn, not a file).
