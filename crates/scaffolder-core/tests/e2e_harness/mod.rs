//! E2E test harness for template validation.
//!
//! Scaffolds a template into a temp directory, starts the iii engine and workers,
//! waits for readiness, and provides HTTP helpers for assertions.
//!
//! Requires the `iii` binary on PATH (or via `III_BIN` env var).

#![allow(dead_code)]

use scaffolder_core::{copy_template, Language, RootManifest, TemplateFetcher, TemplateManifest};
use serde::Deserialize;
use serde_json::Value;
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::{Duration, Instant};
use tempfile::TempDir;
use tokio::process::{Child, Command};

const DEFAULT_HTTP_PORT: u16 = 3111;
const DEFAULT_ENGINE_PORT: u16 = 49134;

// ---------------------------------------------------------------------------
// Config types (lightweight, only the fields we need)
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct EngineConfigFile {
    #[serde(default)]
    workers: Vec<ConfigWorkerEntry>,
}

#[derive(Deserialize)]
struct ConfigWorkerEntry {
    name: String,
    #[serde(default)]
    config: Option<serde_json::Value>,
}

#[derive(Deserialize, Default)]
struct WorkerScripts {
    #[serde(default)]
    install: Option<String>,
    #[serde(default)]
    start: Option<String>,
}

#[derive(Deserialize)]
struct WorkerManifest {
    name: String,
    #[serde(default)]
    runtime: Option<WorkerRuntime>,
    #[serde(default)]
    scripts: Option<WorkerScripts>,
}

#[derive(Deserialize)]
struct WorkerRuntime {
    #[serde(default)]
    language: Option<String>,
    #[serde(default)]
    entry: Option<String>,
}

// ---------------------------------------------------------------------------
// Scenario
// ---------------------------------------------------------------------------

pub struct Scenario {
    pub project_dir: TempDir,
    children: Vec<(Child, String)>,
    pub http_port: u16,
    pub engine_port: u16,
    http_client: reqwest::Client,
    template_name: String,
    manifest: TemplateManifest,
}

impl Scenario {
    pub fn builder(template_name: &str, product: &str) -> ScenarioBuilder {
        ScenarioBuilder {
            template_name: template_name.to_string(),
            product: product.to_string(),
        }
    }

    // -- iii CLI --------------------------------------------------------

