//! Mode selection: what the two remaining flags resolve to.
//!
//! There is one command now, and it only ever serves in the foreground.
//! Everything an operator does to a project goes through `iii trigger
//! compose::*`, so the only decisions left here are which namespace this
//! daemon answers in and which engine it talks to.

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
