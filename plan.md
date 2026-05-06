# Project Init & Docker Generation Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add `iii project init` and `iii project generate-docker` commands that scaffold new iii projects with `.iii/project.ini`, default config, gitignores, and optional Docker assets — with the user's device_id wired into both `project.ini` and the generated Docker envs.

**Architecture:** Add a new built-in `Project` subcommand to `engine/src/main.rs` (handled in-process, not dispatched to a managed binary). Implementation lives in a new module `engine/src/cli/project/` with focused files per responsibility (init scaffold, docker generation, project.ini writer). Reuse the existing telemetry helper `iii::workers::telemetry::environment::get_or_create_device_id()` and the already-supported `III_HOST_USER_ID` Docker env hook. Ship templates as embedded `include_str!` strings to keep the binary self-contained.

**Tech Stack:** Rust 2024 edition, clap derive, tokio, anyhow, tempfile (tests). No new crates — all required behavior is reachable via existing deps (`uuid` for project_id, `rand` is not present so we'll use `uuid::Uuid` for the docker `.env` password too).

**Tickets covered:** MOT-3242 (project init), MOT-3241 (Docker generation), MOT-3276 (device_id injection).

**Out of plan, do separately:**
- CLI restructure proposal (MOT-3239) — must be discussed with Anthony before this lands. This plan assumes the agreed shape is `iii project init` and `iii project generate-docker`. If Anthony picks `iii project gen-docker` or different verbs, only the clap definitions change.
- `iii create` (existing template scaffolder, dispatched to `iii-tools`) is **untouched**. `iii project init` is the bare-minimum scaffold for an existing/empty dir; `iii create` is the full template-based flow.

---

## File Structure

**New files:**
- `engine/src/cli/project/mod.rs` — `Project` subcommand entry point + dispatch.
- `engine/src/cli/project/project_ini.rs` — read/write `.iii/project.ini` (device_id, project_id, project_name, source).
- `engine/src/cli/project/scaffold.rs` — write `config.yaml`, `iii.lock`, `.gitignore`, `data/` dir.
- `engine/src/cli/project/docker.rs` — write `Dockerfile`, `docker-compose.yml`, `.env`, with device_id wiring.
- `engine/src/cli/project/templates/Dockerfile.template` — embedded via `include_str!`.
- `engine/src/cli/project/templates/docker-compose.yml.template` — embedded via `include_str!`.
- `engine/src/cli/project/templates/gitignore.template` — combined Python + TypeScript + Rust ignores.
- `engine/src/cli/project/templates/config.yaml.template` — barebones config.
- `engine/src/cli/project/templates/iii.lock.template` — empty lockfile shape.
- `engine/tests/project_init_e2e.rs` — integration tests via `CARGO_BIN_EXE_iii`.

**Modified files:**
- `engine/src/cli/mod.rs` — register `pub mod project;`.
- `engine/src/main.rs` — add `Project` variant to `Commands` enum + dispatch arm in `main()`.

**Why this split:** Each file has one responsibility (parser, ini I/O, scaffold, docker). Templates are separate to allow text-only edits without touching Rust code. Tests sit at integration level since the user-visible behavior is filesystem state.

---

## Phase 0 — Pre-implementation gate (non-coding)

- [ ] **Step 0.1: Lock command shape with Anthony (HARD GATE)**

This is a gate. Phase 1 cannot start until a verb is locked. Renaming a public CLI command after release is a deprecation cycle, not a refactor.

Send Anthony a short message:
> "Need to lock verbs before coding the project commands. Two options:
> A) `iii project init` + `iii project generate-docker` (longer, explicit)
> B) `iii project init` + `iii project gen docker` (three-token noun-verb-noun, parallel)
> Which?"

Expected: A or B. Update the clap `#[derive(Subcommand)]` variant in Phase 1 step 1.3 to match. If B, the variant becomes `Gen(GenArgs)` with a nested `Docker` subcommand. Plan currently assumes A; B requires one extra `Subcommand` enum.

Do not start Phase 1 until the answer is in writing (Slack message, GitHub comment, etc.). Verbal nods get forgotten.

- [ ] **Step 0.2: Confirm template content sources**

Open `engine/Dockerfile` and `engine/docker-compose.yml`. The new templates derive from these but with three deltas:
1. Redis and RabbitMQ services commented out.
2. Default user/password replaced with `${RABBITMQ_USER}` / `${RABBITMQ_PASS}` (no `:-guest` fallback) so the values come from `.env`.
3. The user `iii` service gets `III_HOST_USER_ID=<device_id>` env injected at generation time.

No code action — just confirm these match expectations.

- [ ] **Step 0.3: Start the Linear tickets**

After the verb gate (0.1) is locked, move all three tickets to In Progress so the team sees the work starting and Linear's git integration links commits:

```bash
linear-cli issues start MOT-3242   # Add CLI project initialization command
linear-cli issues start MOT-3241   # Add CLI Docker file generation command
linear-cli issues start MOT-3276   # Inject device_id into project init outputs
```

Expected: each command prints the new state (Backlog → In Progress) and assigns the issue to you. If a ticket is already assigned to someone else (e.g. MOT-3276 ownership got reassigned), stop and confirm with the assignee before starting.

Tip: include the ticket ID in **every commit message body** for the rest of this plan — Linear's git integration will auto-link the commits. Example:

```
feat(cli): add iii project subcommand skeleton

Refs MOT-3242, MOT-3241.
```

The phase commits below already use Conventional Commit subjects; just append a `Refs MOT-XXXX[, MOT-YYYY]` line per the mapping in the table below.

**Phase → ticket mapping** (use these in commit bodies):

| Phase | MOT-3242 (init) | MOT-3241 (docker) | MOT-3276 (device_id) |
|---|---|---|---|
| 1 (clap skeleton) | ✓ | ✓ | — |
| 2 (project.ini) | ✓ | — | ✓ |
| 3 (scaffold) | ✓ | — | — |
| 4 (Docker templates) | — | ✓ | ✓ |
| 5 (docker.rs) | — | ✓ | ✓ |
| 5.5 (telemetry) | ✓ | — | — |
| 6 (wire run) | ✓ | ✓ | ✓ |
| 7 (e2e tests) | ✓ | ✓ | ✓ |
| 8 (help text) | ✓ | ✓ | — |
| 8.5 (README) | ✓ | — | — |
| 9 (architecture docs) | ✓ | ✓ | ✓ |

---

## Phase 1 — Wire up the `Project` subcommand (clap only, no logic)

### Task 1: Add `Project` to the `Commands` enum

**Files:**
- Modify: `engine/src/main.rs:60-128` (add new variant), `engine/src/main.rs:182-209` (add dispatch arm)
- Create: `engine/src/cli/project/mod.rs`
- Modify: `engine/src/cli/mod.rs:7-16` (add `pub mod project;`)

- [ ] **Step 1.1: Write failing test for `iii project init` parse**

Add to `engine/src/main.rs` `#[cfg(test)] mod tests` (after line 502, before the closing `}`):

