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

/// `iii project init --learn-iii [NAME]`: same as `iii project init -t harness
/// <NAME>`, then `iii compose --up` from inside the new directory. Without
/// NAME the directory is the first free `learn-iii` name; a given NAME is
/// used as-is, so a taken one fails the same way plain init does.
async fn run_learn_iii(mut args: InitArgs) -> i32 {
    let dir = match args.name.as_deref() {
        Some(name) => PathBuf::from(name),
        None => next_free_dir(Path::new(""), LEARN_III_DIR),
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
    // runtime.
    seed_onboarding_container(&dir);

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

    let hint = format!("cd ./{} && iii compose --up", dir.display());
    eprintln!();
    eprintln!("  {} starting the tour: {}", "▶".green(), hint.bold());
    eprintln!();

    // Let the downloads finish before compose asks for the same images.
    // `ensure_rootfs` takes a per-image lock, so an overlap would be safe but
    // pointless: compose would sit on the lock with nothing on screen, while
    // waiting here keeps one pull visible in one place. By now the operator
    // has read a menu, so this is usually already done.
    for child in [prefetch.as_mut(), extra_prefetch.as_mut()]
        .into_iter()
        .flatten()
    {
        let _ = child.wait().await;
    }

    // Aborted when compose exits, so the poll inside needs no deadline of its
    // own: the tour's life is the deadline.
    let announcer = tokio::spawn(announce_console_when_ready(dir.join("worker-compose.yaml")));
    CHILD_OWNS_TERMINAL.store(true, Ordering::Relaxed);

    let code = match tokio::process::Command::new(exe)
        .args(["compose", "--up"])
        .current_dir(&dir)
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

/// The tour's own worker, declared in the project's compose file.
///
/// It serves the `ext:onboarding` console page, and a page injected by a
/// worker is only there while the worker runs.
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
// Not `"\` + newline: that also strips the next line's indent, which put the
// first comment line at column 0.
const ONBOARDING_CONTAINER: &str = "  # The guided tour. It serves the `onboarding` console page.
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

/// Best effort. A project whose compose file cannot take the container still
/// starts, without the tour page.
fn seed_onboarding_container(dir: &Path) {
    let path = dir.join("worker-compose.yaml");
    let Ok(text) = std::fs::read_to_string(&path) else {
        return;
    };
    if let Some(patched) = with_onboarding_container(&text) {
        let _ = std::fs::write(&path, patched);
    }
}

/// Set while `iii compose --up` runs as our child, so the Ctrl+C handler above
/// leaves the interrupt to compose.
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
async fn announce_console_when_ready(compose_path: PathBuf) {
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
/// over one optional container. The caller
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

    /// The tour page is served by the `onboarding` worker, so the container
    /// has to reach the project's compose file. It is a text
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
        assert!(
            patched.contains("containers:\n  # The guided tour"),
            "{patched}"
        );
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
