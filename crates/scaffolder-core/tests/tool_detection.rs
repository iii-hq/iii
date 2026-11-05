use scaffolder_core::runtime::tool::{ToolConfig, ToolManager};
use std::{env, fs, process::Command};

const TOOL_NAME: &str = "scaffolder-tool-detection-fixture";
const PROBE_ENV: &str = "SCAFFOLDER_TOOL_DETECTION_PROBE";
const FAILURE_ENV: &str = "SCAFFOLDER_TOOL_DETECTION_FAILURE";

#[test]
fn detects_tools_without_an_external_path_lookup_command() {
    // Run each probe in a child process so its isolated PATH cannot affect
    // other tests, and let the OS resolve the executable just as it resolves iii.
    if let Ok(scenario) = env::var(PROBE_ENV) {
        let tool = ToolManager::new(ToolConfig {
            name: TOOL_NAME,
            display_name: "test tool",
            install_script_url: "https://example.invalid/install.sh",
            docs_url: "https://example.invalid/docs",
        });
        assert_eq!(tool.is_installed(), scenario == "installed");
        if scenario == "installed" {
            assert_eq!(tool.get_version().as_deref(), Some("0.26.0-rc.5"));
        }
        return;
    }

    let fixture = tempfile::tempdir().expect("create fixture directory");
    let source = fixture.path().join("tool.rs");
    fs::write(
        &source,
        r#"
fn main() {
    assert_eq!(std::env::args().nth(1).as_deref(), Some("--version"));
    if std::env::var_os("SCAFFOLDER_TOOL_DETECTION_FAILURE").is_some() {
        std::process::exit(7);
    }
    println!("0.26.0-rc.5");
}
"#,
    )
    .expect("write fixture source");
    let executable = fixture
        .path()
        .join(format!("{TOOL_NAME}{}", env::consts::EXE_SUFFIX));
    let compiler = env::var_os("RUSTC").unwrap_or_else(|| "rustc".into());
    let output = Command::new(compiler)
        .arg(&source)
        .arg("-o")
        .arg(&executable)
        .output()
        .expect("compile native fixture executable");
    assert!(
        output.status.success(),
        "fixture compilation failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let empty_path = fixture.path().join("empty");
    fs::create_dir(&empty_path).expect("create empty PATH directory");
    for scenario in ["installed", "missing", "failing"] {
        let lookup_dir = if scenario == "missing" {
            &empty_path
        } else {
            fixture.path()
        };
        let mut probe = Command::new(env::current_exe().expect("find test executable"));
        probe
            .arg("--exact")
            .arg("detects_tools_without_an_external_path_lookup_command")
            .arg("--nocapture")
            .current_dir(lookup_dir)
            .env("PATH", lookup_dir)
            .env(PROBE_ENV, scenario)
            .env_remove(FAILURE_ENV);
        if scenario == "failing" {
            probe.env(FAILURE_ENV, "1");
        }
        let output = probe.output().expect("run isolated tool detection probe");
        assert!(
            output.status.success(),
            "{scenario} probe failed:\n{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }
}
