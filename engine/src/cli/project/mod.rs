// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

//! `iii project` subcommand dispatch.
//!
//! All template content (the bare scaffold's `config.yaml`/`.gitignore` plus
//! the Docker assets) lives in the canonical templates repo
//! (`iii-hq/templates`). The engine never embeds template content via
//! `include_str!`; everything is fetched at runtime through
//! [`scaffolder_core::TemplateFetcher`]. This decouples template fixes from
//! engine releases — see iii-hq/templates#2 for the templates that back this
//! command.

use clap::{Args, Subcommand};
use colored::Colorize;
use scaffolder_core::cli::{
    apply_template_idempotent, build_fetcher, check_directory_state, print_err, resolve_root,
};
use scaffolder_core::{IiiConfig, TemplateFetcher};
use std::io::IsTerminal;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

#[derive(Args, Debug, Clone)]
pub struct ProjectArgs {
    #[command(subcommand)]
    pub action: ProjectAction,
}

#[derive(Subcommand, Debug, Clone)]
pub enum ProjectAction {
    /// Initialize a new iii project in the current directory
    Init(InitArgs),
    /// Generate Docker assets (Dockerfile, docker-compose.yml, .env) for an existing iii project
    GenerateDocker(GenerateDockerArgs),
}

#[derive(Args, Debug, Clone)]
pub struct InitArgs {
    /// Target directory for the new project (positional). Ignored when
    /// --directory is given. The project name is the resolved directory's
    /// name.
    #[arg(value_name = "NAME")]
    pub name: Option<String>,

    /// Target directory. Takes precedence over NAME. If neither NAME nor
    /// --directory is provided, the directory defaults to the current
    /// directory.
    #[arg(short, long)]
    pub directory: Option<String>,

    /// Also generate Docker assets (Dockerfile, docker-compose.yml, .env).
    /// Equivalent to running `iii project generate-docker` separately.
    #[arg(long)]
    pub docker: bool,

    /// Scaffold from a named template (e.g. "quickstart"). Triggers the
    /// interactive scaffolder TUI.
    #[arg(short, long)]
    pub template: Option<String>,

    /// Local directory to use for templates instead of fetching from remote
    /// (for template development and tests).
    #[arg(long = "template-dir")]
    pub template_dir: Option<String>,

    /// Skip the iii-engine version compatibility check.
    #[arg(long = "skip-iii")]
    pub skip_iii: bool,

    /// Allow initialization into a non-empty directory. Without this flag, init
    /// errors out if the target dir contains anything other than hidden
    /// dotfiles (e.g. `.git/`). Re-running init in a directory with
    /// `.iii/project.ini` is always allowed (idempotent re-init).
    #[arg(long = "allow-non-empty")]
    pub allow_non_empty: bool,

    /// Start the iii harness and take a quick look at what iii can do:
    /// scaffold the "harness" template into NAME, or into ./learn-iii
    /// (learn-iii-1, learn-iii-2, ... when taken) if no NAME is given, then
    /// start `iii compose --up` inside it. Cannot be combined with any other
    /// scaffolding option.
    #[arg(long = "learn-iii", conflicts_with_all = ["directory", "template", "docker", "template_dir"])]
    pub learn_iii: bool,

    /// Start the harness with these workers already declared: a
    /// comma-separated list. The project directory is named after the first
    /// worker (`--start-with worker1` scaffolds `./iii-worker1`), and every
    /// worker in the list is added through `compose::add` once the project is
    /// up.
    #[arg(
        long = "start-with",
        value_delimiter = ',',
        value_name = "WORKERS",
        requires = "learn_iii"
    )]
    pub start_with: Vec<String>,

    /// Environment variables to ask for on top of the inference provider key,
    /// comma-separated. A worker outside the provider list needs its own key
    /// this way: `--start-with worker1 --need-envs WORKER_API_KEY`. Each
    /// answer is written to the new project's `.env`.
    #[arg(
        long = "need-envs",
        value_delimiter = ',',
        value_name = "VARS",
        requires = "learn_iii"
    )]
    pub need_envs: Vec<String>,
}

impl InitArgs {
    /// Resolved target directory: --directory wins, positional name is fallback.
    fn target_dir(&self) -> Option<&str> {
        self.directory.as_deref().or(self.name.as_deref())
    }
}

#[derive(Args, Debug, Clone)]
pub struct GenerateDockerArgs {
    /// Target directory (defaults to current directory)
    #[arg(short, long)]
    pub directory: Option<String>,

    /// Local directory to use for templates instead of fetching from remote
    /// (for template development and tests).
    #[arg(long = "template-dir")]
    pub template_dir: Option<String>,
}

fn template_flow_requested(args: &InitArgs) -> bool {
    // Only --template triggers the interactive scaffolder TUI. The bare flow
    // also uses scaffolder-core under the hood, but goes through the
    // non-interactive `apply_template` helper.
    args.template.is_some()
}

pub async fn run(args: ProjectArgs) -> i32 {
    match args.action {
        ProjectAction::Init(init) => run_init(init).await,
        ProjectAction::GenerateDocker(gd) => run_generate_docker(gd).await,
    }
}

async fn run_init(args: InitArgs) -> i32 {
    if args.learn_iii {
        return run_learn_iii(args).await;
    }
    if template_flow_requested(&args) {
        return run_init_with_template(args).await;
    }

    let target = args.target_dir().map(|s| s.to_string());
    let root = match resolve_root(target.as_deref()) {
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

    if let Err(e) = check_directory_state(&root, args.allow_non_empty, "project.ini") {
        crate::cli::telemetry::send_project_init_failed("non_empty_dir", &e);
        return print_err(
            "target directory is not empty",
            &e,
            "pass --allow-non-empty to scaffold into an existing project, or pick a different directory",
        );
    }

    let device_id = iii::workers::telemetry::environment::get_or_create_device_id();
    let project_name = root
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("iii-project")
        .to_string();

    // Fetch + apply the canonical 'bare' template. Existing project_id is
    // preserved on re-runs.
    let mut fetcher = match build_fetcher(args.template_dir.as_deref()) {
        Ok(f) => f,
        Err(e) => {
            crate::cli::telemetry::send_project_init_failed("fetcher", &e.to_string());
            return print_err(
                "could not build template fetcher",
                &e.to_string(),
                "check III_TEMPLATE_URL or pass --template-dir <path>",
            );
        }
    };

    if let Err(e) = apply_template_idempotent(&mut fetcher, "bare", &root).await {
        crate::cli::telemetry::send_project_init_failed("apply_bare", &e.to_string());
        return print_err(
            "could not apply 'bare' template",
            &e.to_string(),
            "see template fetch error above",
        );
    }

    let project_id = match persist_project_ini(&root, &project_name, "init", &device_id).await {
        Ok(id) => id,
        Err(e) => {
            crate::cli::telemetry::send_project_init_failed("write_project_ini", &e.to_string());
            return print_err(
                "could not write .iii/project.ini",
                &e.to_string(),
                "check that the target directory is writable",
            );
        }
    };

    if args.docker
        && let Err(e) = apply_docker(&mut fetcher, &root, &device_id).await
    {
        crate::cli::telemetry::send_project_init_failed("apply_docker", &e.to_string());
        return print_err(
            "could not apply 'docker' template",
            &e.to_string(),
            "remove existing Dockerfile/docker-compose.yml or check write permissions",
        );
    }

    crate::cli::telemetry::send_project_init_succeeded(args.docker, &project_id);

    print_init_success(&project_name, &root, target.is_some(), args.docker);
    0
}

async fn run_init_with_template(args: InitArgs) -> i32 {
    // Restore terminal cursor on panic and on Ctrl+C — scaffolder runs an
    // interactive TUI via cliclack and we don't want to leave the cursor hidden.
    let default_panic = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let _ = console::Term::stderr().show_cursor();
        default_panic(info);
    }));
    let _ = ctrlc::set_handler(move || {
        let _ = console::Term::stderr().show_cursor();
        // `--learn-iii` runs `iii compose --up` as a child, and Ctrl+C reaches
        // every process in the foreground group. Compose stops the project
        // itself; exiting here would hand the shell a prompt while that
        // teardown still writes to the terminal, which reads as a hang and
        // takes a second Ctrl+C to finish.
        if CHILD_OWNS_TERMINAL.load(Ordering::Relaxed) {
            return;
        }
        std::process::exit(130);
    });

    let target_dir = args.target_dir().map(PathBuf::from);
    let create_args = scaffolder_core::tui::CreateArgs {
        template_dir: args.template_dir.as_ref().map(PathBuf::from),
        template: args.template.clone(),
        directory: target_dir.clone(),
        languages: None,
        skip_tool_check: args.skip_iii,
        skip_install: false,
        // --learn-iii prints its own "starting the tour" line right after.
        skip_next_steps: args.learn_iii,
        yes: false,
    };

    let result = scaffolder_core::run(&IiiConfig, create_args, env!("CARGO_PKG_VERSION")).await;
    let _ = console::Term::stderr().show_cursor();

    if let Err(e) = result {
        crate::cli::telemetry::send_project_init_failed("scaffolder", &e.to_string());
        return print_err(
            "template scaffold failed",
            &e.to_string(),
            "see scaffolder output above",
        );
    }

    let project_id_for_event = if let Some(root) = target_dir.as_ref() {
        if root.is_dir() {
            let device_id = iii::workers::telemetry::environment::get_or_create_device_id();
            let project_name = root
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("iii-project")
                .to_string();
            let template_label = args.template.as_deref().unwrap_or("init-template");
            let id = persist_project_ini(root, &project_name, template_label, &device_id)
                .await
                .unwrap_or_default();

            if args.docker {
                let mut fetcher = match build_fetcher(args.template_dir.as_deref()) {
                    Ok(f) => f,
                    Err(e) => {
                        crate::cli::telemetry::send_project_init_failed("fetcher", &e.to_string());
                        return print_err(
                            "could not build template fetcher for docker assets",
                            &e.to_string(),
                            "check III_TEMPLATE_URL or pass --template-dir <path>",
                        );
                    }
                };
                if let Err(e) = apply_docker(&mut fetcher, root, &device_id).await {
                    crate::cli::telemetry::send_project_init_failed("apply_docker", &e.to_string());
                    return print_err(
                        "could not apply 'docker' template",
                        &e.to_string(),
                        "remove existing Dockerfile/docker-compose.yml or check write permissions",
                    );
                }
            }

            id
        } else {
            String::new()
        }
    } else {
        // Interactive flow — no known directory, no project_id retrofit.
        String::new()
    };

    crate::cli::telemetry::send_project_init_succeeded(args.docker, &project_id_for_event);
    0
}

