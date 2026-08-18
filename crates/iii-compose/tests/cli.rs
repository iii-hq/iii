//! Mode selection: what an invocation resolves to.
//!
//! Compose always serves in the foreground. The decisions left here are which
//! namespace this daemon answers in, which engine it talks to, and whether it
//! starts holding a project or holding nothing.

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
    let ComposeCommand::Serve { .. } = parse(&["iii", "compose"]).plan().unwrap();
}

#[test]
fn an_unnamed_daemon_names_itself() {
    // Without an id there is no safe well-known name to fall back to: two
    // daemons sharing one would be the collision the id exists to prevent, and
    // the second would be refused. So an invocation that does not name itself
    // gets a name — printed on start, and what an operator captures to address
    // it.
    let ComposeCommand::Serve {
        daemon_namespace: first,
        ..
    } = parse(&["iii", "compose"]).plan().unwrap();
    let ComposeCommand::Serve {
        daemon_namespace: second,
        ..
    } = parse(&["iii", "compose"]).plan().unwrap();

    // Two words, because the name is printed for an operator to read once and
    // type from memory. It is still held to the namespace charset: it is also a
    // directory and a routing key.
    assert!(
        iii_compose::namespace::check(&first).is_ok(),
        "a generated name must be a valid namespace, got {first:?}"
    );
    assert_eq!(
        first.split('-').count(),
        2,
        "expected adjective-noun, got {first:?}"
    );
    // Not a uniqueness guarantee — two words collide, and `generate` is given a
    // predicate for the state directory precisely because they do. This only
    // catches a generator that returns a constant.
    assert_ne!(
        first, second,
        "two daemons that did not name themselves drew the same name"
    );
}

#[test]
fn the_namespace_is_how_one_daemon_is_told_from_another() {
    // Several daemons attach to one engine. The id is what tells them apart:
    // it is the namespace this one answers `compose::*` in, so an operator
    // reaches exactly one with `--namespace`.
    let ComposeCommand::Serve {
        daemon_namespace, ..
    } = parse(&["iii", "compose", "--namespace", "pc-da-xuxa"])
        .plan()
        .unwrap();
    assert_eq!(daemon_namespace, "pc-da-xuxa");
}

#[test]
fn a_namespace_that_cannot_also_be_a_directory_is_refused() {
    // It is both the namespace the engine routes on and a directory under
    // ~/.iii/compose, so a separator or an empty string would be a broken
    // daemon discovered later, at the first write.
    for bad in ["", "   ", "a/b", "a\\b", ".."] {
        let err = parse(&["iii", "compose", "--namespace", bad])
            .plan()
            .expect_err("{bad:?} should be refused");
        assert_eq!(err.code(), "INVALID_NAMESPACE", "for {bad:?}");
    }
}

#[test]
fn the_flag_is_held_to_the_same_charset_as_the_file() {
    // The flag used to refuse a path separator and accept a space, while
    // `name:` in the compose file was silently rewritten to fit. One string
    // therefore meant two different namespaces depending on which of the two
    // said it: `'My Shop!'` on the command line registered under `My Shop!`,
    // and `name: "My Shop!"` registered under `my-shop`.
    for bad in ["My Shop!", "my shop", "MY-SHOP", "a.b", "olá"] {
        let err = parse(&["iii", "compose", "--namespace", bad])
            .plan()
            .expect_err("{bad:?} should be refused");
        assert_eq!(err.code(), "INVALID_NAMESPACE", "for {bad:?}");
    }

    for good in ["my-shop", "shop_2", "a"] {
        assert!(
            parse(&["iii", "compose", "--namespace", good])
                .plan()
                .is_ok(),
            "{good:?} should be accepted"
        );
    }
}

#[test]
fn the_project_flags_are_gone_rather_than_ignored() {
    // Everything an operator does to a running project is a `compose::*` call,
    // so `down`, `logs` and `stop` are not process arguments. `up` is the one
    // exception, and only because a daemon has to exist before the first call
    // can be made: it is the step that cannot be a call.
    //
    // `--file` belongs to `up` alone. On the bare command there is no project
    // for it to name, and accepting it would silently do nothing.
    for removed in [
        &["iii", "compose", "--file", "c.yaml"][..],
        &["iii", "compose", "--id", "a"][..],
        &["iii", "compose", "down"][..],
        &["iii", "compose", "logs"][..],
        &["iii", "compose", "stop"][..],
        // Backgrounding is the supervisor's job, not compose's: it never
        // daemonises itself, so there is nothing to attach back to.
        &["iii", "compose", "-d"][..],
        &["iii", "compose", "--detach"][..],
        &["iii", "compose", "--attach"][..],
    ] {
        assert!(
            Wrapper::try_parse_from(removed).is_err(),
            "{removed:?} should no longer parse"
        );
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

#[test]
fn bare_compose_starts_holding_nothing() {
    let ComposeCommand::Serve { start, .. } = parse(&["iii", "compose"]).plan().unwrap();
    assert_eq!(
        start, None,
        "a bare daemon learns about a project from a call"
    );
}

#[test]
fn up_names_the_file_in_the_current_directory() {
    // The point of the command: in a project directory, one word starts it.
    let ComposeCommand::Serve { start, .. } = parse(&["iii", "compose", "up"]).plan().unwrap();
    assert_eq!(
        start.as_deref(),
        Some(std::path::Path::new("worker-compose.yaml"))
    );
}

#[test]
fn up_takes_a_file_of_its_own() {
    let ComposeCommand::Serve { start, .. } =
        parse(&["iii", "compose", "up", "-f", "./other.yaml"])
            .plan()
            .unwrap();
    assert_eq!(start.as_deref(), Some(std::path::Path::new("./other.yaml")));
}

#[test]
fn the_namespace_reads_the_same_on_either_side_of_the_subcommand() {
    // `-n` is global, so an operator who types it where it feels natural does
    // not get told it belongs somewhere else.
    for args in [
        ["iii", "compose", "-n", "orders", "up"],
        ["iii", "compose", "up", "-n", "orders"],
    ] {
        let cli = parse(&args);
        assert_eq!(cli.daemon_namespace().unwrap(), "orders");
        let ComposeCommand::Serve { start, .. } = cli.plan().unwrap();
        assert!(start.is_some(), "{args:?} should still be an up");
    }
}
