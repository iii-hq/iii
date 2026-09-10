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

    /// Take the 5-minute tour of iii: scaffold the "harness" template into
    /// NAME, or into ./learn-iii (learn-iii-1, learn-iii-2, ... when taken)
    /// if no NAME is given, then start `iii compose --up` inside it. Cannot
    /// be combined with any other scaffolding option.
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

    let code = run_init_with_template(args).await;
    if code != 0 {
        return code;
    }

    seed_console_layout(&dir);

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

    let hint = format!("cd ./{} && iii compose --up", dir.display());
    eprintln!();
    eprintln!("  {} starting the tour: {}", "▶".green(), hint.bold());
    eprintln!();

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
";

/// Open the new project's console on chat beside the tour.
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
/// The gate is every declared container reporting `ready` through
/// `compose::status`, which is also when compose's startup renderer lets go of
/// the terminal: it owns one global in-place block for the whole of `--up` and
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

    while !project_is_ready(&client, &file.path, &namespace).await {
        tokio::time::sleep(READY_POLL_INTERVAL).await;
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

    // ponytail: fixed settle delay. The renderer prints its closing line just
    // after the last container flips ready, and compose publishes no "startup
    // block finished" event to wait on instead. Swap this for that event if
    // compose ever grows one.
    tokio::time::sleep(BLOCK_SETTLE).await;

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

/// Whether every container the compose file declares reports `ready`.
///
/// A call that fails is the daemon not serving `compose::status` yet, which is
/// indistinguishable here from a project still starting: both mean "not yet".
async fn project_is_ready(
    client: &iii_compose::engine::EngineClient,
    compose_path: &Path,
    namespace: &str,
) -> bool {
    let request = iii_sdk::protocol::TriggerRequest {
        function_id: "compose::status".to_string(),
        payload: serde_json::json!({ "file": compose_path }),
        action: None,
        timeout_ms: Some(10_000),
    }
    .namespace(namespace);

    let Ok(value) = client.client().trigger(request).await else {
        return false;
    };
    let Some(containers) = value.get("containers").and_then(|list| list.as_array()) else {
        return false;
    };
    !containers.is_empty()
        && containers
            .iter()
            .all(|container| container.get("state").and_then(|s| s.as_str()) == Some("ready"))
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
    let env_path = dir.join(".env");
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
}