const LEARN_III_TEMPLATE: &str = "harness";
/// The base image every worker in the `--learn-iii` project starts from.
/// Also `oci_image_for_kind`'s answer for JavaScript and TypeScript, and its
/// fallback for an unrecognised kind, so it is the right first guess before
/// the template has been written and its manifests can be read.
const LEARN_III_BASE_IMAGE: &str = "docker.io/iiidev/node:latest";

/// Start downloading base images in the background.
///
/// The first boot of a new project waits on an image that is hundreds of
/// megabytes, and the operator spends the minute before it choosing a
/// template and pasting a provider key. This puts the download in that
/// minute instead of after it.
///
/// The work runs in `iii-worker`, which owns the rootfs cache; the engine
/// does not link that crate. Both processes share the cache on disk and take
/// its per-image lock, so this racing the real pull costs at worst a wait.
///
/// `None` when `iii-worker` cannot be found or will not start. That is not
/// worth reporting: nothing is missing yet, and the spawn that needs the
/// image pulls it in the usual place with the usual errors.
fn start_image_prefetch(images: &[String]) -> Option<tokio::process::Child> {
    if images.is_empty() {
        return None;
    }
    let worker = iii::bin_resolve::find_existing_binary("iii-worker")?;
    tokio::process::Command::new(worker)
        .arg("__pull-images")
        .args(images)
        // The operator is reading a menu. A pull that printed onto it, or a
        // failure line for an image nothing has asked for yet, would only be
        // noise.
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .ok()
}

/// Every distinct `runtime.base_image` declared by a worker under `dir`.
///
/// Read after scaffolding to catch whatever the template actually shipped,
/// which need not be the image [`start_image_prefetch`] was already given.
fn declared_base_images(dir: &Path) -> Vec<String> {
    fn walk(dir: &Path, depth: usize, out: &mut Vec<String>) {
        // Worker manifests sit at the top of a worker directory. A handful of
        // levels reaches them under `workers/<name>/` without descending into
        // `node_modules` and friends.
        if depth > 3 {
            return;
        }
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        if let Ok(Some(manifest)) = iii_compose::manifest::read_manifest(dir)
            && let Some(image) = manifest.base_image
            && !out.contains(&image)
        {
            out.push(image);
        }
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if name.starts_with('.') || name == "node_modules" || name == "target" {
                continue;
            }
            walk(&path, depth + 1, out);
        }
    }
    let mut out = Vec::new();
    walk(dir, 0, &mut out);
    out
}

const LEARN_III_DIR: &str = "learn-iii";

/// The directory base name for a tour with no NAME: `learn-iii`, or
/// `iii-<first worker>` when `--start-with` names one.
///
/// A worker reference carries more than a name (`scope/worker1@1.2.0`), and none
/// of it belongs in a directory name: the registry scope would nest the
/// project a level down, and the version would pin the directory to a release.
fn learn_dir_base(start_with: &[String]) -> String {
    let Some(first) = start_with.first() else {
        return LEARN_III_DIR.to_string();
    };
    let name = worker_name(first);
    if name.is_empty() {
        LEARN_III_DIR.to_string()
    } else {
        format!("iii-{name}")
    }
}

/// The worker's own name inside a spec: `scope/name@version` -> `name`.
fn worker_name(spec: &str) -> &str {
    spec.rsplit('/')
        .next()
        .unwrap_or(spec)
        .split('@')
        .next()
        .unwrap_or(spec)
        .trim()
}

/// The tour's worker, seeded into the compose file by
/// [`seed_onboarding_container`] and served behind the layout's second pane.
const ONBOARDING_WORKER: &str = "onboarding";

/// `iii project init --learn-iii [NAME]`: same as `iii project init -t harness
/// <NAME>`, then `iii compose --up` from inside the new directory. Without
/// NAME the directory is the first free `learn-iii` name; a given NAME is
/// used as-is, so a taken one fails the same way plain init does.
async fn run_learn_iii(mut args: InitArgs) -> i32 {
    let start_with = std::mem::take(&mut args.start_with);
    let need_envs = std::mem::take(&mut args.need_envs);
    let dir = match args.name.as_deref() {
        Some(name) => PathBuf::from(name),
        None => next_free_dir(Path::new(""), &learn_dir_base(&start_with)),
    };
    args.template = Some(LEARN_III_TEMPLATE.to_string());
    args.directory = Some(dir.to_string_lossy().into_owned());

    // Before the scaffolder's own menu, not after it: `run_init_with_template`
    // runs an interactive TUI, so this is the earliest moment the download can
    // start and the longest stretch of thinking time it can hide behind.
    let mut prefetch = start_image_prefetch(&[LEARN_III_BASE_IMAGE.to_string()]);

    let code = run_init_with_template(args).await;
    if code != 0 {
        return code;
    }

    // The tour's own worker is declared in the compose file, not added at
    // runtime, and the seeded layout opens a pane onto the page it serves. A
    // `--start-with` project is not the tour unless it asks for it, so both
    // seeds follow the list: `--start-with onboarding,...` gets exactly what
    // the bare command scaffolds, and a list without it gets neither the
    // container nor a pane pointing at a page nothing serves.
    //
    // Matched exactly, not by worker name: the seed declares
    // `package://onboarding` at `latest`, so it can only stand in for a
    // request that asked for precisely that. `acme/onboarding@1.2.0` names a
    // different registry and a pinned version, and is added like any other
    // worker.
    let with_onboarding =
        start_with.is_empty() || start_with.iter().any(|spec| is_tour_worker(spec));
    if with_onboarding {
        seed_console_layout(&dir);
        seed_onboarding_container(&dir);
    }

    // Seeded means already declared, so `compose::add` has nothing to do for
    // it, and adding it anyway would rewrite the block the seed just wrote.
    let start_with: Vec<String> = start_with
        .into_iter()
        .filter(|spec| !(with_onboarding && is_tour_worker(spec)))
        .collect();

    // The template is on disk now, so its manifests can say what they really
    // need. Anything beyond the image already being fetched gets its own pass.
    let extra: Vec<String> = declared_base_images(&dir)
        .into_iter()
        .filter(|image| image != LEARN_III_BASE_IMAGE)
        .collect();
    let mut extra_prefetch = start_image_prefetch(&extra);

    let exe = match std::env::current_exe() {
        Ok(p) => p,
        Err(e) => {
            return print_err(
                "could not locate the iii binary",
                &e.to_string(),
                &format!("cd {} && iii compose --up", dir.display()),
            );
        }
    };

    prompt_provider_key(&dir);
    let extra_env = prompt_extra_env_keys(&dir, &need_envs);

    let hint = format!("cd ./{} && iii compose --up", dir.display());
    eprintln!();
    eprintln!("  {} starting the tour: {}", "▶".green(), hint.bold());
    eprintln!();

    // Let the downloads finish before compose asks for the same images.
    // `ensure_rootfs` takes a per-image lock, so an overlap would be safe but
    // pointless: compose would sit on the lock with nothing on screen, while
    // waiting here keeps one pull visible in one place. By now the operator
    // has read a menu and pasted a key, so this is usually already done.
    for child in [prefetch.as_mut(), extra_prefetch.as_mut()]
        .into_iter()
        .flatten()
    {
        let _ = child.wait().await;
    }

    // Aborted when compose exits, so the poll inside needs no deadline of its
    // own: the tour's life is the deadline.
    let announcer = tokio::spawn(announce_console_when_ready(
        dir.join("worker-compose.yaml"),
        start_with,
    ));
    CHILD_OWNS_TERMINAL.store(true, Ordering::Relaxed);

    let code = match tokio::process::Command::new(exe)
        .args(["compose", "--up"])
        .current_dir(&dir)
        // `WORKER_API_KEY=... iii compose --up`: the same keys that went to the
        // project's `.env`, in this run's environment as well.
        .envs(extra_env)
        .status()
        .await
    {
        Ok(status) => status.code().unwrap_or(1),
        Err(e) => print_err("could not start `iii compose --up`", &e.to_string(), &hint),
    };

    CHILD_OWNS_TERMINAL.store(false, Ordering::Relaxed);
    announcer.abort();
    // The key reader may still be parked on stdin in cbreak mode; the shell
    // must not get its terminal back with ECHO off.
    restore_terminal_mode();
    code
}