```rust
#[test]
fn project_init_parses() {
    let cli = Cli::try_parse_from(["iii", "project", "init"])
        .expect("should parse project init");
    match cli.command {
        Some(Commands::Project(args)) => match args.action {
            ProjectAction::Init(_) => {}
            _ => panic!("expected Init action"),
        },
        _ => panic!("expected Project subcommand"),
    }
}

#[test]
fn project_init_with_directory_parses() {
    let cli = Cli::try_parse_from(["iii", "project", "init", "--directory", "myapp"])
        .expect("should parse project init --directory");
    match cli.command {
        Some(Commands::Project(args)) => match args.action {
            ProjectAction::Init(init) => assert_eq!(init.directory.as_deref(), Some("myapp")),
            _ => panic!("expected Init action"),
        },
        _ => panic!("expected Project subcommand"),
    }
}

#[test]
fn project_init_with_docker_flag_parses() {
    let cli = Cli::try_parse_from(["iii", "project", "init", "--docker"])
        .expect("should parse project init --docker");
    match cli.command {
        Some(Commands::Project(args)) => match args.action {
            ProjectAction::Init(init) => assert!(init.docker),
            _ => panic!("expected Init action"),
        },
        _ => panic!("expected Project subcommand"),
    }
}

#[test]
fn project_generate_docker_parses() {
    let cli = Cli::try_parse_from(["iii", "project", "generate-docker"])
        .expect("should parse project generate-docker");
    match cli.command {
        Some(Commands::Project(args)) => match args.action {
            ProjectAction::GenerateDocker(_) => {}
            _ => panic!("expected GenerateDocker action"),
        },
        _ => panic!("expected Project subcommand"),
    }
}
```

- [ ] **Step 1.2: Run tests to verify they fail**

Run: `cargo test -p iii --bin iii project_`
Expected: FAIL with "cannot find variant Project" / "cannot find type ProjectAction".

- [ ] **Step 1.3: Create the project module skeleton**

Create `engine/src/cli/project/mod.rs`:

```rust
// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at support@motia.dev
// See LICENSE and PATENTS files for details.

use clap::{Args, Subcommand};

#[derive(Args, Debug)]
pub struct ProjectArgs {
    #[command(subcommand)]
    pub action: ProjectAction,
}

#[derive(Subcommand, Debug)]
pub enum ProjectAction {
    /// Initialize a new iii project in the current directory
    Init(InitArgs),
    /// Generate Docker assets (Dockerfile, docker-compose.yml, .env) for an existing project
    GenerateDocker(GenerateDockerArgs),
}

#[derive(Args, Debug)]
pub struct InitArgs {
    /// Target directory (defaults to current directory)
    #[arg(short, long)]
    pub directory: Option<String>,

    /// Also generate Docker assets (Dockerfile, docker-compose.yml, .env)
    #[arg(long)]
    pub docker: bool,
}

#[derive(Args, Debug)]
pub struct GenerateDockerArgs {
    /// Target directory (defaults to current directory)
    #[arg(short, long)]
    pub directory: Option<String>,
}

pub async fn run(_args: ProjectArgs) -> i32 {
    eprintln!("error: not implemented");
    1
}
```

- [ ] **Step 1.4: Register the module in `engine/src/cli/mod.rs`**

Modify `engine/src/cli/mod.rs:7-16`. Find:

```rust
pub mod advisory;
pub mod download;
pub mod error;
pub mod exec;
pub mod github;
pub mod platform;
pub mod registry;
pub mod state;
pub mod telemetry;
pub mod update;
```

Replace with:

```rust
pub mod advisory;
pub mod download;
pub mod error;
pub mod exec;
pub mod github;
pub mod platform;
pub mod project;
pub mod registry;
pub mod state;
pub mod telemetry;
pub mod update;
```

- [ ] **Step 1.5: Add `Project` variant to `Commands` enum**

Modify `engine/src/main.rs:60-128`. After the `Sandbox` variant (line 118) and before `Update` (line 121), add:

```rust
    /// Manage iii projects (init, generate-docker)
    Project(crate::cli::project::ProjectArgs),
```

- [ ] **Step 1.6: Re-export ProjectAction at module path used in tests**

The tests reference `ProjectAction::Init(_)` directly. Add at the top of `engine/src/main.rs` after line 12 (`use cli_trigger::TriggerArgs;`):

```rust
#[cfg(test)]
use cli::project::{InitArgs, ProjectAction};
```

- [ ] **Step 1.7: Add the dispatch arm in `main()`**

Modify `engine/src/main.rs:182-209`. After the `Sandbox` arm (line 200-203) and before `Update` (line 204), add:

```rust
        Some(Commands::Project(args)) => {
            let exit_code = cli::project::run(args).await;
            std::process::exit(exit_code);
        }
```

- [ ] **Step 1.8: Run tests to verify they pass**

Run: `cargo test -p iii --bin iii project_`
Expected: 4 tests pass.

- [ ] **Step 1.9: Commit**

```bash
git add engine/src/main.rs engine/src/cli/mod.rs engine/src/cli/project/mod.rs
git commit -m "feat(cli): add iii project subcommand skeleton"
```

---

## Phase 2 — `project.ini` writer (the device_id home)

### Task 2: Write/read `.iii/project.ini` from the project crate

**Files:**
- Create: `engine/src/cli/project/project_ini.rs`
- Modify: `engine/src/cli/project/mod.rs` (add `pub mod project_ini;`)

**Why a new module instead of reusing the parser in `engine/src/workers/telemetry/mod.rs:89-126`?** That parser is private (`fn read_project_ini`) and lives inside the telemetry pipeline. We need write capability and a stable schema. We'll keep the formats compatible (same key names) so the telemetry reader continues to work unchanged.

- [ ] **Step 2.1: Write failing test**

Create `engine/src/cli/project/project_ini.rs`:

```rust
// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at support@motia.dev
// See LICENSE and PATENTS files for details.

use std::path::Path;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ProjectIni {
    pub project_id: Option<String>,
    pub project_name: Option<String>,
    pub source: Option<String>,
    pub device_id: Option<String>,
}

impl ProjectIni {
    pub fn write(&self, project_root: &Path) -> std::io::Result<()> {
        let dir = project_root.join(".iii");
        std::fs::create_dir_all(&dir)?;
        let path = dir.join("project.ini");

        let mut out = String::new();
        if let Some(v) = &self.project_id {
            out.push_str(&format!("project_id={}\n", v));
        }
        if let Some(v) = &self.project_name {
            out.push_str(&format!("project_name={}\n", v));
        }
        if let Some(v) = &self.source {
            out.push_str(&format!("source={}\n", v));
        }
        if let Some(v) = &self.device_id {
            out.push_str(&format!("device_id={}\n", v));
        }
        std::fs::write(path, out)
    }

    pub fn read(project_root: &Path) -> std::io::Result<Self> {
        let path = project_root.join(".iii").join("project.ini");
        let contents = std::fs::read_to_string(path)?;
        let mut out = Self::default();
        for line in contents.lines() {
            let line = line.trim();
            if let Some(v) = line.strip_prefix("project_id=") {
                out.project_id = Some(v.trim().to_string());
            } else if let Some(v) = line.strip_prefix("project_name=") {
                out.project_name = Some(v.trim().to_string());
            } else if let Some(v) = line.strip_prefix("source=") {
                out.source = Some(v.trim().to_string());
            } else if let Some(v) = line.strip_prefix("device_id=") {
                out.device_id = Some(v.trim().to_string());
            }
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn write_and_read_round_trip() {
        let dir = tempdir().unwrap();
        let ini = ProjectIni {
            project_id: Some("proj-123".to_string()),
            project_name: Some("myapp".to_string()),
            source: Some("init".to_string()),
            device_id: Some("device-abc".to_string()),
        };
        ini.write(dir.path()).unwrap();
        let read = ProjectIni::read(dir.path()).unwrap();
        assert_eq!(read, ini);
    }

    #[test]
    fn write_creates_dot_iii_dir() {
        let dir = tempdir().unwrap();
        let ini = ProjectIni {
            project_id: Some("p".to_string()),
            ..Default::default()
        };
        ini.write(dir.path()).unwrap();
        assert!(dir.path().join(".iii").join("project.ini").exists());
    }

    #[test]
    fn write_skips_none_fields() {
        let dir = tempdir().unwrap();
        let ini = ProjectIni {
            project_id: Some("p".to_string()),
            ..Default::default()
        };
        ini.write(dir.path()).unwrap();
        let contents = std::fs::read_to_string(dir.path().join(".iii").join("project.ini")).unwrap();
        assert!(contents.contains("project_id=p"));
        assert!(!contents.contains("project_name="));
        assert!(!contents.contains("device_id="));
    }
}
```

