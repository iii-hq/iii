use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use tokio::fs;

use crate::runtime::check::Language;

const API_KEY: &str = "a7182ac460dde671c8f2e1318b517228";
const AMPLITUDE_ENDPOINT: &str = "https://api2.amplitude.com/2/httpapi";
const TELEMETRY_SCHEMA_VERSION: u8 = 2;

#[cfg(test)]
fn resolve_endpoint() -> String {
    std::env::var("__AMPLITUDE_ENDPOINT").unwrap_or_else(|_| AMPLITUDE_ENDPOINT.to_string())
}

#[cfg(not(test))]
fn resolve_endpoint() -> String {
    AMPLITUDE_ENDPOINT.to_string()
}

fn telemetry_yaml_path() -> std::path::PathBuf {
    dirs::home_dir()
        .unwrap_or_else(std::env::temp_dir)
        .join(".iii")
        .join("telemetry.yaml")
}

/// Only the fields we need to read from the engine's `~/.iii/telemetry.yaml`.
#[derive(Deserialize)]
struct TelemetryYaml {
    version: Option<u8>,
    #[serde(default)]
    identity: IdentitySection,
}

#[derive(Deserialize, Default)]
struct IdentitySection {
    #[serde(default)]
    device_id: Option<String>,
}

/// Reads the device_id from the engine-managed `~/.iii/telemetry.yaml`.
/// Returns `None` if the file is missing, malformed, or has no device_id.
fn read_device_id() -> Option<String> {
    let contents = std::fs::read_to_string(telemetry_yaml_path()).ok()?;
    let state: TelemetryYaml = serde_yaml::from_str(&contents).ok()?;
    if state.version != Some(TELEMETRY_SCHEMA_VERSION) {
        return None;
    }
    state.identity.device_id.filter(|id| !id.is_empty())
}

pub fn is_telemetry_disabled() -> bool {
    if let Ok(val) = std::env::var("III_TELEMETRY_ENABLED")
        && (val == "false" || val == "0")
    {
        return true;
    }
    if std::env::var("III_TELEMETRY_DEV").ok().as_deref() == Some("true") {
        return true;
    }
    const CI_VARS: &[&str] = &[
        "CI",
        "GITHUB_ACTIONS",
        "GITLAB_CI",
        "CIRCLECI",
        "JENKINS_URL",
        "TRAVIS",
        "BUILDKITE",
        "TF_BUILD",
        "CODEBUILD_BUILD_ID",
        "BITBUCKET_BUILD_NUMBER",
        "DRONE",
        "TEAMCITY_VERSION",
    ];
    CI_VARS.iter().any(|v| std::env::var(v).is_ok())
}

fn detect_is_container() -> bool {
    if std::env::var("III_CONTAINER").is_ok() {
        return true;
    }
    if std::env::var("KUBERNETES_SERVICE_HOST").is_ok() {
        return true;
    }
    Path::new("/.dockerenv").exists()
}

fn detect_install_method() -> &'static str {
    if let Ok(exe) = std::env::current_exe() {
        let path = exe.to_string_lossy();
        if path.contains(".cargo/bin") || path.contains("cargo-install") {
            return "cargo";
        }
        if path.contains("homebrew") || path.contains("Cellar") || path.contains("linuxbrew") {
            return "brew";
        }
        if path.contains("chocolatey") || path.contains("choco") {
            return "chocolatey";
        }
        if path.contains(".local/bin") {
            return "sh";
        }
    }
    "manual"
}

fn build_user_properties(tools_version: &str, device_id: &str) -> serde_json::Value {
    serde_json::json!({
        "environment.os": std::env::consts::OS,
        "environment.arch": std::env::consts::ARCH,
        "environment.cpu_cores": std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1),
        "environment.timezone": std::env::var("TZ").unwrap_or_else(|_| "Unknown".to_string()),
        "environment.machine_id": device_id,
        "environment.is_container": detect_is_container(),
        "env": std::env::var("III_ENV").unwrap_or_else(|_| "unknown".to_string()),
        "install_method": detect_install_method(),
        "cli_version": tools_version,
        "host_user_id": std::env::var("III_HOST_USER_ID").ok(),
    })
}

fn millis_epoch() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