    /// Run an `iii` CLI command in the project directory. Panics on non-zero exit.
    pub async fn run_iii(&self, args: &[&str]) {
        let bin = iii_bin();
        let output = Command::new(&bin)
            .args(args)
            .current_dir(self.project_dir.path())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .await
            .unwrap_or_else(|e| panic!("failed to run {bin}: {e}"));

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            let stdout = String::from_utf8_lossy(&output.stdout);
            panic!(
                "iii {} exited with {}\nstdout: {stdout}\nstderr: {stderr}",
                args.join(" "),
                output.status
            );
        }
    }

    /// Run an `iii` CLI command and return its stdout. Panics on non-zero exit.
    pub async fn run_iii_with_output(&self, args: &[&str]) -> String {
        let bin = iii_bin();
        let output = Command::new(&bin)
            .args(args)
            .current_dir(self.project_dir.path())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .await
            .unwrap_or_else(|e| panic!("failed to run {bin}: {e}"));

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            let stdout = String::from_utf8_lossy(&output.stdout);
            panic!(
                "iii {} exited with {}\nstdout: {stdout}\nstderr: {stderr}",
                args.join(" "),
                output.status
            );
        }

        String::from_utf8_lossy(&output.stdout).into_owned()
    }

    // -- Engine ---------------------------------------------------------

    /// Start the iii engine as a background process.
    /// Waits until the engine port accepts TCP connections.
    pub async fn start_engine(&mut self) {
        let bin = iii_bin();
        eprintln!(
            "[e2e] starting engine: {bin} (cwd: {}, ports: http={}, ws={})",
            self.project_dir.path().display(),
            self.http_port,
            self.engine_port,
        );

        let mut child = Command::new(&bin)
            .current_dir(self.project_dir.path())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap_or_else(|e| panic!("failed to spawn engine ({bin}): {e}"));

        tokio::time::sleep(Duration::from_secs(1)).await;
        if let Some(status) = child.try_wait().unwrap() {
            let stderr = read_child_stderr(&mut child).await;
            let stdout = read_child_stdout(&mut child).await;
            panic!("engine exited immediately with {status}\nstdout: {stdout}\nstderr: {stderr}");
        }

        self.children.push((child, "iii-engine".to_string()));
        self.wait_for_port(self.engine_port, Duration::from_secs(30))
            .await;
    }

    // -- Workers --------------------------------------------------------

    /// Discover worker directories from the template manifest and start each one.
    pub async fn start_workers(&mut self) {
        let worker_dirs = self.discover_worker_dirs();

        for rel_dir in &worker_dirs {
            let abs_dir = self.project_dir.path().join(rel_dir);
            let wm = Self::read_worker_manifest(&abs_dir);
            let Some(wm) = wm else { continue };

            self.install_worker(&wm, &abs_dir).await;
            self.spawn_worker(&wm, &abs_dir).await;
        }
    }

    /// Start a single named worker by its directory under `workers/`.
    pub async fn start_worker(&mut self, worker_rel_dir: &str) {
        let abs_dir = self.project_dir.path().join(worker_rel_dir);
        let wm = Self::read_worker_manifest(&abs_dir)
            .unwrap_or_else(|| panic!("no iii.worker.yaml in {}", abs_dir.display()));

        self.install_worker(&wm, &abs_dir).await;
        self.spawn_worker(&wm, &abs_dir).await;
    }

    fn read_worker_manifest(dir: &Path) -> Option<WorkerManifest> {
        let manifest_path = dir.join("iii.worker.yaml");
        if !manifest_path.exists() {
            return None;
        }
        let content = std::fs::read_to_string(&manifest_path)
            .unwrap_or_else(|e| panic!("read {}: {e}", manifest_path.display()));
        Some(
            serde_yaml::from_str(&content)
                .unwrap_or_else(|e| panic!("parse {}: {e}", manifest_path.display())),
        )
    }

    async fn install_worker(&self, wm: &WorkerManifest, dir: &Path) {
        // Prefer explicit scripts.install from the worker manifest
        let custom_install = wm
            .scripts
            .as_ref()
            .and_then(|s| s.install.as_deref())
            .filter(|s| !s.is_empty());

        if let Some(install_cmd) = custom_install {
            let shell = if cfg!(windows) { "cmd" } else { "sh" };
            let flag = if cfg!(windows) { "/C" } else { "-c" };
            run_cmd(shell, &[flag, install_cmd], dir).await;
            return;
        }

        // Fall back to language-specific install
        let lang = wm
            .runtime
            .as_ref()
            .and_then(|r| r.language.as_deref())
            .unwrap_or("typescript");

        match lang {
            "typescript" | "javascript" => {
                if let Some(sdk_path) = local_node_sdk() {
                    patch_node_sdk(dir, &sdk_path);
                }
                run_cmd("npm", &["install", "--no-audit", "--no-fund"], dir).await;
            }
            "python" => {
                run_cmd("python3", &["-m", "venv", ".venv"], dir).await;
                let pip = if cfg!(windows) {
                    ".venv\\Scripts\\pip"
                } else {
                    ".venv/bin/pip"
                };
                if let Some(sdk_path) = local_python_sdk() {
                    install_python_deps_with_local_sdk(pip, dir, &sdk_path).await;
                } else {
                    run_cmd(pip, &["install", "-q", "-r", "requirements.txt"], dir).await;
                }
            }
            "rust" => {
                if let Some(sdk_path) = local_rust_sdk() {
                    patch_rust_sdk(dir, &sdk_path);
                }
            }
            other => panic!("unsupported language for install: {other}"),
        }
    }

    async fn spawn_worker(&mut self, wm: &WorkerManifest, dir: &Path) {
        let engine_url = format!("ws://localhost:{}", self.engine_port);
        let lang = wm
            .runtime
            .as_ref()
            .and_then(|r| r.language.as_deref())
            .unwrap_or("typescript");

        let start_script = wm
            .scripts
            .as_ref()
            .and_then(|s| s.start.as_deref())
            .unwrap_or_else(|| panic!("worker '{}' has no scripts.start", wm.name));

        let parts: Vec<&str> = start_script.split_whitespace().collect();
        let (program, args) = parts
            .split_first()
            .unwrap_or_else(|| panic!("empty scripts.start for '{}'", wm.name));

        let mut cmd = if lang == "python" && *program == "python" {
            let venv_python = if cfg!(windows) {
                ".venv\\Scripts\\python"
            } else {
                ".venv/bin/python"
            };
            let mut c = Command::new(venv_python);
            c.args(args);
            c
        } else {
            let mut c = Command::new(program);
            c.args(args);
            c
        };

        cmd.current_dir(dir)
            .env("III_URL", &engine_url)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        let child = cmd
            .spawn()
            .unwrap_or_else(|e| panic!("failed to spawn worker in {}: {e}", dir.display()));

        eprintln!(
            "[e2e] spawned worker '{}' via `{start_script}` (pid={})",
            wm.name,
            child.id().unwrap_or(0)
        );
        self.children.push((child, wm.name.clone()));
    }

    // -- HTTP -----------------------------------------------------------

    /// Wait until the HTTP port serves a 200 response on `/health`.
    pub async fn wait_for_http(&self, timeout: Duration) {
        let url = format!("http://localhost:{}/health", self.http_port);
        let start = Instant::now();
        let mut interval = Duration::from_millis(250);

        loop {
            if start.elapsed() > timeout {
                panic!("timed out after {timeout:?} waiting for HTTP at {url}");
            }

            match self.http_client.get(&url).send().await {
                Ok(resp) if resp.status().is_success() => return,
                _ => {}
            }

            tokio::time::sleep(interval).await;
            interval = (interval * 2).min(Duration::from_secs(2));
        }
    }

    pub async fn http_get(&self, path: &str) -> reqwest::Response {
        let url = format!("http://localhost:{}{path}", self.http_port);
        self.http_client
            .get(&url)
            .send()
            .await
            .unwrap_or_else(|e| panic!("GET {url} failed: {e}"))
    }

    pub async fn http_post(&self, path: &str, body: serde_json::Value) -> reqwest::Response {
        let url = format!("http://localhost:{}{path}", self.http_port);
        self.http_client
            .post(&url)
            .json(&body)
            .send()
            .await
            .unwrap_or_else(|e| panic!("POST {url} failed: {e}"))
    }

    // -- Helpers --------------------------------------------------------

    /// Read the HTTP port from config.yaml.
    pub fn read_http_port(&mut self) {
        let config_path = self.project_dir.path().join("config.yaml");
        if !config_path.exists() {
            return;
        }
        let content = std::fs::read_to_string(&config_path).unwrap();
        let cfg: EngineConfigFile = serde_yaml::from_str(&content).unwrap_or(EngineConfigFile {
            workers: Vec::new(),
        });

        for w in &cfg.workers {
            if w.name == "iii-http" {
                if let Some(config) = &w.config {
                    if let Some(port) = config.get("port").and_then(|v| v.as_u64()) {
                        self.http_port = port as u16;
                    }
                }
            }
        }
    }

    async fn wait_for_port(&self, port: u16, timeout: Duration) {
        let start = Instant::now();
        let mut interval = Duration::from_millis(100);

        loop {
            if start.elapsed() > timeout {
                panic!("timed out after {timeout:?} waiting for port {port}");
            }

            if TcpStream::connect(format!("127.0.0.1:{port}")).is_ok() {
                return;
            }

            tokio::time::sleep(interval).await;
            interval = (interval * 2).min(Duration::from_secs(1));
        }
    }

    fn discover_worker_dirs(&self) -> Vec<String> {
        self.manifest
            .files
            .iter()
            .filter(|f| f.ends_with("iii.worker.yaml"))
            .filter_map(|f| f.rsplit_once('/').map(|(dir, _)| dir.to_string()))
            .collect()
    }

    /// Check if each child is still running and print any available stderr.
    pub async fn dump_worker_status(&mut self) {
        for (child, label) in &mut self.children {
            let pid = child.id().unwrap_or(0);
            let exited = child.try_wait().unwrap();
            if let Some(status) = exited {
                let stderr = read_child_stderr(child).await;
                let stdout = read_child_stdout(child).await;
                eprintln!("[e2e] '{label}' (pid={pid}) has EXITED with {status}");
                if !stdout.is_empty() {
                    eprintln!("[e2e]   stdout: {stdout}");
                }
                if !stderr.is_empty() {
                    eprintln!("[e2e]   stderr: {stderr}");
                }
            } else {
                eprintln!("[e2e] '{label}' (pid={pid}) is still running");
            }
        }
    }

    /// Gracefully shut down all child processes: SIGTERM, wait up to 5s, then SIGKILL.
    pub async fn shutdown(&mut self) {
        for (child, label) in &mut self.children {
            let pid = child.id().unwrap_or(0);
            eprintln!("[e2e] shutting down '{label}' (pid={pid})");

            // Send SIGTERM via kill(2)
            if pid > 0 {
                unsafe {
                    libc::kill(pid as i32, libc::SIGTERM);
                }
            }

            match tokio::time::timeout(Duration::from_secs(5), child.wait()).await {
                Ok(Ok(status)) => {
                    eprintln!("[e2e] '{label}' exited with {status}");
                }
                _ => {
                    eprintln!("[e2e] '{label}' did not exit after SIGTERM, sending SIGKILL");
                    let _ = child.kill().await;
                    let _ = child.wait().await;
                    eprintln!("[e2e] '{label}' killed");
                }
            }
        }
        self.children.clear();
    }
}