/// Set while `iii compose --up` runs as our child, so the Ctrl+C handler above
/// leaves the interrupt to compose.
/// The console keeps its pane layout in the `console` configuration entry,
/// which the engine's file adapter stores at `<project>/config/<id>.yaml`.
/// Writing it before the first boot is what opens the tour beside the chat:
/// with no stored value the console seeds its own chat+traces default
/// instead (`register_console_config` only sends `initial_value` when the
/// entry is absent), and re-registration never overwrites a stored value —
/// so this survives restarts, and the operator's own tab edits write back to
/// the same file.
///
/// `name` and `description` are not optional on disk: an entry missing them
/// fails to parse and the adapter skips the file.
const CONSOLE_LAYOUT_SEED: &str = "\
id: console
name: Console
description: Console server and UI settings.
metadata:
  ui_form: console
value:
  workspace:
    tabs:
      - id: tab-home
        columns: 2
        screens:
          - chat
          - \"ext:onboarding\"
        sizes:
          - 0.6
          - 0.4
";

/// The tour's own worker, declared in the project's compose file.
///
/// The layout seed above opens a pane on `ext:onboarding`, and that page is
/// served by the `onboarding` worker — a page injected by a worker is only
/// there while the worker runs. Without this the seeded pane opens on the
/// console's "Extension page not loaded" placeholder.
///
/// A version is not optional for a `package://` container (compose rejects
/// the file without one), so this tracks the `latest` release tag: new
/// releases of the tour reach a new project with no engine release.
///
/// A tag, not a semver range, because the registry resolves range selectors
/// only against versions promoted to `latest`, while an exact pin is the
/// only escape hatch for an unpromoted one. A prerelease like
/// `0.1.0-experimental` therefore matches no range — `^0.1.0` excludes
/// prereleases outright — but the `latest` tag selector resolves it the
/// moment that tag points at it.
const ONBOARDING_CONTAINER: &str = "\
  # The guided tour. It serves the `onboarding` console page that the seeded
  # workspace layout opens beside the chat.
  onboarding:
    worker: package://onboarding
    version: \"latest\"
    start_after:
      - state

";

/// Insert the tour's container into a `worker-compose.yaml` body, or return
/// `None` when there is nothing to do: no `containers:` mapping to insert
/// into, or a container by that name already declared (a re-run, or an
/// operator who added their own).
///
/// A text insert, not a YAML round-trip: the harness template's compose file
/// is half instructive comments, and `serde_yaml` would drop every one of
/// them.
fn with_onboarding_container(text: &str) -> Option<String> {
    if text.lines().any(|line| line.trim_end() == "  onboarding:") {
        return None;
    }
    let heading = "containers:\n";
    let start = if text.starts_with(heading) {
        0
    } else {
        text.find(&format!("\n{heading}"))? + 1
    };
    let insert_at = start + heading.len();
    let mut patched = String::with_capacity(text.len() + ONBOARDING_CONTAINER.len());
    patched.push_str(&text[..insert_at]);
    patched.push_str(ONBOARDING_CONTAINER);
    patched.push_str(&text[insert_at..]);
    Some(patched)
}

/// Best effort, like the layout seed. A project whose compose file cannot
/// take the container still starts; its tour pane is the placeholder until
/// someone declares the worker.
fn seed_onboarding_container(dir: &Path) {
    let path = dir.join("worker-compose.yaml");
    let Ok(text) = std::fs::read_to_string(&path) else {
        return;
    };
    if let Some(patched) = with_onboarding_container(&text) {
        let _ = std::fs::write(&path, patched);
    }
}

/// Open the new project's console on chat beside the tour, 70/30.
///
/// `sizes` are index-aligned with the columns and normalized by their sum;
/// a list whose length does not match the column count is ignored in favour
/// of equal widths (`tabSizes` in the console's workspace model), which is
/// what the pane-count assertion in the tests guards.
///
/// `http_port` is deliberately absent: the console backfills the port it
/// actually bound, which matters because it moves to the next free port when
/// the configured one is taken.
///
/// Best effort. A project that cannot take the seed still starts; its console
/// just opens on the stock layout.
fn seed_console_layout(dir: &Path) {
    let config_dir = dir.join("config");
    let path = config_dir.join("console.yaml");
    if path.exists() {
        return;
    }
    if std::fs::create_dir_all(&config_dir).is_err() {
        return;
    }
    let _ = std::fs::write(&path, CONSOLE_LAYOUT_SEED);
}

static CHILD_OWNS_TERMINAL: AtomicBool = AtomicBool::new(false);

/// The console worker's configuration entry, and its own default port for
/// when that entry has no `http_port` yet.
const CONSOLE_CONFIG: &str = "console";
const DEFAULT_CONSOLE_PORT: u16 = 3113;
const READY_POLL_INTERVAL: std::time::Duration = std::time::Duration::from_millis(500);
/// Room for compose's startup renderer to print its closing line and stop
/// repainting before anything else writes to the terminal.
const BLOCK_SETTLE: std::time::Duration = std::time::Duration::from_millis(1_000);

/// Waits for the tour's project to serve, then points the user at the console
/// and opens it on request.
///
/// The gate is every declared container settling through `compose::status`,
/// which is also when compose's startup renderer lets go of the terminal: it owns one global in-place block for the whole of `--up` and
/// repaints it, so a banner printed before then is overwritten and the user
/// never sees it. A ready console has not necessarily bound its listener yet,
/// so the port is checked too.
///
/// `start_with` workers are declared here too, through the same client and the
/// same readiness gate: `compose::add` needs a serving daemon, and this is the
/// one place that already waits for one.
async fn announce_console_when_ready(compose_path: PathBuf, start_with: Vec<String>) {
    let Ok(file) = iii_compose::config::ComposeFile::load(&compose_path) else {
        return;
    };
    let namespace = file
        .namespace
        .clone()
        .unwrap_or_else(|| "default".to_string());
    let Some(engine) = file.engine.as_ref() else {
        return;
    };

    let client =
        iii_compose::engine::EngineClient::connect(&engine.url, "iii-cli:learn-iii", &namespace);

    wait_until_settled(&client, &file.path, &namespace).await;

    if !start_with.is_empty() {
        // Before the console wait, not after: a project whose console failed
        // to bind still takes its `--start-with` workers.
        add_workers(&client, &file.path, &namespace, &start_with).await;

        // The add starts the workers it declared, which is a second startup
        // with a second block of its own. The console link belongs after it,
        // both because the added workers are part of what the link opens onto
        // and because a link printed into that block is painted over.
        wait_until_settled(&client, &file.path, &namespace).await;
    }

    let port = client
        .fetch_config(CONSOLE_CONFIG)
        .await
        .ok()
        .flatten()
        .and_then(|config| {
            config
                .get("http_port")
                .and_then(|port| port.as_u64())
                .and_then(|port| u16::try_from(port).ok())
        })
        .unwrap_or(DEFAULT_CONSOLE_PORT);

    while tokio::net::TcpStream::connect(("127.0.0.1", port))
        .await
        .is_err()
    {
        tokio::time::sleep(READY_POLL_INTERVAL).await;
    }

    let url = format!("http://127.0.0.1:{port}");
    eprintln!();
    eprintln!(
        "  {} Open your browser to continue: {}",
        "▶".green(),
        url.bold()
    );
    eprintln!("    Press {} to open it here.", "b".bold());
    eprintln!();

    // A blocking read on its own thread, not a blocking task: the process must
    // be free to exit with compose while this is still parked on stdin.
    std::thread::spawn(move || open_console_on_request(&url));
}