#[derive(Serialize)]
struct AmplitudeEvent {
    device_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    user_id: Option<String>,
    event_type: String,
    event_properties: serde_json::Value,
    user_properties: Option<serde_json::Value>,
    platform: String,
    os_name: String,
    app_version: String,
    time: i64,
    insert_id: String,
    ip: Option<String>,
}

#[derive(Serialize)]
struct AmplitudePayload<'a> {
    api_key: &'a str,
    events: Vec<AmplitudeEvent>,
}

fn build_amplitude_client() -> Option<reqwest::Client> {
    reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(5))
        .build()
        .ok()
}

async fn post_amplitude(endpoint: &str, payload: &AmplitudePayload<'_>) {
    let Some(client) = build_amplitude_client() else {
        return;
    };
    let _ = client.post(endpoint).json(payload).send().await;
}

/// Sends a lightweight failure event when telemetry.yaml is missing.
/// Uses a throwaway device_id since we have no identity to attach to.
async fn send_telemetry_failed(endpoint: &str, platform: &str, tools_version: &str) {
    let event = AmplitudeEvent {
        device_id: format!("unknown-{}", uuid::Uuid::new_v4()),
        user_id: None,
        event_type: "iii_tools_telemetry_failed".to_string(),
        event_properties: serde_json::json!({
            "reason": "telemetry_yaml_missing",
            "path": telemetry_yaml_path().to_string_lossy(),
        }),
        user_properties: None,
        platform: platform.to_string(),
        os_name: std::env::consts::OS.to_string(),
        app_version: tools_version.to_string(),
        time: millis_epoch(),
        insert_id: uuid::Uuid::new_v4().to_string(),
        ip: Some("$remote".to_string()),
    };
    let payload = AmplitudePayload {
        api_key: API_KEY,
        events: vec![event],
    };
    post_amplitude(endpoint, &payload).await;
}

async fn send_amplitude_to(
    endpoint: &str,
    event_type: &str,
    platform: &str,
    tools_version: &str,
    event_properties: serde_json::Value,
) {
    let Some(device_id) = read_device_id() else {
        send_telemetry_failed(endpoint, platform, tools_version).await;
        return;
    };
    let event = AmplitudeEvent {
        device_id: device_id.clone(),
        user_id: None,
        event_type: event_type.to_string(),
        event_properties,
        user_properties: Some(build_user_properties(tools_version, &device_id)),
        platform: platform.to_string(),
        os_name: std::env::consts::OS.to_string(),
        app_version: tools_version.to_string(),
        time: millis_epoch(),
        insert_id: uuid::Uuid::new_v4().to_string(),
        ip: Some("$remote".to_string()),
    };
    let payload = AmplitudePayload {
        api_key: API_KEY,
        events: vec![event],
    };
    post_amplitude(endpoint, &payload).await;
}

async fn send_amplitude(
    event_type: &str,
    platform: &str,
    tools_version: &str,
    event_properties: serde_json::Value,
) {
    send_amplitude_to(
        &resolve_endpoint(),
        event_type,
        platform,
        tools_version,
        event_properties,
    )
    .await;
}

pub fn spawn_project_event(
    event_type: &'static str,
    platform: &'static str,
    tools_version: String,
    event_properties: serde_json::Value,
) -> Option<tokio::task::JoinHandle<()>> {
    if is_telemetry_disabled() {
        return None;
    }
    Some(tokio::spawn(async move {
        send_amplitude(event_type, platform, &tools_version, event_properties).await;
    }))
}

pub fn platform_for_product(product_name: &str) -> &'static str {
    match product_name {
        "motia" => "motia-tools",
        _ => "iii-tools",
    }
}

/// Persist a project's identity to `.iii/project.ini`.
///
/// `device_id` is optional: when provided, it's written as an additional
/// line so the engine telemetry pipeline can associate the project with the
/// host machine (e.g. for `III_HOST_USER_ID` Docker injection in
/// `iii project generate-docker`). Unset means "no device association",
/// matching the historical behavior used by the scaffolder TUI.
///
/// Values must not contain `\n` or `\r` — the function rejects them with
/// `InvalidInput` so a hostile project name can't smuggle additional INI
/// keys.
pub async fn write_project_ini(
    project_dir: &Path,
    project_id: &str,
    project_name: &str,
    template: &str,
    device_id: Option<&str>,
) -> Result<()> {
    for (k, v) in [
        ("project_id", project_id),
        ("project_name", project_name),
        ("source", template),
    ] {
        ensure_no_newline(k, v)?;
    }
    if let Some(d) = device_id {
        ensure_no_newline("device_id", d)?;
    }

    let dir = project_dir.join(".iii");
    fs::create_dir_all(&dir)
        .await
        .context("create .iii directory")?;
    let mut body = format!(
        "[project]\nproject_id={project_id}\nproject_name={project_name}\nsource={template}\n"
    );
    if let Some(d) = device_id {
        body.push_str(&format!("device_id={d}\n"));
    }
    fs::write(dir.join("project.ini"), body)
        .await
        .context("write project.ini")?;
    Ok(())
}