impl Drop for Scenario {
    fn drop(&mut self) {
        // Synchronous best-effort: SIGKILL anything still alive
        for (child, label) in &mut self.children {
            let pid = child.id().unwrap_or(0);
            eprintln!("[e2e] drop: force-killing '{label}' (pid={pid})");
            let _ = child.start_kill();
            if pid > 0 {
                unsafe {
                    libc::kill(pid as i32, libc::SIGKILL);
                }
            }
        }
        self.children.clear();
    }
}

// ---------------------------------------------------------------------------
// Port cleanup — kill anything occupying our default ports before tests start
// ---------------------------------------------------------------------------

// TODO: Replace kill_port_holder / ensure_ports_free with ephemeral port allocation
// (bind TcpListener to "127.0.0.1:0") to avoid SIGKILL-ing unrelated local processes.

/// Kill any process listening on the given port. Best-effort, no panic.
fn kill_port_holder(port: u16) {
    let output = std::process::Command::new("lsof")
        .args(["-ti", &format!(":{port}")])
        .output();

    if let Ok(out) = output {
        let pids = String::from_utf8_lossy(&out.stdout);
        for pid_str in pids.split_whitespace() {
            if let Ok(pid) = pid_str.parse::<i32>() {
                eprintln!("[e2e] killing pid {pid} on port {port}");
                unsafe {
                    libc::kill(pid, libc::SIGKILL);
                }
            }
        }
    }
}