/// Declare `workers` in the running project: the `iii trigger compose::add`
/// call, made from here so a `--start-with` tour needs no second terminal.
///
/// One request for the whole list, because `compose::add` takes a `workers`
/// list and reconciles it once; a call per worker would restart the project
/// once per name.
///
/// `compose::add` answers as soon as it has *accepted* the work, not when the
/// work is done: the mutation runs on its own task and reports through the
/// `compose-operation` trigger type. So the completion this waits on is that
/// trigger's terminal event, bound before the add is submitted and against an
/// operation id chosen here, which is what makes it impossible to miss. The
/// timeout below is the backup, not the mechanism.
///
/// Best effort, like every other step of the tour: a failed add leaves a
/// running project the operator can add to by hand.
async fn add_workers(
    client: &iii_compose::engine::EngineClient,
    compose_path: &Path,
    namespace: &str,
    workers: &[String],
) {
    if workers.is_empty() {
        return;
    }
    let names = workers.join(", ");
    eprintln!();
    eprintln!("  {} adding workers: {}", "▶".green(), names.bold());

    // Ours, not the daemon's: an id we picked can be subscribed to before the
    // add exists, and `compose::add` adopts it. Waiting for the id in the add's
    // answer would leave a window where the operation could finish before the
    // binding landed.
    let operation_id = format!("compose:learn-iii:{}", uuid::Uuid::new_v4());
    let (binding, mut finished) = subscribe_to_operation(client, &operation_id);

    let payload = serde_json::json!({
        "file": compose_path,
        "workers": worker_declarations(compose_path, workers),
        "operation_id": operation_id,
    });

    // The daemon registers its functions in one pass, in the order it lists
    // them, and `compose::add` comes after `compose::status`. The settle gate
    // is a `compose::status` answer, so the add can arrive inside that pass
    // and find the function it needs not registered yet. Retried only for
    // that error, because it is the one that means nothing ran: `compose::add`
    // is not idempotent, and a retry on anything else could add twice.
    let deadline = std::time::Instant::now() + ADD_REGISTRATION_GRACE;
    loop {
        let request = iii_sdk::protocol::TriggerRequest {
            function_id: "compose::add".to_string(),
            payload: payload.clone(),
            action: None,
            timeout_ms: Some(ADD_TIMEOUT_MS),
        }
        .namespace(namespace);

        match client.client().trigger(request).await {
            Ok(_) => break,
            Err(e)
                if e.to_string().contains("function_not_found")
                    && std::time::Instant::now() < deadline =>
            {
                tokio::time::sleep(READY_POLL_INTERVAL).await;
            }
            Err(e) => {
                if let Some(binding) = binding {
                    binding.unregister();
                }
                eprintln!("  could not add {names}: {e}");
                eprintln!("  add them yourself with: iii trigger compose::add worker=<name>");
                return;
            }
        }
    }

    // The backup. A binding the daemon never delivered on -- an old daemon with
    // no `compose-operation` trigger type, a socket that dropped and came back
    // after the terminal event -- would otherwise park the tour here forever.
    // Falling through costs nothing: the caller's settle poll is the same gate
    // this was saving.
    let _ = tokio::time::timeout(
        std::time::Duration::from_millis(ADD_TIMEOUT_MS),
        finished.recv(),
    )
    .await;

    if let Some(binding) = binding {
        binding.unregister();
    }
}

/// Binds `compose-operation` for one operation's terminal event.
///
/// The receiver yields once, when that operation finishes, whether it
/// succeeded or failed: both are the end of the add as far as the tour is
/// concerned, and the state the containers ended in is what the settle gate
/// afterwards reads. The binding is returned with it, because a [`Trigger`]
/// that is merely dropped stays registered -- the caller unregisters it once
/// it has its event.
///
/// [`Trigger`]: iii_sdk::trigger::Trigger
fn subscribe_to_operation(
    client: &iii_compose::engine::EngineClient,
    operation_id: &str,
) -> (
    Option<iii_sdk::trigger::Trigger>,
    tokio::sync::mpsc::UnboundedReceiver<()>,
) {
    let (done, finished) = tokio::sync::mpsc::unbounded_channel();
    let callback = format!("iii-cli::learn-iii::operation::{}", uuid::Uuid::new_v4());
    client.client().register_function(
        callback.clone(),
        iii_sdk::RegisterFunction::new(move |_: serde_json::Value| {
            let _ = done.send(());
            Ok(serde_json::Value::Null)
        }),
    );

    // `register_trigger` hands its message to the socket and does not wait for
    // the engine to answer, so an error here is the message never being sent
    // at all, and a daemon too old to serve `compose-operation` does not error
    // here either. The caller's timeout covers both.
    let binding = client
        .client()
        .register_trigger(iii_sdk::protocol::RegisterTriggerInput::new(
            "compose-operation",
            callback,
            serde_json::json!({ "operation_id": operation_id, "terminal_only": true }),
        ))
        .ok();

    (binding, finished)
}

/// `compose::add`'s own recommended timeout: the call resolves a registry
/// graph and pulls images before it answers.
const ADD_TIMEOUT_MS: u64 = 600_000;

/// How long [`add_workers`] keeps waiting for the daemon to finish registering
/// `compose::add`. Long enough for a registration pass, short enough that a
/// daemon which never registers it still lets the tour say so.
const ADD_REGISTRATION_GRACE: std::time::Duration = std::time::Duration::from_secs(30);

/// The `workers` entries for [`add_workers`].
///
/// `compose::add` takes either a bare spec (`"worker1"`) or a container object,
/// and only the object form carries the rest of a container's fields. The
/// object is what these workers need: `--need-envs` wrote its answers to the
/// project's `.env`, and a container reads that file only when it declares it
/// in `env_file`. A bare spec would be added with no environment at all.
///
/// The path stays relative, the way an operator would write it: compose
/// resolves `env_file` against the compose file's own directory, so `.env`
/// means this project's `.env` wherever the project is moved to.
///
/// A project with no `.env` gets the bare specs back: `env_file` naming a
/// missing file fails validation, and that would lose the add itself.
///
/// This is a trust boundary, and it is deliberately wide: every `--start-with`
/// worker receives the whole project `.env`, including keys meant for another
/// worker. The flag is for workers the operator already trusts. Narrowing it
/// would mean a per-worker env file, which is a different feature.
fn worker_declarations(compose_path: &Path, workers: &[String]) -> Vec<serde_json::Value> {
    let has_env = compose_path
        .parent()
        .map(|dir| dir.join(PROJECT_ENV_FILE))
        .is_some_and(|path| path.exists());

    workers
        .iter()
        .map(|worker| {
            if has_env {
                serde_json::json!({ "worker": worker, "env_file": [PROJECT_ENV_FILE] })
            } else {
                serde_json::json!(worker)
            }
        })
        .collect()
}

/// The env file the harness template ships and [`prompt_provider_key`] writes.
const PROJECT_ENV_FILE: &str = ".env";

/// Whether `spec` asks for the tour's own worker, the one the scaffold seeds
/// into `worker-compose.yaml`.
///
/// Exact, because the seed is one specific container: `package://onboarding`
/// at `latest`. A spec that names another registry or pins a version wants a
/// different worker and is added like any other.
fn is_tour_worker(spec: &str) -> bool {
    spec.trim() == ONBOARDING_WORKER
}