Add `pub mod project_ini;` to `engine/src/cli/project/mod.rs` near the top (after the file header / before the `use clap` line):

```rust
pub mod project_ini;
```

- [ ] **Step 2.2: Run tests to verify they pass**

Run: `cargo test -p iii --lib cli::project::project_ini`
Expected: 3 tests pass. (No "fail first" because pure data shape — the assertions verify contract, not behavior gap.)

- [ ] **Step 2.3: Commit**

```bash
git add engine/src/cli/project/mod.rs engine/src/cli/project/project_ini.rs
git commit -m "feat(cli): add ProjectIni reader/writer for .iii/project.ini"
```

---

## Phase 3 — Scaffold files (`config.yaml`, `iii.lock`, `.gitignore`, `data/`)

### Task 3: Embed templates and write scaffold files

**Files:**
- Create: `engine/src/cli/project/templates/config.yaml.template`
- Create: `engine/src/cli/project/templates/iii.lock.template`
- Create: `engine/src/cli/project/templates/gitignore.template`
- Create: `engine/src/cli/project/scaffold.rs`
- Modify: `engine/src/cli/project/mod.rs` (`pub mod scaffold;`)

- [ ] **Step 3.1: Create the three template files**

Create `engine/src/cli/project/templates/config.yaml.template`:

```yaml
# iii project configuration — see https://docs.iii.dev for the full schema.
workers: []
```

Create `engine/src/cli/project/templates/iii.lock.template`:

```yaml
version: 1
workers: {}
```

Create `engine/src/cli/project/templates/gitignore.template`:

```gitignore
# iii
.iii/
data/
*.lock.local

# Python
__pycache__/
*.py[cod]
*.egg-info/
.venv/
venv/

# TypeScript / Node
node_modules/
dist/
*.tsbuildinfo
.npm/

# Rust
target/
Cargo.lock.tmp

# Editors
.vscode/
.idea/
*.swp
.DS_Store

# Env
.env
.env.local
```

- [ ] **Step 3.2: Write failing test for scaffold**

Create `engine/src/cli/project/scaffold.rs`:

```rust
// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at support@motia.dev
// See LICENSE and PATENTS files for details.

use std::path::Path;

const CONFIG_TEMPLATE: &str = include_str!("templates/config.yaml.template");
const LOCK_TEMPLATE: &str = include_str!("templates/iii.lock.template");
const GITIGNORE_TEMPLATE: &str = include_str!("templates/gitignore.template");

pub fn write_scaffold(project_root: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(project_root.join("data"))?;
    write_if_absent(&project_root.join("config.yaml"), CONFIG_TEMPLATE)?;
    write_if_absent(&project_root.join("iii.lock"), LOCK_TEMPLATE)?;
    write_if_absent(&project_root.join(".gitignore"), GITIGNORE_TEMPLATE)?;
    Ok(())
}

fn write_if_absent(path: &Path, contents: &str) -> std::io::Result<()> {
    if path.exists() {
        return Ok(());
    }
    std::fs::write(path, contents)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn writes_all_scaffold_files() {
        let dir = tempdir().unwrap();
        write_scaffold(dir.path()).unwrap();
        assert!(dir.path().join("config.yaml").exists());
        assert!(dir.path().join("iii.lock").exists());
        assert!(dir.path().join(".gitignore").exists());
        assert!(dir.path().join("data").is_dir());
    }

    #[test]
    fn gitignore_includes_dot_iii() {
        let dir = tempdir().unwrap();
        write_scaffold(dir.path()).unwrap();
        let gi = std::fs::read_to_string(dir.path().join(".gitignore")).unwrap();
        assert!(gi.contains(".iii/"));
        assert!(gi.contains("node_modules/"));
        assert!(gi.contains("target/"));
        assert!(gi.contains("__pycache__"));
    }

    #[test]
    fn does_not_overwrite_existing_files() {
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("config.yaml"), "existing: true\n").unwrap();
        write_scaffold(dir.path()).unwrap();
        let content = std::fs::read_to_string(dir.path().join("config.yaml")).unwrap();
        assert_eq!(content, "existing: true\n");
    }
}
```

Add `pub mod scaffold;` to `engine/src/cli/project/mod.rs` (under the existing `pub mod project_ini;`).

- [ ] **Step 3.3: Run tests to verify they pass**

Run: `cargo test -p iii --lib cli::project::scaffold`
Expected: 3 tests pass.

- [ ] **Step 3.4: Commit**

```bash
git add engine/src/cli/project/templates engine/src/cli/project/scaffold.rs engine/src/cli/project/mod.rs
git commit -m "feat(cli): add project scaffold (config.yaml, iii.lock, .gitignore, data/)"
```

---

## Phase 4 — Docker template files

### Task 4: Create the Dockerfile and docker-compose templates

**Files:**
- Create: `engine/src/cli/project/templates/Dockerfile.template`
- Create: `engine/src/cli/project/templates/docker-compose.yml.template`

- [ ] **Step 4.1: Create `Dockerfile.template`**

Create `engine/src/cli/project/templates/Dockerfile.template`:

```dockerfile
# iii project Dockerfile — generated by `iii project generate-docker`.
# Edit freely; re-running the generator will not overwrite this file.
FROM iiidev/iii:latest

ARG III_HOST_USER_ID
ENV III_EXECUTION_CONTEXT=docker
ENV III_HOST_USER_ID=${III_HOST_USER_ID}

WORKDIR /app
COPY config.yaml /app/config.yaml
COPY iii.lock /app/iii.lock

EXPOSE 49134 3111 3112 9464
ENTRYPOINT ["iii"]
CMD ["--config", "/app/config.yaml"]
```

- [ ] **Step 4.2: Create `docker-compose.yml.template`**

Create `engine/src/cli/project/templates/docker-compose.yml.template`:

```yaml
# iii project docker-compose — generated by `iii project generate-docker`.
# Re-running the generator will not overwrite this file.
services:
  iii:
    build:
      context: .
      args:
        III_HOST_USER_ID: "${III_HOST_USER_ID}"
    ports:
      - "49134:49134"  # WebSocket (worker connections)
      - "3111:3111"    # REST API
      - "3112:3112"    # Stream API
      - "9464:9464"    # Prometheus metrics
    volumes:
      - ./config.yaml:/app/config.yaml:ro
    environment:
      - RUST_LOG=info
      - III_EXECUTION_CONTEXT=docker
      - III_HOST_USER_ID=${III_HOST_USER_ID}
    restart: unless-stopped

  # Uncomment if your workers need Redis.
  # redis:
  #   image: redis:7-alpine
  #   ports:
  #     - "6379:6379"
  #   volumes:
  #     - redis_data:/data
  #   restart: unless-stopped

  # Uncomment if your workers need RabbitMQ.
  # rabbitmq:
  #   image: rabbitmq:3-management-alpine
  #   ports:
  #     - "5672:5672"
  #     - "15672:15672"
  #   environment:
  #     - RABBITMQ_DEFAULT_USER=${RABBITMQ_USER}
  #     - RABBITMQ_DEFAULT_PASS=${RABBITMQ_PASS}
  #   volumes:
  #     - rabbitmq_data:/var/lib/rabbitmq
  #   restart: unless-stopped

# volumes:
#   redis_data:
#   rabbitmq_data:
```

