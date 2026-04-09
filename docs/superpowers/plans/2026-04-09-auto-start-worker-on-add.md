# Auto-Start Worker on Add Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** When `iii worker add` is called while the engine is already running, automatically start the worker instead of just telling the user to restart the engine.

**Architecture:** Add a shared `is_engine_running()` TCP port probe to `config_file.rs`. After each add handler writes to config.yaml, probe the engine port and call the appropriate start function if the engine is detected. Auto-start is best-effort — failures warn but don't fail the add.

**Tech Stack:** Rust, std::net::TcpStream

---

### Task 1: Extract `is_engine_running()` into `config_file.rs`

There is already an inline port probe in `managed.rs:1144-1149` (inside `handle_worker_list`). Extract it into a reusable public function in `config_file.rs` so all add handlers can use it.

**Files:**
- Modify: `crates/iii-worker/src/cli/config_file.rs` (add function at bottom of public API section)
- Modify: `crates/iii-worker/src/cli/managed.rs:1144-1149` (replace inline probe with call)

- [ ] **Step 1: Add `is_engine_running()` to `config_file.rs`**

Add this function after the existing public API functions (after `list_worker_names()`):

```rust
/// Probes `127.0.0.1:DEFAULT_PORT` to check whether the engine is listening.
/// Uses a 200ms timeout to avoid blocking the CLI.
pub fn is_engine_running() -> bool {
    std::net::TcpStream::connect_timeout(
        &std::net::SocketAddr::from(([127, 0, 0, 1], super::app::DEFAULT_PORT)),
        std::time::Duration::from_millis(200),
    )
    .is_ok()
}
```

- [ ] **Step 2: Replace the inline probe in `handle_worker_list`**

In `managed.rs`, replace lines 1144-1149:

```rust
    // Check if engine is running by probing its default port
    let engine_running = std::net::TcpStream::connect_timeout(
        &std::net::SocketAddr::from(([127, 0, 0, 1], super::app::DEFAULT_PORT)),
        std::time::Duration::from_millis(200),
    )
    .is_ok();
```

With:

```rust
    let engine_running = super::config_file::is_engine_running();
```

- [ ] **Step 3: Verify it compiles**

Run: `cargo build -p iii-worker 2>&1 | tail -5`
Expected: successful build, no errors.

- [ ] **Step 4: Commit**

```bash
git add crates/iii-worker/src/cli/config_file.rs crates/iii-worker/src/cli/managed.rs
git commit -m "refactor(iii-worker): extract is_engine_running() into config_file"
```

---

### Task 2: Auto-start in `handle_local_add()`

After the config.yaml write + success message in `local_worker.rs`, add the engine probe and conditional start.

**Files:**
- Modify: `crates/iii-worker/src/cli/local_worker.rs:350-365` (tail of `handle_local_add`)

- [ ] **Step 1: Replace the static "Start the engine" message with auto-start logic**

In `handle_local_add()`, replace the tail section (lines ~351-365) that currently reads:

```rust
    // 8. Print success
    if brief {
        eprintln!("        {} {}", "\u{2713}".green(), worker_name.bold());
    } else {
        eprintln!(
            "\n  {} Worker {} added to {}",
            "\u{2713}".green(),
            worker_name.bold(),
            "config.yaml".dimmed(),
        );
        eprintln!("  {}  {}", "Path".cyan().bold(), abs_path_str.bold());
        eprintln!("  Start the engine to run it, or edit config.yaml to customize.");
    }

    0
```

With:

```rust
    // 8. Print success
    if brief {
        eprintln!("        {} {}", "\u{2713}".green(), worker_name.bold());
    } else {
        eprintln!(
            "\n  {} Worker {} added to {}",
            "\u{2713}".green(),
            worker_name.bold(),
            "config.yaml".dimmed(),
        );
        eprintln!("  {}  {}", "Path".cyan().bold(), abs_path_str.bold());

        // Auto-start if engine is running
        if super::config_file::is_engine_running() {
            let port = super::app::DEFAULT_PORT;
            let result = start_local_worker(&worker_name, &abs_path_str, port).await;
            if result == 0 {
                eprintln!("  {} Worker auto-started", "\u{2713}".green());
            } else {
                eprintln!(
                    "  {} Could not auto-start worker. Run `iii worker start {}` manually.",
                    "\u{26a0}".yellow(),
                    worker_name
                );
            }
        } else {
            eprintln!("  Start the engine to run it, or edit config.yaml to customize.");
        }
    }

    0
```

