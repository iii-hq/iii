# Building and Running `iii worker add` / `iii worker dev` Locally

This guide explains how to build and run the `iii worker add` and `iii worker dev` commands in your local development environment without creating a GitHub release.

## Architecture Overview

The `iii worker add` and `iii worker dev` commands involve **two binaries** working together:

1. **`iii` (engine binary)** — the main CLI that dispatches `iii worker ...` to the `iii-worker` binary
2. **`iii-worker`** — the actual worker runtime binary that handles `add`, `dev`, `list`, etc.

When you run `iii worker add pdfkit`, the engine binary resolves the `iii-worker` binary via a dispatch mechanism, then replaces its process with `iii-worker add pdfkit`.

In a release, `iii` auto-downloads `iii-worker` from GitHub. For local dev, you need to build both and make them discoverable.

Additionally, `iii worker dev` boots a libkrun microVM, which requires two more runtime dependencies:

- **`libkrunfw`** — the VM firmware/kernel
- **`iii-init`** — the PID 1 init binary for the guest VM

---

## Step 1: Build the Engine Binary (`iii`)

```bash
cargo build -p iii
```

This produces `./target/debug/iii`.

## Step 2: Build the Worker Binary (`iii-worker`)

### Without embedded assets (simplest, recommended for local dev)

```bash
cargo build -p iii-worker
```

This produces `./target/debug/iii-worker`. Without the `embed-init` and `embed-libkrunfw` features, the runtime dependencies (libkrunfw and iii-init) are **downloaded automatically on first use** from GitHub releases.

### With embedded assets (fully self-contained, similar to release)

```bash
# First build iii-init for your VM guest architecture (always Linux musl)

# On Apple Silicon Mac:
rustup target add aarch64-unknown-linux-musl
cargo build -p iii-init --target aarch64-unknown-linux-musl --release

# On x86_64:
rustup target add x86_64-unknown-linux-musl
cargo build -p iii-init --target x86_64-unknown-linux-musl --release

# Then build iii-worker with embedded features
cargo build -p iii-worker --features embed-init,embed-libkrunfw
```

Or use the provided Makefile targets:

```bash
make sandbox-debug   # Debug builds of iii-init + iii + iii-worker (embedded)
make sandbox         # Release builds of all three
```

## Step 3: Make `iii-worker` Discoverable by `iii`

The `iii` engine dispatch mechanism looks for `iii-worker` in this order:

1. `~/.local/bin/iii-worker` (managed bin dir)
2. System `PATH`

You have three options:

### Option A: Symlink into `~/.local/bin/` (recommended)

```bash
mkdir -p ~/.local/bin
ln -sf "$(pwd)/target/debug/iii-worker" ~/.local/bin/iii-worker
```

### Option B: Add `target/debug` to your PATH

```bash
export PATH="$(pwd)/target/debug:$PATH"
```

### Option C: Run `iii-worker` directly (bypass the engine dispatch)

You can skip `iii` entirely and invoke `iii-worker` directly:

```bash
./target/debug/iii-worker add pdfkit@1.0.0
./target/debug/iii-worker dev ./my-project --port 49134
./target/debug/iii-worker list
```

This skips the engine's dispatch, download, and update-check machinery entirely.

## Step 4: Run the Commands

### Using the engine CLI (requires Step 3)

```bash
./target/debug/iii worker add pdfkit
./target/debug/iii worker dev ./my-project
./target/debug/iii worker list
```

### Using `iii-worker` directly

```bash
./target/debug/iii-worker add pdfkit@1.0.0
./target/debug/iii-worker dev ./my-project --port 49134
./target/debug/iii-worker list
```

---

## Runtime Dependency Resolution (libkrunfw and iii-init)

Both `worker add` (start) and `worker dev` need a running libkrun VM, which requires:

| Dependency | Resolution Order |
|---|---|
| **libkrunfw** | 1. `III_LIBKRUNFW_PATH` env var  2. `~/.iii/lib/libkrunfw.{5}.dylib`  3. Embedded bytes (if `--features embed-libkrunfw`)  4. Auto-download from GitHub release |
| **iii-init** | 1. Embedded (if `--features embed-init`)  2. `III_INIT_PATH` env var  3. `~/.iii/lib/iii-init`  4. Auto-download from GitHub release |

### For local dev without embedded features

You can either:

**Let them auto-download** — they will be fetched from the GitHub release matching the version in `Cargo.toml`. This may fail if no matching release exists for your dev version.

**Pre-provision manually:**

```bash
# Build iii-init locally
rustup target add aarch64-unknown-linux-musl  # or x86_64-unknown-linux-musl
cargo build -p iii-init --target aarch64-unknown-linux-musl --release

# Copy to the expected location
mkdir -p ~/.iii/lib
cp target/aarch64-unknown-linux-musl/release/iii-init ~/.iii/lib/iii-init
```

For libkrunfw, point to your local copy if you have one:

```bash
export III_LIBKRUNFW_PATH=/path/to/libkrunfw.5.dylib
```

**Or use env vars to point to local builds:**

```bash
export III_INIT_PATH="$(pwd)/target/aarch64-unknown-linux-musl/release/iii-init"
export III_LIBKRUNFW_PATH="/path/to/libkrunfw.5.dylib"
```

---

## Quick Reference

### Minimal steps (macOS Apple Silicon)

```bash
# 1. Build everything
cargo build -p iii              # engine CLI
cargo build -p iii-worker       # worker binary

# 2. Make iii-worker findable
mkdir -p ~/.local/bin
ln -sf "$(pwd)/target/debug/iii-worker" ~/.local/bin/iii-worker

# 3. Run (libkrunfw + iii-init auto-download on first use)
./target/debug/iii worker add pdfkit
./target/debug/iii worker dev ./my-project

# Or run iii-worker directly
./target/debug/iii-worker add pdfkit
./target/debug/iii-worker dev ./my-project
```

### Fully self-contained sandbox (no downloads needed)

```bash
make sandbox-debug
ln -sf "$(pwd)/target/debug/iii-worker" ~/.local/bin/iii-worker
./target/debug/iii worker dev ./my-project
```

---

## Important Notes

- **macOS codesign**: On macOS, `iii-worker dev` automatically codesigns itself with Hypervisor entitlements on first run. This may prompt for confirmation.
- **Intel Macs are not supported** for VM features — libkrunfw firmware is only available for Apple Silicon and Linux.
- **The engine must be running** for `iii worker add`/`start` to actually connect workers. Start the engine first with `./target/debug/iii` (no subcommand) or `make engine-up`.
- **Auto-download may fail** in dev if your local `Cargo.toml` version doesn't match any GitHub release tag. In that case, pre-provision `iii-init` and `libkrunfw` manually as described above.
