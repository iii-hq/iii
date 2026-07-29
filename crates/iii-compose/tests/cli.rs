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
fn daemon_mode_requires_an_id_and_a_file() {
    let missing_id = parse(&["iii", "compose", "--file", "worker-compose.yaml"])
        .plan()
        .expect_err("daemon mode without --id is incomplete");
    assert_eq!(missing_id.code(), "MISSING_FLAG");
    assert!(missing_id.to_string().contains("--id"));

    let missing_file = parse(&["iii", "compose", "--id", "host-a"])
        .plan()
        .expect_err("daemon mode without --file is incomplete");
    assert_eq!(missing_file.code(), "MISSING_FLAG");
    assert!(missing_file.to_string().contains("--file"));
}

#[test]
fn daemon_mode_resolves_id_file_and_engine() {
    let cli = parse(&[
        "iii",
        "compose",
        "--id",
        "host-a",
        "--engine",
        "wss://engine.example",
        "--file",
        "/srv/app/worker-compose.yaml",
    ]);

    assert_eq!(
        cli.plan().unwrap(),
        ComposeCommand::Daemon {
            id: "host-a".to_string(),
            file: PathBuf::from("/srv/app/worker-compose.yaml"),
            engine_url: "wss://engine.example".to_string(),
            namespace: None,
        }
    );
}

#[test]
fn validate_needs_no_id_and_accepts_the_file_after_the_subcommand() {
    let cli = parse(&[
        "iii",
        "compose",
        "validate",
        "--file",
        "/srv/app/worker-compose.yaml",
    ]);

    assert_eq!(
        cli.plan().unwrap(),
        ComposeCommand::Validate {
            file: PathBuf::from("/srv/app/worker-compose.yaml"),
        }
    );
}

#[test]
fn validate_still_requires_a_file() {
    let err = parse(&["iii", "compose", "validate"])
        .plan()
        .expect_err("validate needs a file");
    assert_eq!(err.code(), "MISSING_FLAG");
}

#[test]
fn the_short_file_flag_works() {
    let cli = parse(&["iii", "compose", "validate", "-f", "compose.yaml"]);
    assert_eq!(
        cli.plan().unwrap(),
        ComposeCommand::Validate {
            file: PathBuf::from("compose.yaml"),
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