- [ ] **Step 2: Verify it compiles**

Run: `cargo build -p iii-worker 2>&1 | tail -5`
Expected: successful build.

- [ ] **Step 3: Commit**

```bash
git add crates/iii-worker/src/cli/local_worker.rs
git commit -m "feat(iii-worker): auto-start local worker on add when engine running"
```

---

### Task 3: Auto-start in binary worker add (`handle_binary_add`)

After the config.yaml write in the binary add handler in `managed.rs`, add the same auto-start logic.

**Files:**
- Modify: `crates/iii-worker/src/cli/managed.rs:139-150` (tail of `handle_binary_add`)

- [ ] **Step 1: Replace static message with auto-start logic**

In `handle_binary_add()`, replace lines ~139-150:

```rust
    if brief {
        eprintln!("        {} {}", "✓".green(), worker_name.bold());
    } else {
        eprintln!(
            "\n  {} Worker {} added to {}",
            "✓".green(),
            worker_name.bold(),
            "config.yaml".dimmed(),
        );
        eprintln!("  Start the engine to run it, or edit config.yaml to customize.");
    }
    0
```

With:

```rust
    if brief {
        eprintln!("        {} {}", "✓".green(), worker_name.bold());
    } else {
        eprintln!(
            "\n  {} Worker {} added to {}",
            "✓".green(),
            worker_name.bold(),
            "config.yaml".dimmed(),
        );

        // Auto-start if engine is running
        if super::config_file::is_engine_running() {
            let result = start_binary_worker(&worker_name, &install_path).await;
            if result == 0 {
                eprintln!("  {} Worker auto-started", "✓".green());
            } else {
                eprintln!(
                    "  {} Could not auto-start worker. Run `iii worker start {}` manually.",
                    "⚠".yellow(),
                    worker_name
                );
            }
        } else {
            eprintln!("  Start the engine to run it, or edit config.yaml to customize.");
        }
    }
    0
```

- [ ] **Step 2: Verify it compiles**

Run: `cargo build -p iii-worker 2>&1 | tail -5`
Expected: successful build.

- [ ] **Step 3: Commit**

```bash
git add crates/iii-worker/src/cli/managed.rs
git commit -m "feat(iii-worker): auto-start binary worker on add when engine running"
```

---

### Task 4: Auto-start in OCI worker add (`handle_oci_pull_and_add`)

After the config.yaml write in the OCI add handler in `managed.rs`, add auto-start logic.

**Files:**
- Modify: `crates/iii-worker/src/cli/managed.rs:424-435` (tail of `handle_oci_pull_and_add`)

- [ ] **Step 1: Replace static message with auto-start logic**

In `handle_oci_pull_and_add()`, replace lines ~424-435:

```rust
    if brief {
        eprintln!("        {} {}", "✓".green(), name.bold());
    } else {
        eprintln!(
            "\n  {} Worker {} added to {}",
            "✓".green(),
            name.bold(),
            "config.yaml".dimmed(),
        );
        eprintln!("  Start the engine to run it, or edit config.yaml to customize.");
    }
    0
```

With:

```rust
    if brief {
        eprintln!("        {} {}", "✓".green(), name.bold());
    } else {
        eprintln!(
            "\n  {} Worker {} added to {}",
            "✓".green(),
            name.bold(),
            "config.yaml".dimmed(),
        );

        // Auto-start if engine is running
        if super::config_file::is_engine_running() {
            let port = super::app::DEFAULT_PORT;
            let worker_def = WorkerDef::Managed {
                image: image_ref.to_string(),
                env: oci_env,
                resources: None,
            };
            let result = start_oci_worker(name, &worker_def, port).await;
            if result == 0 {
                eprintln!("  {} Worker auto-started", "✓".green());
            } else {
                eprintln!(
                    "  {} Could not auto-start worker. Run `iii worker start {}` manually.",
                    "⚠".yellow(),
                    name
                );
            }
        } else {
            eprintln!("  Start the engine to run it, or edit config.yaml to customize.");
        }
    }
    0
```

