//! Mode selection: what the three remaining flags resolve to.
//!
//! There is one command now. Everything an operator does to a project goes
//! through `iii trigger compose::*`, so the only decisions left here are where
//! the daemon runs and which engine it talks to.

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
fn bare_compose_serves_in_the_foreground() {
    // No id, no file, no namespace: a daemon starts knowing nothing and learns
    // about a project the first time a call names one.
    // The address is asserted in the III_URL test and nowhere else: it reads
    // process-wide state, and two tests reading it while one writes it is a
    // flake waiting for a loaded machine.
    match parse(&["iii", "compose"]).plan().unwrap() {
        ComposeCommand::Serve { .. } => {}
        other => panic!("expected Serve, got {other:?}"),
    }
}

#[test]
fn the_project_flags_are_gone_rather_than_ignored() {
    // They mean something else now — `id` and `file` are call arguments, not
    // process arguments. Accepting them here would silently do nothing, so
    // parsing has to fail and say so.
    for removed in [
        &["iii", "compose", "--id", "a"][..],
        &["iii", "compose", "--file", "c.yaml"][..],
        &["iii", "compose", "--ns", "a"][..],
        &["iii", "compose", "up"][..],
        &["iii", "compose", "down"][..],
        &["iii", "compose", "logs"][..],
        &["iii", "compose", "stop"][..],
    ] {
        assert!(
            Wrapper::try_parse_from(removed).is_err(),
            "{removed:?} should no longer parse"
        );
    }
}

#[test]
fn attach_and_detach_are_the_only_modes_left() {
    match parse(&["iii", "compose", "--attach"]).plan().unwrap() {
        ComposeCommand::Attach => {}
        other => panic!("expected Attach, got {other:?}"),
    }

    // Asking for both is a contradiction: one backgrounds a new daemon, the
    // other follows one that is already running.
    let err = parse(&["iii", "compose", "-d", "--attach"])
        .plan()
        .expect_err("both at once means nothing");
    assert_eq!(err.code(), "CONFLICTING_FLAGS");
}

#[test]
fn detach_is_carried_unless_the_guard_says_it_already_happened() {
    // One test, not several: the guard lives in the process environment, and
    // tests share a process. Setting it in one while another reads it is a
    // flake waiting for a loaded machine.
    // `-d` re-execs, so whatever resolved the address has to be what the child
    // is told; recomputing it there would read a different environment.
    match parse(&["iii", "compose", "-d", "--engine", "ws://host:1/"])
        .plan()
        .unwrap()
    {
        ComposeCommand::Detach { engine_url } => assert_eq!(engine_url, "ws://host:1/"),
        other => panic!("expected Detach, got {other:?}"),
    }

    // The background process is launched with the same argv, so a `-d` still
    // in it must not make it fork a second time.
    unsafe { std::env::set_var(iii_compose::cli::DETACHED_GUARD, "1") };
    let planned = parse(&["iii", "compose", "--detach"]).plan();
    unsafe { std::env::remove_var(iii_compose::cli::DETACHED_GUARD) };

    match planned.unwrap() {
        ComposeCommand::Serve { .. } => {}
        other => panic!("the guard should force foreground, got {other:?}"),
    }
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
    assert_eq!(from_default, iii_compose::cli::DEFAULT_ENGINE_URL);
}

#[test]
fn a_project_namespace_still_comes_from_the_file_or_default() {
    // Unchanged by the redesign, and worth keeping honest: the namespace is
    // the workers' address, never the project id. Nothing is hashed in, so it
    // stays something an operator can type.
    use iii_compose::namespace::{DEFAULT_NAMESPACE, project_namespace};

    assert_eq!(project_namespace(Some("shop"), Some("loja")), "shop");
    assert_eq!(project_namespace(None, Some("loja")), "loja");
    assert_eq!(project_namespace(None, None), DEFAULT_NAMESPACE);
}