/// Waits for the project to stop moving, then for compose's startup renderer
/// to let go of the terminal.
///
/// "Nothing is starting" is also true before anything has started, so the wait
/// needs to have seen the project move first. Until it does, the gate is the
/// stricter "every container is ready", which is what a project with no
/// failing container reaches anyway.
async fn wait_until_settled(
    client: &iii_compose::engine::EngineClient,
    compose_path: &Path,
    namespace: &str,
) {
    let mut seen_moving = false;
    loop {
        if let Some(progress) = project_progress(client, compose_path, namespace).await {
            seen_moving |= progress.any_moving;
            if progress.all_ready || (seen_moving && !progress.any_moving) {
                break;
            }
        }
        tokio::time::sleep(READY_POLL_INTERVAL).await;
    }

    // ponytail: fixed settle delay. The renderer prints its closing line just
    // after the last container settles, and compose publishes no "startup
    // block finished" event to wait on instead. Swap this for that event if
    // compose ever grows one.
    tokio::time::sleep(BLOCK_SETTLE).await;
}

/// What one `compose::status` answer says about the project's progress.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Progress {
    /// Every declared container reports `ready`: startup finished with nothing
    /// left behind.
    all_ready: bool,
    /// At least one container is still moving on its own: `starting`, or
    /// `restarting` while the supervisor waits to try it again.
    any_moving: bool,
}

/// Read the project's progress from `compose::status`.
///
/// `None` is the daemon not serving `compose::status` yet, or an answer with
/// no containers in it, which is indistinguishable here from a project that
/// has not loaded: both mean "ask again".
async fn project_progress(
    client: &iii_compose::engine::EngineClient,
    compose_path: &Path,
    namespace: &str,
) -> Option<Progress> {
    let request = iii_sdk::protocol::TriggerRequest {
        function_id: "compose::status".to_string(),
        payload: serde_json::json!({ "file": compose_path }),
        action: None,
        timeout_ms: Some(10_000),
    }
    .namespace(namespace);

    let value = client.client().trigger(request).await.ok()?;
    let containers = value.get("containers")?.as_array()?;
    read_progress(containers)
}

/// The container states, as progress. Split from the call so the states that
/// matter can be tested without a daemon.
///
/// A container that failed, or was declared `required: false` and stopped,
/// never becomes ready. Waiting for it is waiting forever, so "every container
/// is ready" cannot be the only way out: the tour would print no console link
/// and add no `--start-with` worker over one optional container. The caller
/// pairs the `any_moving` half with "the project has moved at least once",
/// because a project that has not started yet has nothing starting either.
fn read_progress(containers: &[serde_json::Value]) -> Option<Progress> {
    if containers.is_empty() {
        return None;
    }
    let state = |container: &serde_json::Value| {
        container
            .get("state")
            .and_then(|state| state.as_str())
            .map(str::to_string)
    };
    Some(Progress {
        all_ready: containers
            .iter()
            .all(|container| state(container).as_deref() == Some("ready")),
        any_moving: containers.iter().any(|container| {
            matches!(state(container).as_deref(), Some("starting" | "restarting"))
        }),
    })
}

/// Waits for a bare `b` and opens `url` on it.
///
/// Cbreak, not raw: only `ICANON` and `ECHO` come off, so the keypress arrives
/// without Enter while the terminal keeps translating the newlines compose
/// writes for the rest of the tour, and keeps turning Ctrl+C into SIGINT. A
/// raw-mode read drops both.
#[cfg(unix)]
fn open_console_on_request(url: &str) {
    use nix::sys::termios::{LocalFlags, SetArg, SpecialCharacterIndices, tcgetattr, tcsetattr};
    use std::io::Read;

    let stdin = std::io::stdin();
    if let Ok(saved) = tcgetattr(&stdin) {
        let mut cbreak = saved.clone();
        cbreak.local_flags &= !(LocalFlags::ICANON | LocalFlags::ECHO);
        // Canonical mode ignores these two, so they carry whatever the shell
        // left behind: one byte, no timer, or the read returns immediately and
        // spins.
        cbreak.control_chars[SpecialCharacterIndices::VMIN as usize] = 1;
        cbreak.control_chars[SpecialCharacterIndices::VTIME as usize] = 0;
        if tcsetattr(&stdin, SetArg::TCSANOW, &cbreak).is_err() {
            return;
        }
        // Ctrl+C ends the tour with this thread still parked below, so the
        // restore cannot live only at the end of this function.
        let _ = SAVED_TERMIOS.set(saved.into());
    }

    let mut key = [0u8; 1];
    while let Ok(1) = stdin.lock().read(&mut key) {
        if !key[0].eq_ignore_ascii_case(&b'b') {
            continue;
        }
        if open::that(url).is_err() {
            eprintln!("  could not open a browser, open {url} yourself\r");
        }
        break;
    }
    restore_terminal_mode();
}

#[cfg(not(unix))]
fn open_console_on_request(url: &str) {
    // Windows reads console input events, so there is no output mode to
    // protect and no termios to put back.
    let term = console::Term::stdout();
    while let Ok(key) = term.read_char() {
        if key.eq_ignore_ascii_case(&'b') {
            if open::that(url).is_err() {
                eprintln!("  could not open a browser, open {url} yourself");
            }
            break;
        }
    }
}

/// The caller's terminal settings, saved when [`open_console_on_request`] puts
/// stdin in cbreak mode. Held as the raw struct because nix's `Termios` wraps
/// it in a `RefCell` and so is not `Sync`.
#[cfg(unix)]
static SAVED_TERMIOS: std::sync::OnceLock<libc::termios> = std::sync::OnceLock::new();

/// Puts the terminal back the way the shell handed it over. Safe to call when
/// nothing changed it, and safe to call twice.
fn restore_terminal_mode() {
    #[cfg(unix)]
    if let Some(saved) = SAVED_TERMIOS.get() {
        let _ = nix::sys::termios::tcsetattr(
            std::io::stdin(),
            nix::sys::termios::SetArg::TCSANOW,
            &nix::sys::termios::Termios::from(*saved),
        );
    }
}

/// `base` if it does not exist under `parent`, else the first free
/// `base-1`, `base-2`, ...
fn next_free_dir(parent: &Path, base: &str) -> PathBuf {
    let first = parent.join(base);
    if !first.exists() {
        return first;
    }
    (1u32..)
        .map(|i| parent.join(format!("{base}-{i}")))
        .find(|p| !p.exists())
        .expect("unbounded range always yields a free name")
}

/// The inference providers the harness template ships a key line for, in
/// `.env` order. `container` is the commented `worker-compose.yaml` block to
/// uncomment; the first two are enabled by the template already.
const PROVIDERS: &[(&str, &str, Option<&str>)] = &[
    ("Anthropic", "ANTHROPIC_API_KEY", None),
    ("OpenAI", "OPENAI_API_KEY", None),
    ("DeepSeek", "DEEPSEEK_API_KEY", Some("provider-deepseek")),
    ("Kimi (Moonshot)", "MOONSHOT_API_KEY", Some("provider-kimi")),
    ("xAI", "XAI_API_KEY", Some("provider-xai")),
    ("Z.ai", "ZAI_API_KEY", Some("provider-zai")),
    (
        "OpenRouter",
        "OPENROUTER_API_KEY",
        Some("provider-openrouter"),
    ),
    ("llama.cpp", "LLAMACPP_API_KEY", Some("provider-llamacpp")),
];

const PROVIDER_KEY_NOTE: &str = "Before we begin, if you want to use the iii harness you'll need to \
provide an API Key for an inference provider (ex. OpenAI, Anthropic). You can provide that now or \
manually edit the .env that is at the root of this project's directory.\n\nNote: If you provide the \
key after the project has started you'll need to manually restart the llm-router by running \
`iii trigger compose::restart worker=llm-router`.";

/// Ask for one provider API key and record it in the new project's `.env`.
/// A provider the template ships commented out also gets its
/// `worker-compose.yaml` block uncommented, so the router can reach it.
///
/// Every failure here is non-fatal: the tour still starts, and the note tells
/// the user how to add the key by hand.
fn prompt_provider_key(dir: &Path) {
    let env_path = dir.join(PROJECT_ENV_FILE);
    if !std::io::stdin().is_terminal() || !env_path.exists() {
        return;
    }

    eprintln!();
    if cliclack::log::info(PROVIDER_KEY_NOTE).is_err() {
        return;
    }

    let mut select = cliclack::select("Which inference provider?");
    for (label, var, _) in PROVIDERS {
        select = select.item(Some(*var), *label, *var);
    }
    select = select.item(None, "Skip for now", "edit .env yourself");

    let Ok(Some(var)) = select.interact() else {
        return;
    };
    let Ok(key) = cliclack::password(var).mask('•').interact() else {
        return;
    };
    // Terminals and password managers pad pasted keys; a stray space breaks auth.
    let key = key.trim();
    if key.is_empty() {
        let _ = cliclack::log::warning("No key entered, leaving .env unchanged.");
        return;
    }

    if let Err(e) = set_env_var(&env_path, var, key) {
        let _ = cliclack::log::warning(format!("could not write {}: {e}", env_path.display()));
        return;
    }

    let container = PROVIDERS
        .iter()
        .find(|(_, v, _)| *v == var)
        .and_then(|(_, _, c)| *c);
    if let Some(container) = container
        && let Err(e) = uncomment_container(&dir.join("worker-compose.yaml"), container)
    {
        let _ = cliclack::log::warning(format!("could not enable {container}: {e}"));
    }

    let _ = cliclack::log::success(format!("{var} written to {}", env_path.display()));
}

