//! Test machine inheritance in a subprocess without mutating Cargo's environment.

use std::process::Command;

#[test]
fn workers_receive_the_machine_environment() {
    let mut command = Command::new(std::env::current_exe().unwrap());
    command
        .args([
            "--ignored",
            "--exact",
            "machine_environment_probe",
            "--nocapture",
        ])
        .env("COMPOSE_MACHINE_ONLY", "from-machine")
        .env("III_URL", "ws://stale:1")
        .env("III_CONFIG", "/stale/config")
        .env("III_CONFIG_NAME", "stale-config")
        .env("III_HOST_USER_ID", "other-project");
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStringExt;
        command.env(
            "COMPOSE_NON_UNICODE",
            std::ffi::OsString::from_vec(vec![0xff]),
        );
    }
    let output = command.output().unwrap();
    assert!(
        output.status.success(),
        "environment probe failed:\n{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
}

#[test]
#[ignore = "run by workers_receive_the_machine_environment with a controlled parent environment"]
fn machine_environment_probe() {
    use iii_compose::{
        manifest::StartSpec,
        spawn::{SpawnCtx, spawn_plan},
    };

    let tmp = tempfile::tempdir().unwrap();
    let compose_file = tmp.path().join("worker-compose.yaml");
    let user_env = std::collections::BTreeMap::new();
    #[cfg(unix)]
    let script = "printf '%s' \"$COMPOSE_MACHINE_ONLY\"";
    #[cfg(windows)]
    let script = "echo %COMPOSE_MACHINE_ONLY%";
    let start = StartSpec::Shell(script.to_string());
    let plan = spawn_plan(&SpawnCtx {
        engine_url: "ws://engine.test:49134",
        namespace: "machine-test",
        compose_namespace: "compose-test",
        compose_file: &compose_file,
        container_key: "api",
        start: &start,
        config_path: None,
        config_name: None,
        working_dir: tmp.path(),
        user_env: &user_env,
    });

    assert_eq!(plan.env["COMPOSE_MACHINE_ONLY"], "from-machine");
    assert_eq!(plan.env["III_URL"], "ws://engine.test:49134");
    assert!(!plan.env.contains_key("III_CONFIG"));
    assert!(!plan.env.contains_key("III_CONFIG_NAME"));
    assert!(!plan.env.contains_key("III_HOST_USER_ID"));
    #[cfg(unix)]
    assert!(!plan.env.contains_key("COMPOSE_NON_UNICODE"));

    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let output = runtime
        .block_on(async { plan.command().unwrap().output().await })
        .unwrap();
    assert!(output.status.success());
    assert_eq!(
        String::from_utf8(output.stdout).unwrap().trim(),
        "from-machine"
    );
}
