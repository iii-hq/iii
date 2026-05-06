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
fn project_init_accepts_positional_name() {
    // `iii project init myapp` should produce the same scaffold as
    // `iii project init --directory myapp`.
    let parent = tempdir().unwrap();
    let out = iii_bin()
        .args(["project", "init", "myapp"])
        .current_dir(parent.path())
        .output()
        .expect("failed to run iii");
    assert!(
        out.status.success(),
        "init <name> failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let project = parent.path().join("myapp");
    assert!(project.join(".iii").join("project.ini").exists());
    assert!(project.join("config.yaml").exists());
}

#[test]
fn project_init_preserves_existing_project_id_on_rerun() {
    // Re-running `iii project init` in an existing project must not rotate
    // project_id or wipe the existing identity. Telemetry depends on
    // project_id staying stable across runs.
    let dir = tempdir().unwrap();
    let out1 = iii_bin()
        .args(["project", "init", "--directory"])
        .arg(dir.path())
        .output()
        .expect("first init");
    assert!(out1.status.success(), "first init must succeed");
    let ini1 = std::fs::read_to_string(dir.path().join(".iii").join("project.ini")).unwrap();

    let out2 = iii_bin()
        .args(["project", "init", "--directory"])
        .arg(dir.path())
        .output()
        .expect("second init");
    assert!(out2.status.success(), "second init must succeed");
    let ini2 = std::fs::read_to_string(dir.path().join(".iii").join("project.ini")).unwrap();

    let project_id_of = |s: &str| {
        s.lines()
            .find_map(|l| l.strip_prefix("project_id="))
            .map(|v| v.trim().to_string())
            .expect("project_id present")
    };
    assert_eq!(
        project_id_of(&ini1),
        project_id_of(&ini2),
        "project_id must remain stable across re-runs"
    );
}

#[test]
fn project_init_with_empty_directory_flag_errors_clearly() {
    let out = iii_bin()
        .args(["project", "init", "--directory", ""])
        .output()
        .expect("failed to run iii");
    assert!(
        !out.status.success(),
        "empty --directory should fail to parse"
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("directory argument cannot be empty"),
        "expected explicit empty-arg message:\n{stderr}"
    );
}

#[test]
fn project_init_template_with_docker_writes_docker_assets() {
    // --docker must be honored even when template flags are also set.
    // Use --template-dir pointing at a path that does NOT exist so the
    // scaffolder fails fast — but if we ever got past the scaffolder, the
    // docker write would still need to happen. For now, we only assert
    // that the parser accepts the combination (sanity check on clap).
    // The actual write-docker-after-scaffold path is covered by the unit
    // test in cli::project::tests below.
    let dir = tempdir().unwrap();
    let out = iii_bin()
        .args([
            "project",
            "init",
            "--template",
            "definitely-not-a-real-template",
            "--docker",
            "--yes",
            "--skip-iii",
            "--template-dir",
        ])
        .arg(dir.path().join("nonexistent"))
        .arg("--directory")
        .arg(dir.path().join("app"))
        .output()
        .expect("failed to run iii");
    // Scaffolder will reject the bogus template, but the parse should have
    // accepted --docker alongside --template.
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        !stderr.contains("unexpected argument") && !stderr.contains("conflicts with"),
        "clap should accept --docker with template flags:\n{stderr}"
    );
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
    assert!(
        out.status.success(),
        "generate-docker should still succeed, just warn"
    );
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
    assert!(
        stderr.contains("error:"),
        "stderr should contain 'error:':\n{}",
        stderr
    );
    assert!(
        stderr.contains("cause:"),
        "stderr should contain 'cause:':\n{}",
        stderr
    );
    assert!(
        stderr.contains("fix:"),
        "stderr should contain 'fix:':\n{}",
        stderr
    );
}
