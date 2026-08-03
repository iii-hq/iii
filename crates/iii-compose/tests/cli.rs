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
fn up_is_a_subcommand_and_bare_daemon_mode_starts_nothing() {
    // `up` and `stop` are the pair an operator reaches for. A flag on one side
    // of that pair reads as an afterthought — and bare daemon mode serving
    // without starting anything is a separate, deliberate mode.
    let bare = parse(&["iii", "compose", "--id", "host-a", "--file", "c.yaml"])
        .plan()
        .unwrap();
    let upped = parse(&["iii", "compose", "up", "--id", "host-a", "--file", "c.yaml"])
        .plan()
        .unwrap();

    match (bare, upped) {
        (
            ComposeCommand::Daemon {
                up_on_start: plain, ..
            },
            ComposeCommand::Daemon {
                up_on_start: one_shot,
                id,
                ..
            },
        ) => {
            assert!(!plain, "the daemon alone brings nothing up");
            assert!(one_shot);
            assert_eq!(id, "host-a");
        }
        other => panic!("expected two Daemons, got {other:?}"),
    }
}

#[test]
fn up_takes_the_id_before_or_after_the_subcommand() {
    let after = parse(&["iii", "compose", "up", "--id", "host-a", "-f", "c.yaml"])
        .plan()
        .unwrap();
    let before = parse(&["iii", "compose", "--id", "host-a", "-f", "c.yaml", "up"])
        .plan()
        .unwrap();

    for planned in [after, before] {
        match planned {
            ComposeCommand::Daemon { id, up_on_start, .. } => {
                assert_eq!(id, "host-a");
                assert!(up_on_start);
            }
            other => panic!("expected Daemon, got {other:?}"),
        }
    }
}

#[test]
fn up_can_detach() {
    let cli = parse(&["iii", "compose", "up", "--id", "host-a", "-f", "c.yaml", "-d"]);
    match cli.plan().unwrap() {
        ComposeCommand::Daemon { detach, .. } => assert!(detach),
        other => panic!("expected Daemon, got {other:?}"),
    }
}

#[test]
fn up_without_an_id_says_which_flag_is_missing() {
    let err = parse(&["iii", "compose", "up", "-f", "c.yaml"])
        .plan()
        .expect_err("--id names the daemon");
    assert_eq!(err.code(), "MISSING_FLAG");
}

// ── `iii compose logs` ───────────────────────────────────────────────────

#[test]
fn logs_targets_a_daemon_by_namespace_and_needs_no_compose_file() {
    // Deliberately from a directory with no worker-compose.yaml: the daemon
    // holds the project, this only asks it a question.
    let cli = parse(&["iii", "compose", "logs", "--namespace", "orders"]);
    let command = cli.plan().expect("logs should not require a compose file");

    match command {
        ComposeCommand::Logs {
            namespace,
            container,
            tail,
            follow,
            ..
        } => {
            assert_eq!(namespace, "orders");
            assert_eq!(container, None, "no --container means every container");
            assert_eq!(tail, iii_compose::cli::DEFAULT_TAIL);
            assert!(!follow, "following is opt-in");
        }
        other => panic!("expected Logs, got {other:?}"),
    }
}

#[test]
fn ns_is_the_short_spelling_of_namespace() {
    let cli = parse(&["iii", "compose", "logs", "--ns", "orders"]);
    match cli.plan().unwrap() {
        ComposeCommand::Logs { namespace, .. } => assert_eq!(namespace, "orders"),
        other => panic!("expected Logs, got {other:?}"),
    }
}

#[test]
fn logs_carries_the_container_tail_and_follow() {
    let cli = parse(&[
        "iii", "compose", "logs", "--ns", "orders", "-c", "api", "-n", "5", "-f",
    ]);

    match cli.plan().unwrap() {
        ComposeCommand::Logs {
            container,
            tail,
            follow,
            ..
        } => {
            assert_eq!(container.as_deref(), Some("api"));
            assert_eq!(tail, 5);
            // `-f` means follow here. It is the compose file everywhere else,
            // which is why `--file` is not a global flag.
            assert!(follow);
        }
        other => panic!("expected Logs, got {other:?}"),
    }
}

#[test]
fn logs_without_a_namespace_does_not_parse() {
    // `compose::*` lives in the namespace of the daemon's id, so without one
    // there is nothing to address.
    let missing = std::panic::catch_unwind(|| parse(&["iii", "compose", "logs"]));
    assert!(missing.is_err(), "--namespace is what names the daemon");
}

#[test]
fn detach_is_carried_into_daemon_mode_unless_already_detached() {
    // One test, not two: the guard lives in the process environment, and tests
    // share a process. Setting it in one while another reads it is a flake
    // waiting for a loaded machine.
    let cli = parse(&["iii", "compose", "--id", "host-a", "--file", "c.yaml", "-d"]);
    match cli.plan().unwrap() {
        ComposeCommand::Daemon { detach, .. } => assert!(detach),
        other => panic!("expected Daemon, got {other:?}"),
    }

    // The background process is launched with this set, so a `-d` still in its
    // own argv cannot make it fork a second time.
    unsafe { std::env::set_var(iii_compose::cli::DETACHED_GUARD, "1") };
    let planned = parse(&["iii", "compose", "--id", "host-a", "--file", "c.yaml", "--detach"]).plan();
    unsafe { std::env::remove_var(iii_compose::cli::DETACHED_GUARD) };

    match planned.unwrap() {
        ComposeCommand::Daemon { detach, .. } => assert!(!detach),
        other => panic!("expected Daemon, got {other:?}"),
    }
}

#[test]
fn detaching_is_not_offered_where_there_is_no_daemon() {
    // `-d` is not a global: on `logs` the short flag means nothing, and on
    // `validate` there is no process to background.
    assert!(Wrapper::try_parse_from(["iii", "compose", "logs", "--ns", "a", "-d"]).is_err());
    assert!(Wrapper::try_parse_from(["iii", "compose", "validate", "-d"]).is_err());
}

// ── `iii compose stop` ───────────────────────────────────────────────────

#[test]
fn stop_targets_a_daemon_by_namespace() {
    // Same addressing as `logs`, and no compose file either: the daemon holds
    // the project, this only tells it to let go.
    let cli = parse(&["iii", "compose", "stop", "--namespace", "orders"]);
    match cli.plan().expect("stop should not require a compose file") {
        ComposeCommand::Stop { namespace, .. } => assert_eq!(namespace, "orders"),
        other => panic!("expected Stop, got {other:?}"),
    }
}

#[test]
fn stop_accepts_the_ns_alias() {
    let cli = parse(&["iii", "compose", "stop", "--ns", "orders"]);
    match cli.plan().unwrap() {
        ComposeCommand::Stop { namespace, .. } => assert_eq!(namespace, "orders"),
        other => panic!("expected Stop, got {other:?}"),
    }
}

#[test]
fn stop_without_a_namespace_does_not_parse() {
    let missing = std::panic::catch_unwind(|| parse(&["iii", "compose", "stop"]));
    assert!(missing.is_err(), "--namespace is what names the daemon");
}

#[test]
fn stop_takes_the_engine_from_the_environment_like_everything_else() {
    let cli = parse(&["iii", "compose", "stop", "--ns", "a", "--engine", "ws://host:1/"]);
    match cli.plan().unwrap() {
        ComposeCommand::Stop { engine_url, .. } => assert_eq!(engine_url, "ws://host:1/"),
        other => panic!("expected Stop, got {other:?}"),
    }
}