- [ ] **Step 4.3: Commit (templates only — no code yet, so no test)**

```bash
git add engine/src/cli/project/templates/Dockerfile.template engine/src/cli/project/templates/docker-compose.yml.template
git commit -m "feat(cli): add Docker generation templates"
```

---

## Phase 5 — Docker generator (writes templates + `.env`)

### Task 5: Implement `docker.rs` — generate Dockerfile, docker-compose, .env

**Files:**
- Create: `engine/src/cli/project/docker.rs`
- Modify: `engine/src/cli/project/mod.rs` (`pub mod docker;`)

The `.env` file gets a UUID-based password (we have `uuid` already in the workspace; no `rand` dep needed). Random password is for RabbitMQ if the user later uncomments it; iii itself doesn't need a password.

- [ ] **Step 5.1: Write failing test**

Create `engine/src/cli/project/docker.rs`:

```rust
// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at support@motia.dev
// See LICENSE and PATENTS files for details.

use std::path::Path;

const DOCKERFILE_TEMPLATE: &str = include_str!("templates/Dockerfile.template");
const COMPOSE_TEMPLATE: &str = include_str!("templates/docker-compose.yml.template");

pub fn write_docker_assets(project_root: &Path, device_id: &str) -> std::io::Result<()> {
    write_if_absent(&project_root.join("Dockerfile"), DOCKERFILE_TEMPLATE)?;
    write_if_absent(&project_root.join("docker-compose.yml"), COMPOSE_TEMPLATE)?;
    write_env_file(project_root, device_id)?;
    Ok(())
}

fn write_env_file(project_root: &Path, device_id: &str) -> std::io::Result<()> {
    let path = project_root.join(".env");
    if path.exists() {
        return Ok(());
    }
    let rabbitmq_pass = uuid::Uuid::new_v4().simple().to_string();
    let contents = format!(
        "# Generated by `iii project generate-docker`. Do not commit.\n\
         III_HOST_USER_ID={device_id}\n\
         RABBITMQ_USER=iii\n\
         RABBITMQ_PASS={rabbitmq_pass}\n",
    );
    std::fs::write(path, contents)
}

fn write_if_absent(path: &Path, contents: &str) -> std::io::Result<()> {
    if path.exists() {
        return Ok(());
    }
    std::fs::write(path, contents)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn writes_dockerfile_compose_and_env() {
        let dir = tempdir().unwrap();
        write_docker_assets(dir.path(), "device-abc-123").unwrap();
        assert!(dir.path().join("Dockerfile").exists());
        assert!(dir.path().join("docker-compose.yml").exists());
        assert!(dir.path().join(".env").exists());
    }

    #[test]
    fn env_contains_device_id() {
        let dir = tempdir().unwrap();
        write_docker_assets(dir.path(), "device-abc-123").unwrap();
        let env = std::fs::read_to_string(dir.path().join(".env")).unwrap();
        assert!(env.contains("III_HOST_USER_ID=device-abc-123"));
    }

    #[test]
    fn env_has_random_rabbitmq_password() {
        let dir1 = tempdir().unwrap();
        let dir2 = tempdir().unwrap();
        write_docker_assets(dir1.path(), "d").unwrap();
        write_docker_assets(dir2.path(), "d").unwrap();
        let env1 = std::fs::read_to_string(dir1.path().join(".env")).unwrap();
        let env2 = std::fs::read_to_string(dir2.path().join(".env")).unwrap();
        assert_ne!(env1, env2, "two .env files should have different passwords");
    }

    #[test]
    fn compose_has_redis_commented_out() {
        let dir = tempdir().unwrap();
        write_docker_assets(dir.path(), "d").unwrap();
        let compose = std::fs::read_to_string(dir.path().join("docker-compose.yml")).unwrap();
        // Redis section exists but is commented
        let redis_lines: Vec<&str> = compose.lines().filter(|l| l.contains("redis")).collect();
        assert!(!redis_lines.is_empty(), "redis should be referenced");
        for line in &redis_lines {
            assert!(
                line.trim_start().starts_with('#'),
                "redis line should be commented: {}",
                line
            );
        }
    }

    #[test]
    fn compose_has_rabbitmq_commented_out() {
        let dir = tempdir().unwrap();
        write_docker_assets(dir.path(), "d").unwrap();
        let compose = std::fs::read_to_string(dir.path().join("docker-compose.yml")).unwrap();
        let rmq_lines: Vec<&str> = compose
            .lines()
            .filter(|l| l.to_lowercase().contains("rabbitmq"))
            .collect();
        assert!(!rmq_lines.is_empty(), "rabbitmq should be referenced");
        for line in &rmq_lines {
            assert!(
                line.trim_start().starts_with('#'),
                "rabbitmq line should be commented: {}",
                line
            );
        }
    }

    #[test]
    fn dockerfile_references_iii_host_user_id() {
        let dir = tempdir().unwrap();
        write_docker_assets(dir.path(), "d").unwrap();
        let df = std::fs::read_to_string(dir.path().join("Dockerfile")).unwrap();
        assert!(df.contains("III_HOST_USER_ID"), "Dockerfile must wire III_HOST_USER_ID");
    }

    #[test]
    fn does_not_overwrite_existing_dockerfile() {
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("Dockerfile"), "FROM scratch\n").unwrap();
        write_docker_assets(dir.path(), "d").unwrap();
        let df = std::fs::read_to_string(dir.path().join("Dockerfile")).unwrap();
        assert_eq!(df, "FROM scratch\n");
    }
}
```

Add `pub mod docker;` to `engine/src/cli/project/mod.rs` (under existing module decls).

- [ ] **Step 5.2: Run tests to verify they pass**

Run: `cargo test -p iii --lib cli::project::docker`
Expected: 7 tests pass.

> **Note**: if `uuid` isn't already in `engine/Cargo.toml` `[dependencies]`, add it. Check first with `grep '^uuid' engine/Cargo.toml`. If absent, add: `uuid = { version = "1", features = ["v4"] }`.

- [ ] **Step 5.3: Commit**

```bash
git add engine/src/cli/project/docker.rs engine/src/cli/project/mod.rs
git commit -m "feat(cli): generate Dockerfile, docker-compose.yml, and .env with device_id"
```

---

## Phase 5.5 — Telemetry helpers for project init

### Task 5.5: Add `send_project_init_succeeded` / `send_project_init_failed` to engine telemetry

**Files:**
- Modify: `engine/src/cli/telemetry.rs` (append two pub fns after the existing `send_cli_update_*` helpers)

**Why:** A successful first `iii project init` is the conversion event for the entire DevEx revamp. Without a telemetry event we cannot answer "did this ship a DX win?" Existing pattern at `engine/src/cli/telemetry.rs:123-167` is the template — sync, fire-and-forget, gated by `is_telemetry_disabled`.

- [ ] **Step 5.5.1: Append the two helpers**

In `engine/src/cli/telemetry.rs`, after the existing `send_cli_update_failed` function (after line 167 or wherever it ends), append:

```rust
pub fn send_project_init_succeeded(with_docker: bool, project_id: &str) {
    if let Some(event) = build_event(
        "project_init_succeeded",
        serde_json::json!({
            "with_docker": with_docker,
            "project_id": project_id,
        }),
        None,
    ) {
        send_fire_and_forget(event);
    }
}

pub fn send_project_init_failed(stage: &str, error: &str) {
    if let Some(event) = build_event(
        "project_init_failed",
        serde_json::json!({
            "stage": stage,
            "error": error,
        }),
        None,
    ) {
        send_fire_and_forget(event);
    }
}
```

- [ ] **Step 5.5.2: Verify the crate compiles**

Run: `cargo check -p iii`
Expected: clean build. The new fns reuse `build_event` and `send_fire_and_forget` already defined in the file.

- [ ] **Step 5.5.3: Commit**

```bash
git add engine/src/cli/telemetry.rs
git commit -m "feat(telemetry): add project_init_succeeded and project_init_failed events"
```

---

## Phase 6 — Wire `run` to actually execute init / generate-docker

### Task 6: Implement `run()` dispatch in `mod.rs`

**Files:**
- Modify: `engine/src/cli/project/mod.rs` (replace stub `run()`)

- [ ] **Step 6.1: Replace the stub `run()` with the real dispatcher (with DX polish baked in)**

This is the user-facing entry point. Errors must follow the problem/cause/fix triad. Success must tell the user what to type next. Init must emit a telemetry event so we can measure if the DevEx revamp actually moved the needle. `generate-docker` must warn (not silently degrade) when no `project.ini` exists.

In `engine/src/cli/project/mod.rs`, replace the `pub async fn run` body. The full updated function:

```rust
use colored::Colorize;

pub async fn run(args: ProjectArgs) -> i32 {
    match args.action {
        ProjectAction::Init(init) => run_init(init).await,
        ProjectAction::GenerateDocker(gd) => run_generate_docker(gd).await,
    }
}

async fn run_init(args: InitArgs) -> i32 {
    let root = match resolve_root(args.directory.as_deref()) {
        Ok(p) => p,
        Err(e) => {
            return print_err(
                "could not resolve target directory",
                &e,
                "pass --directory <path> or run from a writable cwd",
            );
        }
    };

    if let Err(e) = std::fs::create_dir_all(&root) {
        crate::cli::telemetry::send_project_init_failed("create_dir", &e.to_string());
        return print_err(
            &format!("could not create {}", root.display()),
            &e.to_string(),
            "check parent directory permissions or pick a different --directory",
        );
    }

    let device_id = iii::workers::telemetry::environment::get_or_create_device_id();
    let project_name = root
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("iii-project")
        .to_string();
    let project_id = uuid::Uuid::new_v4().to_string();

    let ini = project_ini::ProjectIni {
        project_id: Some(project_id.clone()),
        project_name: Some(project_name.clone()),
        source: Some("init".to_string()),
        device_id: Some(device_id.clone()),
    };
    if let Err(e) = ini.write(&root) {
        crate::cli::telemetry::send_project_init_failed("write_project_ini", &e.to_string());
        return print_err(
            "could not write .iii/project.ini",
            &e.to_string(),
            "check that the target directory is writable",
        );
    }

    if let Err(e) = scaffold::write_scaffold(&root) {
        crate::cli::telemetry::send_project_init_failed("write_scaffold", &e.to_string());
        return print_err(
            "could not write scaffold files",
            &e.to_string(),
            "check disk space and target directory permissions",
        );
    }

    if args.docker {
        if let Err(e) = docker::write_docker_assets(&root, &device_id) {
            crate::cli::telemetry::send_project_init_failed("write_docker", &e.to_string());
            return print_err(
                "could not write Docker assets",
                &e.to_string(),
                "remove existing Dockerfile/docker-compose.yml or check write permissions",
            );
        }
    }

    crate::cli::telemetry::send_project_init_succeeded(args.docker, &project_id);

    eprintln!();
    eprintln!(
        "  {} iii project '{}' initialized at {}",
        "✓".green(),
        project_name.bold(),
        root.display()
    );
    eprintln!();
    eprintln!("  Next steps:");
    if args.directory.is_some() {
        eprintln!("    {}", format!("cd {}", project_name).bold());
    }
    eprintln!("    {}    # add a worker", "iii worker add <package>".bold());
    eprintln!("    {}                          # start the engine", "iii".bold());
    if args.docker {
        eprintln!("    {}           # or start in Docker", "docker compose up".bold());
    }
    eprintln!();
    eprintln!("  Docs: https://iii.dev/docs/quickstart");
    0
}

async fn run_generate_docker(args: GenerateDockerArgs) -> i32 {
    let root = match resolve_root(args.directory.as_deref()) {
        Ok(p) => p,
        Err(e) => {
            return print_err(
                "could not resolve target directory",
                &e,
                "pass --directory <path> or run from a writable cwd",
            );
        }
    };

    let device_id = resolve_device_id_for_docker(&root);

    if let Err(e) = docker::write_docker_assets(&root, &device_id) {
        return print_err(
            "could not write Docker assets",
            &e.to_string(),
            "remove existing Dockerfile/docker-compose.yml or check write permissions",
        );
    }
    eprintln!();
    eprintln!(
        "  {} Docker assets generated at {}",
        "✓".green(),
        root.display()
    );
    eprintln!();
    eprintln!("  Next: {}", "docker compose up".bold());
    0
}

fn resolve_device_id_for_docker(root: &std::path::Path) -> String {
    match project_ini::ProjectIni::read(root) {
        Ok(ini) => match ini.device_id {
            Some(id) => id,
            None => {
                warn_missing_project_ini(root, "device_id missing in .iii/project.ini");
                iii::workers::telemetry::environment::get_or_create_device_id()
            }
        },
        Err(_) => {
            warn_missing_project_ini(root, "no .iii/project.ini found");
            iii::workers::telemetry::environment::get_or_create_device_id()
        }
    }
}

fn warn_missing_project_ini(root: &std::path::Path, problem: &str) {
    eprintln!("  {} {} at {}", "warning:".yellow().bold(), problem, root.display());
    eprintln!("  {} using a fresh device_id; metrics will not link to a project.", "impact:".dimmed());
    eprintln!("  {} run `iii project init` here to persist a project identity.", "fix:".dimmed());
}

fn resolve_root(dir: Option<&str>) -> Result<std::path::PathBuf, String> {
    match dir {
        Some(d) => Ok(std::path::PathBuf::from(d)),
        None => std::env::current_dir().map_err(|e| format!("cannot read cwd: {}", e)),
    }
}

fn print_err(problem: &str, cause: &str, fix: &str) -> i32 {
    eprintln!("{} {}", "error:".red().bold(), problem);
    eprintln!("  {} {}", "cause:".dimmed(), cause);
    eprintln!("  {} {}", "fix:".dimmed(), fix);
    1
}
```

Note: `colored::Colorize` is already a dependency (`engine/src/cli/mod.rs:18` imports it). No Cargo.toml change needed.

- [ ] **Step 6.2: Verify the crate compiles**

Run: `cargo check -p iii`
Expected: clean build (warnings allowed).

If `iii::workers::telemetry::environment::get_or_create_device_id` isn't `pub` at the path used, fix the path. Verify with `grep -n "pub fn get_or_create_device_id" engine/src/workers/telemetry/environment.rs` — it's `pub` per `engine/src/workers/telemetry/environment.rs:268`. Confirm `workers::telemetry::environment` is reachable from the `iii` crate root (`engine/src/lib.rs`).

- [ ] **Step 6.3: Verify the parser tests still pass**