/// Ask for each variable named by `--need-envs` and record it in the new
/// project's `.env`, the same prompt and the same writer as the provider key
/// above. This is how a `--start-with` worker outside [`PROVIDERS`] gets the
/// key it needs before the project starts.
///
/// The answers come back so the caller can also put them in `iii compose
/// --up`'s environment: a worker that reads its key from the process
/// environment rather than the project's `.env` gets it on this first run
/// too, without waiting for a restart.
///
/// Non-fatal throughout: a skipped or failed variable leaves the tour running
/// with that key unset.
fn prompt_extra_env_keys(dir: &Path, vars: &[String]) -> Vec<(String, String)> {
    let mut collected = Vec::new();
    let env_path = dir.join(PROJECT_ENV_FILE);
    if vars.is_empty() || !std::io::stdin().is_terminal() || !env_path.exists() {
        return collected;
    }

    for var in vars {
        let var = var.trim();
        if var.is_empty() {
            continue;
        }
        let Ok(key) = cliclack::password(var).mask('•').interact() else {
            return collected;
        };
        // Terminals and password managers pad pasted keys; a stray space breaks auth.
        let key = key.trim();
        if key.is_empty() {
            let _ = cliclack::log::warning(format!("No key entered, leaving {var} unset."));
            continue;
        }
        if let Err(e) = set_env_var(&env_path, var, key) {
            let _ = cliclack::log::warning(format!("could not write {}: {e}", env_path.display()));
            return collected;
        }
        collected.push((var.to_string(), key.to_string()));
        let _ = cliclack::log::success(format!("{var} written to {}", env_path.display()));
    }
    collected
}

/// Set `var` in a `.env` file, replacing the existing line even when the
/// template ships it commented out. Appends when the file has no such line.
fn set_env_var(path: &Path, var: &str, value: &str) -> std::io::Result<()> {
    let text = std::fs::read_to_string(path)?;
    let assignment = format!("{var}=");
    let mut out = String::with_capacity(text.len() + value.len());
    let mut written = false;

    for line in text.lines() {
        let bare = line.trim_start().trim_start_matches('#').trim_start();
        if !written && bare.starts_with(&assignment) {
            out.push_str(&format!("{var}={value}\n"));
            written = true;
        } else {
            out.push_str(line);
            out.push('\n');
        }
    }
    if !written {
        out.push_str(&format!("{var}={value}\n"));
    }
    std::fs::write(path, out)
}

/// Uncomment the commented-out `worker-compose.yaml` container block whose
/// first line names `container`. The block ends at the first blank line.
fn uncomment_container(path: &Path, container: &str) -> std::io::Result<()> {
    let text = std::fs::read_to_string(path)?;
    let header = format!("{container}:");
    let mut out = String::with_capacity(text.len());
    let mut inside = false;

    for line in text.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with('#') && trimmed.contains(&header) {
            inside = true;
        } else if line.trim().is_empty() || !trimmed.starts_with('#') {
            inside = false;
        }
        out.push_str(&if inside {
            uncomment_line(line)
        } else {
            line.to_string()
        });
        out.push('\n');
    }
    std::fs::write(path, out)
}

