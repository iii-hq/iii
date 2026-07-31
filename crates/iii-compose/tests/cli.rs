//! Mode selection: which flags each `iii compose` invocation requires.

use std::path::PathBuf;

use clap::Parser;
use iii_compose::{ComposeCli, ComposeCommand};

/// Mirrors how the engine mounts the subcommand, so parsing is exercised
/// through the same shape the real binary uses.
#[derive(Parser, Debug)]
enum Wrapper {
    Compose(ComposeCli),
}

fn parse(args: &[&str]) -> ComposeCli {
    match Wrapper::try_parse_from(args).expect("arguments should parse") {
        Wrapper::Compose(cli) => cli,
    }
}

#[test]
fn daemon_mode_requires_an_id() {
    let missing_id = parse(&["iii", "compose", "--file", "worker-compose.yaml"])
        .plan()
        .expect_err("daemon mode without --id is incomplete");
    assert_eq!(missing_id.code(), "MISSING_FLAG");
    assert!(missing_id.to_string().contains("--id"));
}

/// The compose-file default, in one test: the current directory is
/// process-wide state, and cargo runs tests in threads, so these assertions
/// cannot be split into separate test functions without racing each other.
#[test]
fn the_compose_file_defaults_to_the_one_in_this_directory() {
    let project = tempfile::tempdir().unwrap();
    std::fs::write(
        project.path().join("worker-compose.yaml"),
        "name: x\ncontainers:\n  a:\n    worker: path://./a\n",
    )
    .unwrap();
    let empty = tempfile::tempdir().unwrap();
    let previous = std::env::current_dir().expect("cwd");

    // In a project directory, naming the file is not required.
    std::env::set_current_dir(project.path()).unwrap();
    let defaulted = parse(&["iii", "compose", "--id", "host-a"]).plan();
    let explicit = parse(&["iii", "compose", "--id", "host-a", "-f", "other.yaml"]).plan();
    let validated = parse(&["iii", "compose", "validate"]).plan();

    // Outside one, the error names the way out instead of blaming a path the
    // operator never typed.
    std::env::set_current_dir(empty.path()).unwrap();
    let nowhere = parse(&["iii", "compose", "--id", "host-a"]).plan();

    std::env::set_current_dir(previous).unwrap();

    match defaulted.expect("a project directory needs no --file") {
        ComposeCommand::Daemon { file, .. } => {
            assert_eq!(file, PathBuf::from("worker-compose.yaml"))
        }
        other => panic!("expected daemon mode, got {other:?}"),
    }
    match explicit.expect("an explicit file parses") {
        ComposeCommand::Daemon { file, .. } => assert_eq!(file, PathBuf::from("other.yaml")),
        other => panic!("expected daemon mode, got {other:?}"),
    }
    match validated.expect("validate shares the default") {
        ComposeCommand::Validate { file, .. } => {
            assert_eq!(file, PathBuf::from("worker-compose.yaml"))
        }
        other => panic!("expected validate, got {other:?}"),
    }

    let err = nowhere.expect_err("there is no project in an empty directory");
    assert_eq!(err.code(), "NO_COMPOSE_FILE");
    assert!(err.to_string().contains("--file"), "{err}");
}

#[test]
fn the_short_file_flag_works() {
    let cli = parse(&["iii", "compose", "validate", "-f", "compose.yaml"]);
    assert_eq!(
        cli.plan().unwrap(),
        ComposeCommand::Validate {
            file: PathBuf::from("compose.yaml"),
            namespace: None,
        }
    );
}

#[test]
fn an_explicit_engine_beats_the_environment() {
    // The only test that touches III_URL, so no other test can race it.
    unsafe { std::env::set_var("III_URL", "ws://from-env:1") };

    let from_flag = parse(&["iii", "compose", "--engine", "ws://from-flag:2"]).engine_url();
    let from_env = parse(&["iii", "compose"]).engine_url();

    unsafe { std::env::remove_var("III_URL") };
    let from_default = parse(&["iii", "compose"]).engine_url();

    assert_eq!(from_flag, "ws://from-flag:2");
    assert_eq!(from_env, "ws://from-env:1");
    assert_eq!(from_default, "ws://127.0.0.1:49134");
}

/// `--up` is what makes compose usable from a script: bring the project up and
/// fail the process if it does not come up.
#[test]
fn the_up_flag_is_carried_into_daemon_mode() {
    let plain = parse(&["iii", "compose", "--id", "host-a", "--file", "c.yaml"])
        .plan()
        .unwrap();
    let one_shot = parse(&[
        "iii", "compose", "--id", "host-a", "--file", "c.yaml", "--up",
    ])
    .plan()
    .unwrap();

    match (plain, one_shot) {
        (
            ComposeCommand::Daemon {
                up_on_start: plain, ..
            },
            ComposeCommand::Daemon {
                up_on_start: one_shot,
                ..
            },
        ) => {
            assert!(!plain, "a daemon without --up must not start anything");
            assert!(one_shot);
        }
        _ => panic!("expected daemon mode"),
    }
}