Run: `cargo test -p iii --bin iii project_`
Expected: the 4 parse tests from Phase 1 still pass.

- [ ] **Step 6.4: Commit**

```bash
git add engine/src/cli/project/mod.rs
git commit -m "feat(cli): wire iii project init and generate-docker to scaffold and Docker writers"
```

---

## Phase 7 — End-to-end integration tests

### Task 7: Verify `iii project init` produces a working scaffold via the real binary

**Files:**
- Create: `engine/tests/project_init_e2e.rs`

- [ ] **Step 7.1: Write the e2e tests**

Create `engine/tests/project_init_e2e.rs`:

```rust
//! End-to-end tests for `iii project init` and `iii project generate-docker`.
//! Exercises the real binary so subcommand routing and filesystem state are both verified.

use std::process::Command;
use tempfile::tempdir;

fn iii_bin() -> Command {
    Command::new(env!("CARGO_BIN_EXE_iii"))
}

#[test]
fn project_init_creates_minimum_scaffold() {
    let dir = tempdir().unwrap();
    let out = iii_bin()
        .args(["project", "init", "--directory"])
        .arg(dir.path())
        .output()
        .expect("failed to run iii");
    assert!(
        out.status.success(),
        "init failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    assert!(dir.path().join(".iii").join("project.ini").exists());
    assert!(dir.path().join("config.yaml").exists());
    assert!(dir.path().join("iii.lock").exists());
    assert!(dir.path().join(".gitignore").exists());
    assert!(dir.path().join("data").is_dir());
}

#[test]
fn project_init_writes_device_id_into_project_ini() {
    let dir = tempdir().unwrap();
    let out = iii_bin()
        .args(["project", "init", "--directory"])
        .arg(dir.path())
        .output()
        .expect("failed to run iii");
    assert!(out.status.success());

    let ini = std::fs::read_to_string(dir.path().join(".iii").join("project.ini")).unwrap();
    let device_id_line = ini
        .lines()
        .find(|l| l.starts_with("device_id="))
        .expect("project.ini should contain device_id=");
    let value = device_id_line.trim_start_matches("device_id=").trim();
    assert!(!value.is_empty(), "device_id should not be empty");
}

#[test]
fn project_init_with_docker_flag_writes_docker_assets_with_device_id() {
    let dir = tempdir().unwrap();
    let out = iii_bin()
        .args(["project", "init", "--docker", "--directory"])
        .arg(dir.path())
        .output()
        .expect("failed to run iii");
    assert!(
        out.status.success(),
        "init --docker failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    assert!(dir.path().join("Dockerfile").exists());
    assert!(dir.path().join("docker-compose.yml").exists());
    assert!(dir.path().join(".env").exists());

    let ini = std::fs::read_to_string(dir.path().join(".iii").join("project.ini")).unwrap();
    let device_id_in_ini = ini
        .lines()
        .find_map(|l| l.strip_prefix("device_id="))
        .map(|v| v.trim().to_string())
        .expect("project.ini missing device_id");

    let env = std::fs::read_to_string(dir.path().join(".env")).unwrap();
    assert!(
        env.contains(&format!("III_HOST_USER_ID={device_id_in_ini}")),
        "Docker .env should hard-code the same device_id as project.ini"
    );
}

#[test]
fn project_generate_docker_uses_existing_project_ini_device_id() {
    let dir = tempdir().unwrap();
    // Pre-seed a project.ini with a known device_id
    std::fs::create_dir_all(dir.path().join(".iii")).unwrap();
    std::fs::write(
        dir.path().join(".iii").join("project.ini"),
        "device_id=preseeded-xyz\n",
    )
    .unwrap();

    let out = iii_bin()
        .args(["project", "generate-docker", "--directory"])
        .arg(dir.path())
        .output()
        .expect("failed to run iii");
    assert!(
        out.status.success(),
        "generate-docker failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    let env = std::fs::read_to_string(dir.path().join(".env")).unwrap();
    assert!(
        env.contains("III_HOST_USER_ID=preseeded-xyz"),
        "generate-docker should reuse the existing project.ini device_id, got .env:\n{}",
        env
    );
}

#[test]
fn project_init_does_not_clobber_existing_config() {
    let dir = tempdir().unwrap();
    std::fs::write(dir.path().join("config.yaml"), "existing: yes\n").unwrap();
    let out = iii_bin()
        .args(["project", "init", "--directory"])
        .arg(dir.path())
        .output()
        .expect("failed to run iii");
    assert!(out.status.success());
    let cfg = std::fs::read_to_string(dir.path().join("config.yaml")).unwrap();
    assert_eq!(cfg, "existing: yes\n");
}

#[test]
fn project_init_prints_next_steps_with_docs_link() {
    let dir = tempdir().unwrap();
    let out = iii_bin()
        .args(["project", "init", "--directory"])
        .arg(dir.path())
        .output()
        .expect("failed to run iii");
    assert!(out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("Next steps"),
        "expected 'Next steps' block in output:\n{}",
        stderr
    );
    assert!(
        stderr.contains("iii.dev/docs/quickstart"),
        "expected docs link in output:\n{}",
        stderr
    );
    assert!(
        stderr.contains("iii worker add"),
        "next steps should mention worker add:\n{}",
        stderr
    );
}

#[test]
fn project_generate_docker_warns_when_no_project_ini() {
    let dir = tempdir().unwrap();
    let out = iii_bin()
        .args(["project", "generate-docker", "--directory"])
        .arg(dir.path())
        .output()
        .expect("failed to run iii");
    assert!(out.status.success(), "generate-docker should still succeed, just warn");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("warning:"),
        "expected 'warning:' label in output:\n{}",
        stderr
    );
    assert!(
        stderr.contains(".iii/project.ini") || stderr.contains("project.ini"),
        "warning should reference project.ini:\n{}",
        stderr
    );
    assert!(
        stderr.contains("iii project init"),
        "warning should suggest running iii project init:\n{}",
        stderr
    );
}

#[test]
#[cfg(unix)]
fn project_init_failure_emits_problem_cause_fix() {
    // Force a failure: /dev/null/anything is never creatable on Unix.
    let out = iii_bin()
        .args(["project", "init", "--directory", "/dev/null/cannot-create"])
        .output()
        .expect("failed to run iii");
    assert!(
        !out.status.success(),
        "init should fail when target dir cannot be created"
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("error:"), "stderr should contain 'error:':\n{}", stderr);
    assert!(stderr.contains("cause:"), "stderr should contain 'cause:':\n{}", stderr);
    assert!(stderr.contains("fix:"), "stderr should contain 'fix:':\n{}", stderr);
}
```

- [ ] **Step 7.2: Run e2e tests**

Run: `cargo test -p iii --test project_init_e2e`
Expected: 5 tests pass.

If a test fails because of `III_TELEMETRY_ENABLED` or sandboxed home dir affecting `get_or_create_device_id`, set `HOME` to a temp dir for the test — but verify it's actually a problem first by reading the failure output.

- [ ] **Step 7.3: Commit**

```bash
git add engine/tests/project_init_e2e.rs
git commit -m "test(cli): e2e tests for iii project init and generate-docker"
```

---

## Phase 8 — Wire the help text and update CLI integration test

### Task 8: Make `iii --help` advertise the new command and update existing assertions

**Files:**
- Modify: `engine/tests/cli_integration.rs:43-53` (extend the help-shows-subcommands assertion)

- [ ] **Step 8.1: Add `project` to the help-output assertion**

Modify `engine/tests/cli_integration.rs:48-52`. Find:

```rust
    assert!(stdout.contains("trigger"), "help should list trigger");
    assert!(stdout.contains("console"), "help should list console");
    assert!(stdout.contains("create"), "help should list create");
    assert!(stdout.contains("worker"), "help should list worker");
    assert!(stdout.contains("update"), "help should list update");
```

Replace with:

```rust
    assert!(stdout.contains("trigger"), "help should list trigger");
    assert!(stdout.contains("console"), "help should list console");
    assert!(stdout.contains("create"), "help should list create");
    assert!(stdout.contains("worker"), "help should list worker");
    assert!(stdout.contains("project"), "help should list project");
    assert!(stdout.contains("update"), "help should list update");
```

- [ ] **Step 8.2: Run the integration tests**

Run: `cargo test -p iii --test cli_integration`
Expected: all tests pass, including the updated `help_flag_shows_all_subcommands`.

- [ ] **Step 8.3: Commit**

```bash
git add engine/tests/cli_integration.rs
git commit -m "test(cli): assert help output lists project subcommand"
```

---

## Phase 8.5 — README onboarding update

### Task 8.5: Surface `iii project init` in the project README

**Files:**
- Modify: `README.md:53-55` (Quick Start section)

**Why:** Today the README defers everything to https://iii.dev/docs/quickstart. The new `iii project init` command makes a 4-line copy-paste quickstart possible — that's the whole DX win the revamp promised. Don't bury it on a third-party page.

- [ ] **Step 8.5.1: Replace the Quick Start section**

In `README.md`, find:

```markdown
## Quick Start

Get started with iii by following the [Quickstart guide](https://iii.dev/docs/quickstart).
```

Replace with:

````markdown
## Quick Start

```bash
curl -fsSL https://iii.dev/install.sh | sh    # install iii
iii project init myapp                         # scaffold a project
cd myapp
iii                                            # start the engine
```