/// Ensure default ports are free before starting a scenario.
fn ensure_ports_free() {
    for port in [DEFAULT_HTTP_PORT, DEFAULT_ENGINE_PORT] {
        if TcpStream::connect(format!("127.0.0.1:{port}")).is_ok() {
            eprintln!("[e2e] port {port} is in use, killing holder");
            kill_port_holder(port);
            std::thread::sleep(Duration::from_millis(500));

            if TcpStream::connect(format!("127.0.0.1:{port}")).is_ok() {
                panic!("port {port} still in use after killing — aborting to prevent zombies");
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Builder
// ---------------------------------------------------------------------------

pub struct ScenarioBuilder {
    template_name: String,
    product: String,
}

impl ScenarioBuilder {
    /// Scaffold the template into a temp directory and return a ready Scenario.
    pub async fn build(self) -> Scenario {
        ensure_ports_free();

        let workspace = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .canonicalize()
            .expect("resolve workspace root");

        let template_dir = workspace.join(format!("templates/{}", self.product));
        assert!(
            template_dir.join("template.yaml").exists(),
            "template dir not found: {}",
            template_dir.display()
        );

        let root_content = std::fs::read_to_string(template_dir.join("template.yaml")).unwrap();
        let root_manifest: RootManifest = serde_yaml::from_str(&root_content).unwrap();

        let mut fetcher = TemplateFetcher::from_local(template_dir.clone(), "e2e-test");
        let manifest = fetcher
            .fetch_template_manifest(&self.template_name)
            .await
            .unwrap_or_else(|e| panic!("fetch template '{}': {e}", self.template_name));

        if let Some(min_ver) = &manifest.min_iii_version {
            if let Err(msg) = scaffolder_core::templates::version::check_iii_engine_version(min_ver)
            {
                panic!(
                    "Template '{}' requires iii >= {}, but: {}",
                    self.template_name, min_ver, msg
                );
            }
        }

        let mut lang_files = root_manifest.language_files.clone();
        lang_files.merge(&manifest.language_files);

        let all_languages = vec![
            Language::TypeScript,
            Language::JavaScript,
            Language::Python,
            Language::Rust,
        ];

        let project_dir = TempDir::new().expect("create temp dir");

        copy_template(
            &mut fetcher,
            &self.template_name,
            &manifest,
            project_dir.path(),
            &all_languages,
            &lang_files,
        )
        .await
        .unwrap_or_else(|e| panic!("copy_template failed: {e}"));

        Scenario {
            project_dir,
            children: Vec::new(),
            http_port: DEFAULT_HTTP_PORT,
            engine_port: DEFAULT_ENGINE_PORT,
            http_client: reqwest::Client::new(),
            template_name: self.template_name,
            manifest,
        }
    }
}

// ---------------------------------------------------------------------------
// Utilities
// ---------------------------------------------------------------------------

async fn read_child_stderr(child: &mut Child) -> String {
    use tokio::io::AsyncReadExt;
    let Some(stderr) = child.stderr.as_mut() else {
        return String::new();
    };
    let mut buf = String::new();
    let _ = tokio::time::timeout(Duration::from_millis(500), stderr.read_to_string(&mut buf)).await;
    buf
}

async fn read_child_stdout(child: &mut Child) -> String {
    use tokio::io::AsyncReadExt;
    let Some(stdout) = child.stdout.as_mut() else {
        return String::new();
    };
    let mut buf = String::new();
    let _ = tokio::time::timeout(Duration::from_millis(500), stdout.read_to_string(&mut buf)).await;
    buf
}

fn iii_bin() -> String {
    std::env::var("III_BIN").unwrap_or_else(|_| "iii".to_string())
}

/// Resolve iii-mono root: III_MONO_ROOT env, or ../iii-mono relative to workspace.
// cli-tooling has been merged into the monorepo; workspace root is the mono root.
fn iii_mono_root() -> Option<PathBuf> {
    if let Ok(p) = std::env::var("III_MONO_ROOT") {
        let path = PathBuf::from(p);
        if path.is_dir() {
            return Some(path);
        }
    }
    // Workspace root is the monorepo root
    let workspace = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .ok()?;
    let mono = workspace.parent()?.join("iii-mono");
    if mono.is_dir() {
        Some(mono)
    } else {
        None
    }
}

fn local_node_sdk() -> Option<PathBuf> {
    let p = iii_mono_root()?.join("sdk/packages/node/iii");
    if p.join("package.json").exists() {
        Some(p)
    } else {
        None
    }
}

fn local_python_sdk() -> Option<PathBuf> {
    let p = iii_mono_root()?.join("sdk/packages/python/iii");
    if p.join("pyproject.toml").exists() {
        Some(p)
    } else {
        None
    }
}

fn local_rust_sdk() -> Option<PathBuf> {
    let p = iii_mono_root()?.join("sdk/packages/rust/iii");
    if p.join("Cargo.toml").exists() {
        Some(p)
    } else {
        None
    }
}

/// Rewrite package.json to point iii-sdk at a local file: path.
fn patch_node_sdk(worker_dir: &Path, sdk_path: &Path) {
    let pkg_path = worker_dir.join("package.json");
    if !pkg_path.exists() {
        return;
    }
    let content = std::fs::read_to_string(&pkg_path).unwrap();
    let mut pkg: Value = serde_json::from_str(&content).unwrap();

    let sdk_file_ref = format!("file:{}", sdk_path.display());

    if let Some(deps) = pkg.get_mut("dependencies").and_then(|d| d.as_object_mut()) {
        if deps.contains_key("iii-sdk") {
            deps.insert("iii-sdk".to_string(), Value::String(sdk_file_ref.clone()));
        }
    }
    if let Some(deps) = pkg
        .get_mut("devDependencies")
        .and_then(|d| d.as_object_mut())
    {
        if deps.contains_key("iii-sdk") {
            deps.insert("iii-sdk".to_string(), Value::String(sdk_file_ref));
        }
    }

    let patched = serde_json::to_string_pretty(&pkg).unwrap();
    std::fs::write(&pkg_path, patched).unwrap();
    eprintln!("[e2e] patched {} -> local SDK", pkg_path.display());
}

/// Append a [patch.crates-io] section pointing iii-sdk at a local path.
fn patch_rust_sdk(worker_dir: &Path, sdk_path: &Path) {
    let cargo_path = worker_dir.join("Cargo.toml");
    if !cargo_path.exists() {
        return;
    }
    let mut content = std::fs::read_to_string(&cargo_path).unwrap();
    content.push_str(&format!(
        "\n[patch.crates-io]\niii-sdk = {{ path = \"{}\" }}\n",
        sdk_path.display()
    ));
    std::fs::write(&cargo_path, content).unwrap();
    eprintln!("[e2e] patched {} -> local SDK", cargo_path.display());
}

/// Install Python deps from requirements.txt, substituting iii-sdk with a local path.
/// Dev versions (e.g. 0.11.0.dev5) don't satisfy `>=0.11.0` per PEP 440, so we
/// install everything else from requirements.txt then install the local SDK separately.
async fn install_python_deps_with_local_sdk(pip: &str, dir: &Path, sdk_path: &Path) {
    let req_path = dir.join("requirements.txt");
    let content = std::fs::read_to_string(&req_path)
        .unwrap_or_else(|e| panic!("read {}: {e}", req_path.display()));

    let filtered: Vec<&str> = content
        .lines()
        .filter(|line| {
            let trimmed = line.trim();
            !trimmed.is_empty() && !trimmed.starts_with('#') && !trimmed.starts_with("iii-sdk")
        })
        .collect();

    if !filtered.is_empty() {
        let filtered_path = dir.join("requirements-filtered.txt");
        std::fs::write(&filtered_path, filtered.join("\n")).unwrap();
        run_cmd(
            pip,
            &["install", "-q", "-r", "requirements-filtered.txt"],
            dir,
        )
        .await;
    }

    run_cmd(
        pip,
        &["install", "-q", "--no-cache-dir", "--force-reinstall", sdk_path.to_str().unwrap()],
        dir,
    )
    .await;
    eprintln!(
        "[e2e] installed local Python SDK from {}",
        sdk_path.display()
    );
}

async fn run_cmd(program: &str, args: &[&str], dir: &Path) {
    let output = Command::new(program)
        .args(args)
        .current_dir(dir)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .await
        .unwrap_or_else(|e| panic!("failed to run {program} {}: {e}", args.join(" ")));

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        panic!(
            "{program} {} in {} exited with {}\nstdout: {stdout}\nstderr: {stderr}",
            args.join(" "),
            dir.display(),
            output.status
        );
    }
}