Note: `oci_env` is already in scope from the env extraction earlier in the function. `image_ref` is the function parameter. `start_oci_worker` is already defined in this file.

- [ ] **Step 2: Verify it compiles**

Run: `cargo build -p iii-worker 2>&1 | tail -5`
Expected: successful build.

- [ ] **Step 3: Commit**

```bash
git add crates/iii-worker/src/cli/managed.rs
git commit -m "feat(iii-worker): auto-start OCI worker on add when engine running"
```

---

### Task 5: Auto-start in builtin worker add (inside `handle_managed_add`)

The builtin worker path in `handle_managed_add()` at lines ~262-293 also prints "Start the engine to run it." This needs the same treatment.

**Files:**
- Modify: `crates/iii-worker/src/cli/managed.rs:274-292` (builtin worker success path in `handle_managed_add`)

- [ ] **Step 1: Replace static message with auto-start logic**

In `handle_managed_add()`, find the non-brief builtin success path at lines ~274-292:

```rust
        } else {
            if already_exists {
                eprintln!(
                    "\n  {} Worker {} updated in {} (merged with builtin defaults)",
                    "✓".green(),
                    image_or_name.bold(),
                    "config.yaml".dimmed(),
                );
            } else {
                eprintln!(
                    "\n  {} Worker {} added to {}",
                    "✓".green(),
                    image_or_name.bold(),
                    "config.yaml".dimmed(),
                );
            }
            eprintln!("  Start the engine to run it, or edit config.yaml to customize.");
        }
        return 0;
```

Replace with:

```rust
        } else {
            if already_exists {
                eprintln!(
                    "\n  {} Worker {} updated in {} (merged with builtin defaults)",
                    "✓".green(),
                    image_or_name.bold(),
                    "config.yaml".dimmed(),
                );
            } else {
                eprintln!(
                    "\n  {} Worker {} added to {}",
                    "✓".green(),
                    image_or_name.bold(),
                    "config.yaml".dimmed(),
                );
            }

            // Auto-start if engine is running
            if super::config_file::is_engine_running() {
                let port = super::app::DEFAULT_PORT;
                let result = handle_managed_start(image_or_name, "0.0.0.0", port).await;
                if result == 0 {
                    eprintln!("  {} Worker auto-started", "✓".green());
                } else {
                    eprintln!(
                        "  {} Could not auto-start worker. Run `iii worker start {}` manually.",
                        "⚠".yellow(),
                        image_or_name
                    );
                }
            } else {
                eprintln!("  Start the engine to run it, or edit config.yaml to customize.");
            }
        }
        return 0;
```

Note: For builtins, we use `handle_managed_start()` which already handles type resolution internally — it's the same function `iii worker start <name>` calls.

- [ ] **Step 2: Verify it compiles**

Run: `cargo build -p iii-worker 2>&1 | tail -5`
Expected: successful build.

- [ ] **Step 3: Commit**

```bash
git add crates/iii-worker/src/cli/managed.rs
git commit -m "feat(iii-worker): auto-start builtin worker on add when engine running"
```

---

### Task 6: Final build and verify

**Files:** None (verification only)

- [ ] **Step 1: Full build**

Run: `cargo build -p iii-worker 2>&1 | tail -10`
Expected: successful build, no warnings related to our changes.

- [ ] **Step 2: Run existing tests**

Run: `cargo test -p iii-worker 2>&1 | tail -20`
Expected: all existing tests pass.

- [ ] **Step 3: Commit (if any cleanup needed)**

Only if the previous steps revealed issues that needed fixing.