Full walkthrough at the [Quickstart guide](https://iii.dev/docs/quickstart).
````

- [ ] **Step 8.5.2: Verify the install URL is canonical**

Run: `curl -sI https://iii.dev/install.sh | head -1`
Expected: HTTP 200 (or a 30x to a canonical install entry point).

If install.sh does not exist, swap line 1 of the code block for whichever install method the README badges already imply. Inspect `README.md:6-10` (Docker / npm / PyPI / Crates badges) — pick the one closest to "first 5 minutes" experience for a new dev. Do **not** invent a URL.

- [ ] **Step 8.5.3: Commit**

```bash
git add README.md
git commit -m "docs(readme): surface iii project init in Quick Start"
```

---

## Phase 9 — Architecture docs sync

### Task 9: Update `architecture/CHANGE-MAP.md` per project rules

**Files:**
- Modify: `architecture/CHANGE-MAP.md` (add entry for project init/docker)

- [ ] **Step 9.1: Read existing CHANGE-MAP**

Run: `cat architecture/CHANGE-MAP.md | head -80`
Expected: see the file structure to understand where to add a new entry.

- [ ] **Step 9.2: Add the new section**

Add a new entry under the appropriate task heading (or create one) with:
- Files: `engine/src/cli/project/`, `engine/tests/project_init_e2e.rs`
- Trigger: "Adding a new top-level CLI subcommand"
- Note: device_id wiring relies on `engine/src/workers/telemetry/environment.rs:268` (`get_or_create_device_id`).

Use the same format as existing entries — don't invent a new structure.

- [ ] **Step 9.3: Commit**

```bash
git add architecture/CHANGE-MAP.md
git commit -m "docs(architecture): map iii project subcommand"
```

---

## Phase 10 — Manual smoke test

### Task 10: Build and run end-to-end against a real shell

- [ ] **Step 10.1: Build release**

Run: `cargo build -p iii --release`
Expected: clean build.

- [ ] **Step 10.2: Smoke test init**

Run from `/tmp`:

```bash
cd /tmp
rm -rf iii-smoke-test
target/release/iii project init --directory iii-smoke-test
```

Then inspect:

```bash
ls -la iii-smoke-test
cat iii-smoke-test/.iii/project.ini
```

Expected: `.iii/project.ini` exists with `device_id=`, `project_id=`, `project_name=iii-smoke-test`. `data/`, `config.yaml`, `iii.lock`, `.gitignore` all present.

- [ ] **Step 10.3: Smoke test generate-docker (files)**

```bash
cd iii-smoke-test
../target/release/iii project generate-docker
ls -la
cat .env
cat Dockerfile
```

Expected: `Dockerfile`, `docker-compose.yml`, `.env` present. `.env` contains `III_HOST_USER_ID=<same as project.ini>`. Output ends with a "Next: docker compose up" hint line.

- [ ] **Step 10.3a: Smoke test generate-docker (actually runs)**

The whole point of Docker generation is that `docker compose up` works. Files-on-disk is necessary, not sufficient. Skip this step only if `docker` is not installed locally.

```bash
if command -v docker >/dev/null 2>&1 && docker info >/dev/null 2>&1; then
  cd iii-smoke-test
  docker compose up -d
  sleep 8
  # Port check — adjust if the engine exposes a different healthz route.
  if nc -z localhost 3111 2>/dev/null; then
    echo "OK: iii engine reachable on :3111"
  else
    echo "FAIL: engine not reachable; logs:"
    docker compose logs iii
    docker compose down -v
    exit 1
  fi
  docker compose down -v
else
  echo "skip: docker not available locally"
fi
```

Expected: `OK: iii engine reachable on :3111`, exit 0. If port 3111 isn't the right one, swap for whichever the engine exposes (see `engine/Dockerfile:9` — `EXPOSE 49134 3111 3112 9464`).

- [ ] **Step 10.3b: Smoke test generate-docker warns on missing project.ini**

Run from a fresh tempdir without an init:

```bash
mkdir -p /tmp/iii-no-ini
cd /tmp/iii-no-ini
../iii/target/release/iii project generate-docker 2>&1 | tee out.txt
grep -q "warning:" out.txt && grep -q "iii project init" out.txt && echo "OK: warning present"
rm -rf /tmp/iii-no-ini
```

Expected: `OK: warning present`. Confirms the silent-fallback gap from the DX review is closed.

- [ ] **Step 10.4: Verify telemetry pipeline still picks it up**

Read `engine/src/workers/telemetry/mod.rs:73` — the existing `find_project_root` looks for `.iii/project.ini`. Smoke test that the engine starting from inside `/tmp/iii-smoke-test` with the new layout doesn't blow up:

```bash
target/release/iii --use-default-config --no-update-check &
SERVER_PID=$!
sleep 2
kill $SERVER_PID
```

Expected: process starts cleanly, no stack trace about missing telemetry config or project.ini.

- [ ] **Step 10.5: Cleanup**

```bash
rm -rf /tmp/iii-smoke-test
```

---

## Phase 11 — Linear ticket handoff to review

### Task 11: Post completion comments and move tickets to In Review

This phase only runs **after Phase 10 smoke tests pass**. We do **not** close tickets here — closing is a downstream signal that means "merged + verified in main." This phase moves the tickets to **In Review** so a reviewer (PR author or peer) can take it from there.

The Motia team workflow uses these states (verified via `linear-cli statuses list --team Motia`):
`Backlog → Todo → In Progress → In Review → Merged → Done` (with `Canceled` / `Duplicate` as terminal sidebands).

- [ ] **Step 11.1: Post completion comment on MOT-3242**

```bash
linear-cli comments create MOT-3242 -b "**Implementation complete — moving to In Review**

\`iii project init\` lands with:
- Scaffolds \`.iii/project.ini\` (with device_id), \`config.yaml\`, \`iii.lock\`, \`.gitignore\`, \`data/\`.
- Optional \`--docker\` flag wires Docker generation in one step.
- Structured errors (problem / cause / fix) and a \`Next steps:\` block in success output.
- README quickstart updated to point at the new command.
- e2e tests in \`engine/tests/project_init_e2e.rs\`; manual smoke test green.
- Telemetry: \`project_init_succeeded\` / \`project_init_failed\` events.

Implementation plan: \`docs/superpowers/plans/2026-05-06-project-init-and-docker.md\`. Ready for code review." -q
```

- [ ] **Step 11.2: Post completion comment on MOT-3241**

```bash
linear-cli comments create MOT-3241 -b "**Implementation complete — moving to In Review**

\`iii project generate-docker\` lands with:
- Generates \`Dockerfile\`, \`docker-compose.yml\`, \`.env\` into the project root.
- Hard-codes user device_id via \`III_HOST_USER_ID\` env (replaces the broken Docker-mount approach).
- \`docker-compose.yml\` ships with Redis and RabbitMQ commented out; \`.env\` includes a randomly generated RabbitMQ password.
- Warns (does not silently degrade) when \`.iii/project.ini\` is missing.
- Verified: \`docker compose up\` reaches the engine on :3111.

Implementation plan: \`docs/superpowers/plans/2026-05-06-project-init-and-docker.md\`. Ready for code review." -q
```

- [ ] **Step 11.3: Post completion comment on MOT-3276**

```bash
linear-cli comments create MOT-3276 -b "**Implementation complete — moving to In Review**

device_id is now written into both targets at project init:
- \`.iii/project.ini\` via \`run_init\` (Phase 6 of the plan).
- Docker \`.env\` as \`III_HOST_USER_ID\` via \`write_docker_assets\` (Phase 5).

The same value flows through both, verified by the e2e test \`project_init_with_docker_flag_writes_docker_assets_with_device_id\`. Existing engine pipeline (\`engine/src/workers/telemetry/environment.rs:198\`) already consumes \`III_HOST_USER_ID\` to derive the container device_id, so no engine changes were needed.

Implementation plan: \`docs/superpowers/plans/2026-05-06-project-init-and-docker.md\`. Ready for code review." -q
```

- [ ] **Step 11.4: Move the three tickets to In Review**

```bash
linear-cli issues update MOT-3242 -s "In Review"
linear-cli issues update MOT-3241 -s "In Review"
linear-cli issues update MOT-3276 -s "In Review"
```

Expected: each command prints the new state (In Progress → In Review). State name is case-sensitive — copy exactly.

> **Do not run `linear-cli issues close` here.** Closing moves tickets to Done, which is the wrong signal at this stage. A reviewer (or the merge automation) takes the ticket from In Review → Merged → Done after the PR lands.

- [ ] **Step 11.5: Verify**

```bash
for id in MOT-3242 MOT-3241 MOT-3276; do
  state=$(linear-cli issues get "$id" 2>&1 | grep -E "^State:" | awk -F'State:[[:space:]]*' '{print $2}' | tr -d ' ')
  echo "$id: $state"
done
```

Expected output:

```
MOT-3242: InReview
MOT-3241: InReview
MOT-3276: InReview
```

If any row shows `InProgress` (still) or `Done` (closed by mistake), re-run the matching `issues update` command from step 11.4.

---

## Self-Review Checklist

**Spec coverage:**
- ✅ MOT-3242 (project init): Phase 3 (scaffold) + Phase 6 (wire) + Phase 7 (e2e).
- ✅ MOT-3241 (Docker generation): Phase 4 (templates) + Phase 5 (docker.rs) + Phase 6 (wire) + Phase 7 (e2e) + Phase 10.3a (compose up).
- ✅ MOT-3276 (device_id injection): covered in Phase 6 (`run_init` writes device_id to project.ini) + Phase 5 (.env contains `III_HOST_USER_ID`).
- ✅ Redis/RabbitMQ commented out: Phase 4 template + Phase 5 test asserts.
- ✅ Random `.env` password: Phase 5 step 5.1 + test asserts non-determinism.
- ✅ Hard-coded device_id replaces Docker mounts: implicit — generated Dockerfile uses `ENV III_HOST_USER_ID` instead of mounts.
- ⚠️ Init in non-empty dir (git-init style behavior): not explicitly decided. Current plan: `write_if_absent` skips existing files (Phase 3 step 3.2 and Phase 5 step 5.1 both test this). If Anthony wants harder failure on non-empty dirs, swap `write_if_absent` for an early return.

**DX review coverage (`/devex-review` patches applied):**
- ✅ Hello-world cliff: Phase 6 prints `Next steps:` block + docs link; Phase 7 asserts.
- ✅ Structured errors (problem/cause/fix): Phase 6 `print_err` triad; Phase 7 asserts on `/dev/null/...` failure path.
- ✅ README quickstart: Phase 8.5.
- ✅ Verb shape locked before coding: Phase 0 step 0.1 hardened to a gate.
- ✅ Migration warning when `generate-docker` runs without `project.ini`: Phase 6 `warn_missing_project_ini`; Phase 7 + Phase 10.3b assert.
- ✅ Real `docker compose up` smoke test: Phase 10.3a (Docker-conditional).
- ✅ Telemetry on init success/failure: Phase 5.5 helpers + Phase 6 callsites.

**Linear lifecycle coverage:**
- ✅ Tickets moved to **In Progress** after the verb gate clears: Phase 0 step 0.3.
- ✅ Commit messages include `Refs MOT-XXXX` per the phase→ticket mapping table at the end of Phase 0.
- ✅ Per-ticket completion comments posted: Phase 11 steps 11.1, 11.2, 11.3.
- ✅ All three tickets moved to **In Review** (not Done) after Phase 10 smoke tests pass: Phase 11 step 11.4.
- ✅ Closing to Done is explicitly out of scope for this plan — that's a downstream signal handled by reviewer / merge automation.
- ✅ Verification that the In Review handoff landed: Phase 11 step 11.5.

**Placeholder scan:** Searched for "TBD", "TODO", "implement later" — none present. All steps have full code.

**Type consistency:**
- `ProjectIni` struct: 4 `Option<String>` fields throughout (Phase 2 def, Phase 6 use, Phase 7 test).
- `write_scaffold(&Path)`: signature consistent (Phase 3 def, Phase 6 call).
- `write_docker_assets(&Path, &str)`: signature consistent (Phase 5 def, Phase 6 call).
- `InitArgs.directory: Option<String>` and `.docker: bool`: matches Phase 1 tests, Phase 6 use, Phase 7 e2e flag passing.
- `GenerateDockerArgs.directory: Option<String>`: matches Phase 1 tests + Phase 7 e2e usage.
- `send_project_init_succeeded(bool, &str)` / `send_project_init_failed(&str, &str)`: defined Phase 5.5, called Phase 6.

**Open assumption to confirm with Anthony before merging:**
- Verb: option A `iii project init` + `iii project generate-docker`, option B `iii project init` + `iii project gen docker`. Locked at Phase 0 step 0.1 before Phase 1 starts. Plan currently assumes A.

---

## Execution Handoff

Plan complete and saved to `docs/superpowers/plans/2026-05-06-project-init-and-docker.md`. Two execution options:

**1. Subagent-Driven (recommended)** — I dispatch a fresh subagent per phase, review between phases, fast iteration.

**2. Inline Execution** — Execute phases in this session using executing-plans, batch execution with checkpoints.

**Which approach?**