/// `  #    worker: x` -> `    worker: x`: drop the first `#` and the two
/// spaces after it, which puts the YAML back on its original column.
fn uncomment_line(line: &str) -> String {
    match line.split_once('#') {
        Some((indent, rest)) => format!("{indent}{}", rest.strip_prefix("  ").unwrap_or(rest)),
        None => line.to_string(),
    }
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

    let mut fetcher = match build_fetcher(args.template_dir.as_deref()) {
        Ok(f) => f,
        Err(e) => {
            return print_err(
                "could not build template fetcher",
                &e.to_string(),
                "check III_TEMPLATE_URL or pass --template-dir <path>",
            );
        }
    };

    if let Err(e) = apply_docker(&mut fetcher, &root, &device_id).await {
        return print_err(
            "could not apply 'docker' template",
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

// ============================================================================
// Helpers
// ============================================================================

/// Fetch the docker template's two files directly (skipping the shared_files
/// merge that [`copy_template`] applies). We can't go through `copy_template`
/// here because it'd re-copy `config.yaml` / `.gitignore` from `shared_files`
/// and clobber any user customizations — the caller already has those from the
/// 'bare' template or a prior `iii project init`.
///
/// The Dockerfile template carries a literal `__III_DEVICE_ID__` placeholder
/// that we substitute with the actual device_id before writing, so the image
/// no longer needs an `III_HOST_USER_ID` env var at runtime. The generated
/// `.env` carries RabbitMQ credentials that the engine reads while expanding
/// `${VAR}` placeholders and that the commented-out RabbitMQ service can use.
const DEVICE_ID_PLACEHOLDER: &str = "__III_DEVICE_ID__";

async fn apply_docker(
    fetcher: &mut TemplateFetcher,
    target: &Path,
    device_id: &str,
) -> anyhow::Result<()> {
    let dockerfile_bytes = fetcher.fetch_file_bytes("docker", "Dockerfile").await?;
    let compose = fetcher
        .fetch_file_bytes("docker", "docker-compose.yml")
        .await?;

    let dockerfile = substitute_device_id(&dockerfile_bytes, device_id)?;

    write_if_absent(&target.join("Dockerfile"), &dockerfile)?;
    write_if_absent(&target.join("docker-compose.yml"), &compose)?;
    write_env_if_absent(target)?;
    Ok(())
}

fn substitute_device_id(bytes: &[u8], device_id: &str) -> anyhow::Result<Vec<u8>> {
    let text = std::str::from_utf8(bytes)
        .map_err(|e| anyhow::anyhow!("Dockerfile template is not valid UTF-8: {e}"))?;
    if !text.contains(DEVICE_ID_PLACEHOLDER) {
        anyhow::bail!(
            "Dockerfile template is missing the {DEVICE_ID_PLACEHOLDER} \
             placeholder — the template repo and engine are out of sync"
        );
    }
    Ok(text.replace(DEVICE_ID_PLACEHOLDER, device_id).into_bytes())
}

fn write_if_absent(path: &Path, contents: &[u8]) -> std::io::Result<()> {
    if path.exists() {
        return Ok(());
    }
    std::fs::write(path, contents)
}

fn write_env_if_absent(target: &Path) -> std::io::Result<()> {
    let path = target.join(".env");
    if path.exists() {
        return Ok(());
    }
    let rabbitmq_pass = uuid::Uuid::new_v4().simple().to_string();
    let contents = format!(
        "# Generated by `iii project generate-docker`. Do not commit.\n\
         RABBITMQ_USER=iii\n\
         RABBITMQ_PASS={rabbitmq_pass}\n",
    );
    std::fs::write(path, contents)
}

/// Persist `.iii/project.ini`, preserving any existing project_id when called
/// against an already-initialized project. Returns the (existing or freshly
/// generated) project_id so the caller can include it in the success event.
async fn persist_project_ini(
    root: &Path,
    project_name: &str,
    source: &str,
    device_id: &str,
) -> anyhow::Result<String> {
    let project_id =
        read_existing_project_id(root).unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
    scaffolder_core::telemetry::write_project_ini(
        root,
        &project_id,
        project_name,
        source,
        Some(device_id),
    )
    .await?;
    Ok(project_id)
}

fn read_existing_project_id(root: &Path) -> Option<String> {
    read_project_ini_field(root, "project_id")
}

/// Read a single key from `.iii/project.ini` (flat or `[project]`-prefixed
/// format), returning `None` when the file is absent, unreadable, or the key
/// is missing/empty. The format-tolerant parser is shared between
/// `read_existing_project_id` (used by re-init) and
/// `resolve_device_id_for_docker` (used by the docker generator).
fn read_project_ini_field(root: &Path, key: &str) -> Option<String> {
    let path = root.join(".iii").join("project.ini");
    let contents = std::fs::read_to_string(path).ok()?;
    let prefix = format!("{key}=");
    contents
        .lines()
        .find_map(|l| l.trim().strip_prefix(&prefix))
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
}

fn resolve_device_id_for_docker(root: &Path) -> String {
    let ini_exists = root.join(".iii").join("project.ini").exists();
    match read_project_ini_field(root, "device_id") {
        Some(id) => id,
        None => {
            if ini_exists {
                // Legacy project.ini that pre-dates the device_id field
                // (e.g. created by the old iii-tools or the interactive
                // TUI flow without a --directory). Don't claim the
                // project is uninitialized — it isn't.
                eprintln!(
                    "  {} no device_id in .iii/project.ini; generating a fresh one.",
                    "note:".dimmed()
                );
            } else {
                warn_missing_project_ini(root);
            }
            iii::workers::telemetry::environment::get_or_create_device_id()
        }
    }
}

fn warn_missing_project_ini(root: &Path) {
    eprintln!(
        "  {} project not initialized at {}",
        "warning:".yellow().bold(),
        root.display()
    );
    eprintln!(
        "  {} run `iii project init` here first to persist a project identity.",
        "fix:".dimmed()
    );
}

fn print_init_success(project_name: &str, root: &Path, target_specified: bool, docker: bool) {
    eprintln!();
    eprintln!(
        "  {} iii project '{}' initialized at {}",
        "✓".green(),
        project_name.bold(),
        root.display()
    );
    eprintln!();
    eprintln!("  Next steps:");
    if target_specified {
        eprintln!("    {}", format!("cd {}", root.display()).bold());
    }
    eprintln!(
        "    {}    # declare project workers",
        "edit worker-compose.yaml".bold()
    );
    eprintln!(
        "    {}    # start the engine and project workers",
        "iii compose --up".bold()
    );
    if docker {
        eprintln!(
            "    {}           # or start in Docker",
            "docker compose up".bold()
        );
    }
    eprintln!();
    eprintln!("  Docs: https://iii.dev/docs/quickstart");
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    #[derive(Parser)]
    struct Cli {
        #[command(subcommand)]
        action: ProjectAction,
    }

    #[test]
    fn learn_iii_parses_alone() {
        let cli = Cli::try_parse_from(["project", "init", "--learn-iii"]).unwrap();
        let ProjectAction::Init(init) = cli.action else {
            panic!("expected init");
        };
        assert!(init.learn_iii);
    }

    #[test]
    fn learn_iii_accepts_a_name() {
        let cli = Cli::try_parse_from(["project", "init", "--learn-iii", "my-tour"]).unwrap();
        let ProjectAction::Init(init) = cli.action else {
            panic!("expected init");
        };
        assert!(init.learn_iii);
        assert_eq!(init.name.as_deref(), Some("my-tour"));
    }

    #[test]
    fn start_with_splits_on_commas_and_needs_learn_iii() {
        let cli = Cli::try_parse_from([
            "project",
            "init",
            "--learn-iii",
            "--start-with",
            "worker1,worker2",
        ])
        .unwrap();
        let ProjectAction::Init(init) = cli.action else {
            panic!("expected init");
        };
        assert_eq!(init.start_with, ["worker1", "worker2"]);

        assert!(
            Cli::try_parse_from(["project", "init", "--start-with", "worker1"]).is_err(),
            "--start-with alone should require --learn-iii"
        );
    }

    #[test]
    fn need_envs_splits_on_commas() {
        let cli = Cli::try_parse_from([
            "project",
            "init",
            "--learn-iii",
            "--start-with",
            "worker1",
            "--need-envs",
            "WORKER_API_KEY,SECOND_KEY",
        ])
        .unwrap();
        let ProjectAction::Init(init) = cli.action else {
            panic!("expected init");
        };
        assert_eq!(init.need_envs, ["WORKER_API_KEY", "SECOND_KEY"]);
    }

    #[test]
    fn only_the_bare_name_is_the_tour_worker() {
        assert!(is_tour_worker("onboarding"));
        assert!(is_tour_worker(" onboarding "));
        assert!(!is_tour_worker("onboarding@1.2.0"));
        assert!(!is_tour_worker("acme/onboarding"));
    }

    #[test]
    fn a_restarting_container_is_still_moving() {
        let containers = vec![
            serde_json::json!({ "state": "ready" }),
            serde_json::json!({ "state": "restarting" }),
        ];
        assert_eq!(
            read_progress(&containers),
            Some(Progress {
                all_ready: false,
                any_moving: true
            })
        );
    }

    #[test]
    fn progress_needs_containers() {
        assert_eq!(read_progress(&[]), None);
    }

    #[test]
    fn a_project_that_has_not_started_is_not_ready_and_not_starting() {
        // Every container declared, none created yet. Taken alone this looks
        // exactly like a settled project, which is why the caller also waits
        // to have seen something start.
        let containers = [
            serde_json::json!({ "container": "state", "state": "stopped" }),
            serde_json::json!({ "container": "ade", "state": "stopped" }),
        ];
        assert_eq!(
            read_progress(&containers),
            Some(Progress {
                all_ready: false,
                any_moving: false
            })
        );
    }

    #[test]
    fn a_starting_container_is_reported_as_starting() {
        let containers = [
            serde_json::json!({ "container": "state", "state": "ready" }),
            serde_json::json!({ "container": "ade", "state": "starting" }),
        ];
        assert_eq!(
            read_progress(&containers),
            Some(Progress {
                all_ready: false,
                any_moving: true
            })
        );
    }

    #[test]
    fn a_failed_optional_container_still_counts_as_settled() {
        let containers = [
            serde_json::json!({ "container": "state", "state": "ready" }),
            serde_json::json!({ "container": "ade", "state": "stopped" }),
        ];
        assert_eq!(
            read_progress(&containers),
            Some(Progress {
                all_ready: false,
                any_moving: false
            })
        );
    }

    #[test]
    fn every_container_ready_is_all_ready() {
        let containers = [
            serde_json::json!({ "container": "state", "state": "ready" }),
            serde_json::json!({ "container": "ade", "state": "ready" }),
        ];
        assert_eq!(
            read_progress(&containers),
            Some(Progress {
                all_ready: true,
                any_moving: false
            })
        );
    }

    #[test]
    fn workers_are_declared_with_the_project_env_file() {
        let tmp = tempfile::tempdir().unwrap();
        let compose = tmp.path().join("worker-compose.yaml");
        std::fs::write(tmp.path().join(".env"), "WORKER_API_KEY=x\n").unwrap();

        let declared = worker_declarations(&compose, &["worker1".to_string()]);
        assert_eq!(
            declared,
            vec![serde_json::json!({ "worker": "worker1", "env_file": [".env"] })]
        );
    }

    #[test]
    fn workers_stay_bare_specs_without_an_env_file() {
        let tmp = tempfile::tempdir().unwrap();
        let compose = tmp.path().join("worker-compose.yaml");

        let declared = worker_declarations(&compose, &["worker1".to_string()]);
        assert_eq!(declared, vec![serde_json::json!("worker1")]);
    }

    #[test]
    fn worker_name_strips_the_scope_and_the_version() {
        assert_eq!(worker_name("onboarding"), "onboarding");
        assert_eq!(worker_name("onboarding@0.1.3"), "onboarding");
        assert_eq!(worker_name("iii-hq/onboarding@0.1.3"), "onboarding");
        assert_eq!(worker_name(" onboarding "), "onboarding");
        assert_eq!(worker_name(""), "");
    }

    #[test]
    fn learn_dir_base_names_the_project_after_the_first_worker() {
        assert_eq!(learn_dir_base(&[]), "learn-iii");
        assert_eq!(learn_dir_base(&["worker1".to_string()]), "iii-worker1");
        assert_eq!(
            learn_dir_base(&["worker1".to_string(), "queue".to_string()]),
            "iii-worker1"
        );
        // A reference carries a scope and a version; neither belongs in a path.
        assert_eq!(
            learn_dir_base(&["iii-hq/worker1@1.2.0".to_string()]),
            "iii-worker1"
        );
        assert_eq!(learn_dir_base(&[String::new()]), "learn-iii");
    }

    #[test]
    fn learn_iii_rejects_template_and_directory() {
        for extra in [
            &["-t", "quickstart"][..],
            &["-d", "x"],
            &["--docker"],
            &["--template-dir", "x"],
        ] {
            let mut argv = vec!["project", "init", "--learn-iii"];
            argv.extend_from_slice(extra);
            assert!(
                Cli::try_parse_from(argv).is_err(),
                "--learn-iii should conflict with {extra:?}"
            );
        }
    }

    #[test]
    fn set_env_var_replaces_active_and_commented_lines() {
        let tmp = tempfile::tempdir().unwrap();
        let env = tmp.path().join(".env");
        std::fs::write(
            &env,
            "# comment\nANTHROPIC_API_KEY=\nOPENAI_API_KEY=old\n# XAI_API_KEY=\n",
        )
        .unwrap();

        set_env_var(&env, "OPENAI_API_KEY", "sk-new").unwrap();
        set_env_var(&env, "XAI_API_KEY", "xai-new").unwrap();
        set_env_var(&env, "ZAI_API_KEY", "zai-new").unwrap();

        let text = std::fs::read_to_string(&env).unwrap();
        assert_eq!(
            text,
            "# comment\nANTHROPIC_API_KEY=\nOPENAI_API_KEY=sk-new\nXAI_API_KEY=xai-new\nZAI_API_KEY=zai-new\n"
        );
    }

    #[test]
    fn uncomment_container_touches_only_its_own_block() {
        let tmp = tempfile::tempdir().unwrap();
        let compose = tmp.path().join("worker-compose.yaml");
        std::fs::write(
            &compose,
            "containers:\n  queue:\n    worker: package://queue\n\n  #  provider-xai:                # XAI_API_KEY\n  #    worker: package://provider-xai\n  #    start_after:\n  #      - llm-router\n\n  #  provider-zai:\n  #    worker: package://provider-zai\n",
        )
        .unwrap();

        uncomment_container(&compose, "provider-xai").unwrap();

        let text = std::fs::read_to_string(&compose).unwrap();
        assert!(text.contains("\n  provider-xai:                # XAI_API_KEY\n"));
        assert!(text.contains("\n    worker: package://provider-xai\n"));
        assert!(text.contains("\n      - llm-router\n"));
        // The next block stays commented out.
        assert!(text.contains("\n  #  provider-zai:\n"));
    }

    #[test]
    fn every_provider_env_var_is_unique() {
        let mut vars: Vec<_> = PROVIDERS.iter().map(|(_, v, _)| *v).collect();
        vars.sort_unstable();
        let count = vars.len();
        vars.dedup();
        assert_eq!(vars.len(), count);
    }

    /// The seed has to survive the round trip the engine actually does: the
    /// configuration file adapter parses each `config/*.yaml` into a
    /// `ConfigurationEntry` and SKIPS any file it cannot parse, which would
    /// leave the tour project on the stock layout with only a log line.
    #[test]
    fn the_console_seed_parses_as_a_configuration_entry() {
        let tmp = tempfile::tempdir().unwrap();
        seed_console_layout(tmp.path());

        let raw = std::fs::read(tmp.path().join("config").join("console.yaml")).unwrap();
        let entry: iii::workers::configuration::structs::ConfigurationEntry =
            serde_yaml::from_slice(&raw).unwrap();

        assert_eq!(entry.id, CONSOLE_CONFIG);
        assert!(!entry.name.is_empty());
        let tabs = entry.value["workspace"]["tabs"].as_array().unwrap();
        assert_eq!(tabs.len(), 1);
        assert_eq!(tabs[0]["columns"], 2);
        assert_eq!(
            tabs[0]["screens"].as_array().unwrap(),
            &vec![
                serde_json::json!("chat"),
                serde_json::json!("ext:onboarding")
            ]
        );
        // Sizes only apply when they line up with the column count; a
        // mismatch silently drops the project back to equal widths.
        let sizes = tabs[0]["sizes"].as_array().unwrap();
        assert_eq!(sizes.len(), tabs[0]["columns"].as_u64().unwrap() as usize);
        assert_eq!(sizes[0].as_f64().unwrap(), 0.6);
        // No port: the console backfills the one it actually bound.
        assert!(entry.value.get("http_port").is_none());
    }

    /// A project that already carries a console entry keeps it — the seed is
    /// for a fresh scaffold, not a re-run over someone's saved layout.
    #[test]
    fn the_console_seed_never_overwrites_an_existing_entry() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("config").join("console.yaml");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, "id: console\nname: mine\ndescription: mine\n").unwrap();

        seed_console_layout(tmp.path());

        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "id: console\nname: mine\ndescription: mine\n"
        );
    }

    /// The pane the layout seed opens is served by the `onboarding` worker, so
    /// the container has to reach the project's compose file. It is a text
    /// insert, and the harness template's compose file is mostly comments, so
    /// the test pins both the placement and that the rest survives.
    #[test]
    fn the_tour_container_lands_under_containers() {
        let source = "namespace: demo\n\ncontainers:\n  # keep me\n  state:\n    worker: package://state\n    version: \"1.0.0\"\n";

        let patched = with_onboarding_container(source).expect("compose file takes the container");

        let containers = patched.find("containers:\n").unwrap();
        let onboarding = patched.find("  onboarding:\n").unwrap();
        let state = patched.find("  state:\n").unwrap();
        assert!(containers < onboarding && onboarding < state);
        assert!(patched.contains("worker: package://onboarding"));
        assert!(patched.contains("# keep me"), "comments must survive");
        assert!(patched.contains("namespace: demo"));

        // Compose rejects a `package://` container with no version.
        assert!(patched.contains("version: \"latest\""));
    }

    #[test]
    fn the_tour_container_is_written_once() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("worker-compose.yaml");
        std::fs::write(&path, "containers:\n  state:\n    worker: path://./state\n").unwrap();

        seed_onboarding_container(tmp.path());
        seed_onboarding_container(tmp.path());

        let text = std::fs::read_to_string(&path).unwrap();
        assert_eq!(text.matches("  onboarding:").count(), 1);
        assert!(with_onboarding_container(&text).is_none());
    }

    /// No compose file, or one with no `containers:` mapping: nothing to do,
    /// and the tour still starts.
    #[test]
    fn the_tour_container_needs_a_containers_mapping() {
        assert!(with_onboarding_container("namespace: demo\n").is_none());

        let tmp = tempfile::tempdir().unwrap();
        seed_onboarding_container(tmp.path());
        assert!(!tmp.path().join("worker-compose.yaml").exists());
    }

    #[test]
    fn next_free_dir_skips_taken_names() {
        let tmp = tempfile::tempdir().unwrap();
        assert_eq!(
            next_free_dir(tmp.path(), "learn-iii"),
            tmp.path().join("learn-iii")
        );
        std::fs::create_dir(tmp.path().join("learn-iii")).unwrap();
        assert_eq!(
            next_free_dir(tmp.path(), "learn-iii"),
            tmp.path().join("learn-iii-1")
        );
        std::fs::create_dir(tmp.path().join("learn-iii-1")).unwrap();
        assert_eq!(
            next_free_dir(tmp.path(), "learn-iii"),
            tmp.path().join("learn-iii-2")
        );
    }

    #[test]
    fn base_images_are_collected_from_every_worker_and_deduplicated() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();

        let write = |dir: &std::path::Path, image: Option<&str>| {
            std::fs::create_dir_all(dir).unwrap();
            let runtime = match image {
                Some(image) => format!("runtime:\n  base_image: {image}\n"),
                None => String::new(),
            };
            std::fs::write(
                dir.join("iii.worker.yaml"),
                format!("name: w\nlanguage: javascript\n{runtime}"),
            )
            .unwrap();
        };

        write(
            &root.join("workers/alpha"),
            Some("docker.io/iiidev/node:latest"),
        );
        write(
            &root.join("workers/beta"),
            Some("docker.io/iiidev/node:latest"),
        );
        write(
            &root.join("workers/gamma"),
            Some("docker.io/iiidev/python:latest"),
        );
        // A worker with no `base_image` runs on the engine, not in a VM.
        write(&root.join("workers/delta"), None);
        // Never descended into, however deep a manifest sits inside it.
        write(
            &root.join("workers/alpha/node_modules/pkg"),
            Some("docker.io/library/never:pulled"),
        );

        let mut images = declared_base_images(root);
        images.sort();
        assert_eq!(
            images,
            vec![
                "docker.io/iiidev/node:latest".to_string(),
                "docker.io/iiidev/python:latest".to_string(),
            ]
        );
    }

    #[test]
    fn an_empty_image_list_starts_no_prefetch() {
        assert!(start_image_prefetch(&[]).is_none());
    }
}