fn ensure_no_newline(key: &str, value: &str) -> Result<()> {
    if value.contains('\n') || value.contains('\r') {
        anyhow::bail!("{key} value must not contain newline characters");
    }
    Ok(())
}

pub async fn run_dependency_install(project_dir: &Path, langs: &[Language]) -> Result<()> {
    let mut failures: Vec<String> = Vec::new();

    // JS/TS: npm install at project root when package.json is present.
    let has_js_ts = langs
        .iter()
        .any(|l| matches!(l, Language::TypeScript | Language::JavaScript));
    if has_js_ts && project_dir.join("package.json").exists() {
        let res = tokio::process::Command::new("npm")
            .args(["install"])
            .current_dir(project_dir)
            .status()
            .await;
        match res {
            Ok(s) if s.success() => {}
            Ok(s) => failures.push(format!("npm install: exit {}", s)),
            Err(e) => failures.push(format!("npm install: {}", e)),
        }
    }

    // Python: prefer `uv sync`, fall back to pip/pip3 on requirements.txt.
    // Run independently of the JS branch above so mixed-language projects
    // (e.g. TS + Python in the quickstart) install both.
    let has_python = langs.contains(&Language::Python);
    let has_pyproject = project_dir.join("pyproject.toml").exists();
    let has_requirements = project_dir.join("requirements.txt").exists();
    if has_python && (has_pyproject || has_requirements) {
        let mut python_ok = false;
        let mut python_attempts: Vec<String> = Vec::new();

        if has_pyproject {
            let uv = tokio::process::Command::new("uv")
                .args(["sync"])
                .current_dir(project_dir)
                .status()
                .await;
            match uv {
                Ok(s) if s.success() => python_ok = true,
                Ok(s) => python_attempts.push(format!("uv sync: exit {}", s)),
                Err(e) => python_attempts.push(format!("uv sync: {}", e)),
            }
        }
        if !python_ok && has_requirements {
            for bin in ["pip", "pip3"] {
                let res = tokio::process::Command::new(bin)
                    .args(["install", "-r", "requirements.txt"])
                    .current_dir(project_dir)
                    .status()
                    .await;
                match res {
                    Ok(s) if s.success() => {
                        python_ok = true;
                        break;
                    }
                    Ok(s) => python_attempts.push(format!("{}: exit {}", bin, s)),
                    Err(e) => python_attempts.push(format!("{}: {}", bin, e)),
                }
            }
        }

        if !python_ok {
            failures.extend(python_attempts);
        }
    }

    if !failures.is_empty() {
        eprintln!(
            "Warning: dependency install incomplete - some installers did not succeed ({}). Install manually in {}.",
            failures.join(", "),
            project_dir.display()
        );
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, Request, ResponseTemplate};

    #[test]
    fn project_ini_body_format() {
        let s = format!(
            "[project]\nproject_id={}\nproject_name={}\nsource={}\n",
            "abc", "my-app", "quickstart"
        );
        assert!(s.contains("project_id=abc"));
        assert!(s.contains("project_name=my-app"));
        assert!(s.contains("source=quickstart"));
    }

    #[tokio::test]
    async fn write_project_ini_includes_source() {
        let tmp = tempfile::tempdir().unwrap();
        write_project_ini(tmp.path(), "proj-1", "my-proj", "quickstart", None)
            .await
            .unwrap();
        let contents =
            std::fs::read_to_string(tmp.path().join(".iii").join("project.ini")).unwrap();
        assert!(contents.contains("project_id=proj-1"));
        assert!(contents.contains("project_name=my-proj"));
        assert!(contents.contains("source=quickstart"));
        assert!(!contents.contains("device_id="));
    }

    #[tokio::test]
    async fn write_project_ini_with_custom_template() {
        let tmp = tempfile::tempdir().unwrap();
        write_project_ini(
            tmp.path(),
            "proj-2",
            "other",
            "multi-worker-orchestration",
            None,
        )
        .await
        .unwrap();
        let contents =
            std::fs::read_to_string(tmp.path().join(".iii").join("project.ini")).unwrap();
        assert!(contents.contains("source=multi-worker-orchestration"));
    }

    #[tokio::test]
    async fn write_project_ini_with_device_id() {
        let tmp = tempfile::tempdir().unwrap();
        write_project_ini(tmp.path(), "p", "n", "init", Some("device-abc"))
            .await
            .unwrap();
        let contents =
            std::fs::read_to_string(tmp.path().join(".iii").join("project.ini")).unwrap();
        assert!(contents.contains("device_id=device-abc"));
    }

    #[tokio::test]
    async fn write_project_ini_rejects_newline_in_value() {
        let tmp = tempfile::tempdir().unwrap();
        let err = write_project_ini(tmp.path(), "p", "evil\nproject_id=spoofed", "init", None)
            .await
            .unwrap_err();
        assert!(err.to_string().contains("must not contain newline"));
    }

    #[test]
    fn read_device_id_returns_none_when_no_file() {
        // Unless the engine has written telemetry.yaml, this may be None.
        // We just verify it doesn't panic.
        let _ = read_device_id();
    }

    #[tokio::test]
    #[serial_test::serial(home_env)]
    async fn sends_failed_event_when_yaml_missing() {
        let mock_server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path("/2/httpapi"))
            .respond_with(ResponseTemplate::new(200))
            .expect(1)
            .mount(&mock_server)
            .await;

        let endpoint = format!("{}/2/httpapi", mock_server.uri());

        // Point HOME at a temp dir so telemetry.yaml won't exist
        let tmp = tempfile::tempdir().unwrap();
        unsafe {
            std::env::set_var("HOME", tmp.path());
        }

        send_amplitude_to(
            &endpoint,
            "project_created",
            "iii-tools",
            "0.3.0",
            serde_json::json!({"project_id": "test-id"}),
        )
        .await;

        unsafe {
            std::env::remove_var("HOME");
        }

        let requests: Vec<Request> = mock_server.received_requests().await.unwrap();
        assert_eq!(requests.len(), 1);

        let body: serde_json::Value = serde_json::from_slice(&requests[0].body).unwrap();
        let event = &body["events"][0];
        assert_eq!(event["event_type"], "iii_tools_telemetry_failed");
        assert_eq!(
            event["event_properties"]["reason"],
            "telemetry_yaml_missing"
        );
    }

    #[tokio::test]
    #[serial_test::serial(home_env)]
    async fn sends_normal_event_when_yaml_exists() {
        let mock_server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path("/2/httpapi"))
            .respond_with(ResponseTemplate::new(200))
            .expect(1)
            .mount(&mock_server)
            .await;

        let tmp = tempfile::tempdir().unwrap();
        let iii_dir = tmp.path().join(".iii");
        std::fs::create_dir_all(&iii_dir).unwrap();
        std::fs::write(
            iii_dir.join("telemetry.yaml"),
            "version: 2\nidentity:\n  device_id: test-device-abc\n",
        )
        .unwrap();

        unsafe {
            std::env::set_var("HOME", tmp.path());
        }

        let endpoint = format!("{}/2/httpapi", mock_server.uri());

        send_amplitude_to(
            &endpoint,
            "project_created",
            "iii-tools",
            "0.3.0",
            serde_json::json!({
                "project_id": "test-id",
                "project_name": "my-project",
                "template": "quickstart",
                "product": "iii",
            }),
        )
        .await;

        unsafe {
            std::env::remove_var("HOME");
        }

        let requests: Vec<Request> = mock_server.received_requests().await.unwrap();
        assert_eq!(requests.len(), 1);

        let body: serde_json::Value = serde_json::from_slice(&requests[0].body).unwrap();
        let event = &body["events"][0];
        assert_eq!(event["event_type"], "project_created");
        assert_eq!(event["device_id"], "test-device-abc");
        assert!(
            event.get("user_id").is_none() || event["user_id"].is_null(),
            "user_id should not be sent"
        );
        assert_eq!(event["event_properties"]["project_id"], "test-id");
    }
}
