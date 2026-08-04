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
    // No id and no file: a daemon starts knowing nothing and learns about a
    // project the first time a call names one.
    // The address is asserted in the III_URL test and nowhere else: it reads
    // process-wide state, and two tests reading it while one writes it is a
    // flake waiting for a loaded machine.
    match parse(&["iii", "compose"]).plan().unwrap() {
        ComposeCommand::Serve { .. } => {}
        other => panic!("expected Serve, got {other:?}"),
    }
}

#[test]
fn an_unnamed_daemon_names_itself() {
    // Without an id there is no safe well-known name to fall back to: two
    // daemons sharing one would be the collision the id exists to prevent, and
    // the second would be refused. So an invocation that does not name itself
    // gets a name — printed on start, and what an operator captures to address
    // it.
    let first = match parse(&["iii", "compose"]).plan().unwrap() {
        ComposeCommand::Serve { daemon_namespace, .. } => daemon_namespace,
        other => panic!("expected Serve, got {other:?}"),
    };
    let second = match parse(&["iii", "compose"]).plan().unwrap() {
        ComposeCommand::Serve { daemon_namespace, .. } => daemon_namespace,
        other => panic!("expected Serve, got {other:?}"),
    };

    assert!(
        uuid::Uuid::parse_str(&first).is_ok(),
        "an unnamed daemon should get a uuid, got {first:?}"
    );
    assert_ne!(
        first, second,
        "two daemons that did not name themselves must not collide"
    );
}

#[test]
fn the_namespace_is_how_one_daemon_is_told_from_another() {
    // Several daemons attach to one engine. The id is what tells them apart:
    // it is the namespace this one answers `compose::*` in, so an operator
    // reaches exactly one with `--namespace`.
    match parse(&["iii", "compose", "--ns", "pc-da-xuxa"])
        .plan()
        .unwrap()
    {
        ComposeCommand::Serve { daemon_namespace, .. } => assert_eq!(daemon_namespace, "pc-da-xuxa"),
        other => panic!("expected Serve, got {other:?}"),
    }

    // The view has to name the same daemon the foreground would, or `--attach`
    // follows a neighbour's log. (`Detach` carries it too; that is asserted in
    // the test that owns DETACHED_GUARD, since reading it here would race.)
    match parse(&["iii", "compose", "--ns", "pc-a", "--attach"])
        .plan()
        .unwrap()
    {
        ComposeCommand::Attach { daemon_namespace } => {
            assert_eq!(daemon_namespace.as_deref(), Some("pc-a"));
        }
        other => panic!("expected Attach, got {other:?}"),
    }
}

#[test]
fn a_namespace_that_cannot_also_be_a_directory_is_refused() {
    // It is both the namespace the engine routes on and a directory under
    // ~/.iii/compose, so a separator or an empty string would be a broken
    // daemon discovered later, at the first write.
    for bad in ["", "   ", "a/b", "a\\b", ".."] {
        let err = parse(&["iii", "compose", "--ns", bad])
            .plan()
            .expect_err("{bad:?} should be refused");
        assert_eq!(err.code(), "INVALID_NAMESPACE", "for {bad:?}");
    }
}

#[test]
fn the_project_flags_are_gone_rather_than_ignored() {
    // `file` is a call argument, not a process argument, and the subcommands
    // are `compose::*` calls now. Accepting either here would silently do
    // nothing, so parsing has to fail and say so.
    for removed in [
        &["iii", "compose", "--file", "c.yaml"][..],
        &["iii", "compose", "--id", "a"][..],
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
    // Nothing is generated here: `--attach` follows a daemon that already
    // exists, so an unnamed one is resolved against what has run on this
    // machine rather than invented.
    match parse(&["iii", "compose", "--attach"]).plan().unwrap() {
        ComposeCommand::Attach { daemon_namespace } => assert_eq!(daemon_namespace, None),
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
    match parse(&["iii", "compose", "-d", "--engine", "ws://host:1/", "--ns", "pc-a"])
        .plan()
        .unwrap()
    {
        // The id goes with it for the same reason the address does: the child
        // must come back as the same daemon, not as `default`.
        ComposeCommand::Detach {
            engine_url,
            daemon_namespace,
        } => {
            assert_eq!(engine_url, "ws://host:1/");
            assert_eq!(daemon_namespace, "pc-a");
        }
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
