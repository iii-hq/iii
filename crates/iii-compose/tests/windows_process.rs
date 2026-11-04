//! Native Windows regressions for shell quoting and detached supervision.

#![cfg(windows)]

use std::{collections::BTreeMap, os::windows::process::CommandExt, path::Path, time::Duration};

use iii_compose::{
    manifest::StartSpec,
    process::spawn_supervised_piped_with_stdin,
    spawn::{SpawnCtx, spawn_plan},
};
use tokio::io::AsyncReadExt;
use windows_sys::Win32::System::{
    Console::{GetConsoleProcessList, GetConsoleWindow},
    Threading::{CREATE_NO_WINDOW, DETACHED_PROCESS},
};

const FIXTURE_ROLE: &str = "III_COMPOSE_WINDOWS_TEST_ROLE";

fn console_process_count() -> u32 {
    let mut pid = 0;
    // A one-element buffer is enough to distinguish no console from an
    // attached console, including a console with no visible window.
    unsafe { GetConsoleProcessList(&mut pid, 1) }
}

fn shell_command(script: &str, cwd: &Path) -> tokio::process::Command {
    let start = StartSpec::Shell(script.to_string());
    let env = BTreeMap::new();
    let ctx = SpawnCtx {
        engine_url: "ws://127.0.0.1:49134",
        namespace: "windows-test",
        compose_namespace: "windows-test",
        compose_file: cwd,
        container_key: "fixture",
        start: &start,
        config_name: None,
        working_dir: cwd,
        user_env: &env,
    };
    let mut command = spawn_plan(&ctx).command().unwrap();
    command.creation_flags(CREATE_NO_WINDOW);
    command
}

#[tokio::test]
async fn shell_runs_a_quoted_executable_path_with_quoted_arguments() {
    let dir = tempfile::tempdir().unwrap();
    let bin_dir = dir.path().join("runtime with spaces");
    std::fs::create_dir(&bin_dir).unwrap();
    let exe = bin_dir.join("fixture.exe");
    std::fs::copy(std::env::current_exe().unwrap(), &exe).unwrap();
    // cmd /C requires these outer quotes when both the executable and its
    // arguments are quoted, as in a Windows path:// scripts.run declaration.
    let script = format!(
        "\"\"{}\" \"--exact\" \"windows_fixture\" \"--nocapture\"\"",
        exe.display()
    );
    let output = shell_command(&script, dir.path())
        .env(FIXTURE_ROLE, "arguments")
        .output()
        .await
        .unwrap();

    assert!(output.status.success(), "{output:?}");
    assert!(String::from_utf8_lossy(&output.stdout).contains("fixture-ok"));
}

#[tokio::test]
async fn shell_preserves_quotes_and_command_operators() {
    let dir = tempfile::tempdir().unwrap();
    let output = shell_command("echo \"two words\"&&echo second", dir.path())
        .output()
        .await
        .unwrap();

    assert!(output.status.success(), "{output:?}");
    assert_eq!(
        String::from_utf8(output.stdout)
            .unwrap()
            .lines()
            .collect::<Vec<_>>(),
        ["\"two words\"", "second"]
    );
}

fn run_console_fixture(flags: u32, role: &str) {
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "windows_fixture", "--nocapture"])
        .env(FIXTURE_ROLE, role)
        .creation_flags(flags)
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
}

#[test]
fn supervised_children_do_not_open_windows_when_the_parent_is_detached() {
    run_console_fixture(DETACHED_PROCESS, "detached-parent");
}

#[test]
fn supervised_children_preserve_an_existing_hidden_console() {
    run_console_fixture(CREATE_NO_WINDOW, "attached-parent");
}

// Each console scenario runs in a fresh process: detaching the test runner's
// own console would interfere with concurrently running tests.
#[test]
fn windows_fixture() {
    let Ok(role) = std::env::var(FIXTURE_ROLE) else {
        return;
    };
    if role == "arguments" {
        println!("fixture-ok");
        return;
    }
    if role == "child" {
        let parent: u32 = std::env::var("III_COMPOSE_WINDOWS_TEST_PARENT")
            .unwrap()
            .parse()
            .unwrap();
        let mut pids = [0; 16];
        let count = unsafe { GetConsoleProcessList(pids.as_mut_ptr(), pids.len() as u32) } as usize;
        println!(
            "console-window={}",
            !unsafe { GetConsoleWindow() }.is_null()
        );
        println!(
            "shares-parent-console={}",
            pids[..count.min(pids.len())].contains(&parent)
        );
        // Stay alive until supervision has assigned the job and the parent
        // drops its stdin writer, even on very fast test machines.
        std::io::Read::read_to_end(&mut std::io::stdin(), &mut Vec::new()).unwrap();
        return;
    }
    let detached = role == "detached-parent";
    assert_eq!(console_process_count() == 0, detached);

    tokio::runtime::Runtime::new().unwrap().block_on(async {
        let mut command = tokio::process::Command::new(std::env::current_exe().unwrap());
        command
            .args(["--exact", "windows_fixture", "--nocapture"])
            .env(FIXTURE_ROLE, "child")
            .env(
                "III_COMPOSE_WINDOWS_TEST_PARENT",
                std::process::id().to_string(),
            )
            .stdin(std::process::Stdio::piped());
        let (child, mut output, stdin) = spawn_supervised_piped_with_stdin(command).unwrap();
        drop(stdin);
        let mut stdout = String::new();
        let mut stderr = String::new();
        let result = tokio::time::timeout(Duration::from_secs(10), async {
            tokio::join!(
                output.stdout.as_mut().unwrap().read_to_string(&mut stdout),
                output.stderr.as_mut().unwrap().read_to_string(&mut stderr),
                child.wait()
            )
        })
        .await;
        let (out, err, status) = match result {
            Ok(result) => result,
            Err(error) => {
                child.stop(Duration::ZERO).await;
                panic!("console fixture timed out: {error}");
            }
        };
        out.unwrap();
        err.unwrap();
        assert!(status.success(), "stdout={stdout}, stderr={stderr}");
        assert!(stdout.contains("console-window=false"), "stdout={stdout}");
        assert!(
            stdout.contains(&format!("shares-parent-console={}", !detached)),
            "stdout={stdout}"
        );
    });
}
