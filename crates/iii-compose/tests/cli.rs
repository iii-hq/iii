//! Mode selection: what an invocation resolves to.
//!
//! Compose always serves in the foreground. The decisions left here are which
//! namespace this daemon answers in, which engine it talks to, and whether it
//! starts holding a project or holding nothing.

use clap::Parser;
use iii_compose::{
    BuildCli, ComposeCli, ComposeCommand, ComposeFile, ComposeSubcommand, EngineMode,
    resolve_engine_mode,
};

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
    let ComposeCommand::Serve { .. } = parse(&["iii", "compose"]).plan().unwrap() else {
        panic!("expected serve command");
    };
}

#[test]
fn an_unnamed_daemon_uses_default() {
    assert_eq!(
        parse(&["iii", "compose"]).daemon_namespace().unwrap(),
        iii_compose::namespace::DEFAULT_NAMESPACE
    );
}

#[test]
fn the_namespace_is_how_one_daemon_is_told_from_another() {
    // Several daemons attach to one engine. The id is what tells them apart:
    // it is the namespace this one answers `compose::*` in, so an operator
    // reaches exactly one with `--namespace`.
    let ComposeCommand::Serve {
        explicit_daemon_namespace,
        ..
    } = parse(&["iii", "compose", "--namespace", "pc-da-xuxa"])
        .plan()
        .unwrap()
    else {
        panic!("expected serve command");
    };
    assert_eq!(explicit_daemon_namespace.as_deref(), Some("pc-da-xuxa"));
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
    // so `down`, `logs` and `stop` are not process arguments. `--up` is the one
    // exception, and only because a daemon has to exist before the first call
    // can be made: it is the step that cannot be a call.
    //
    // `--file` belongs to `--up` alone. On the bare command there is no project
    // for it to name, and accepting it would silently do nothing.
    for removed in [
        &["iii", "compose", "--file", "c.yaml"][..],
        &["iii", "compose", "--id", "a"][..],
        &["iii", "compose", "down"][..],
        &["iii", "compose", "stop"][..],
        // Backgrounding is the supervisor's job, not compose's: it never
        // daemonises itself, so there is nothing to attach back to.
        &["iii", "compose", "-d"][..],
        &["iii", "compose", "--detach"][..],
        &["iii", "compose", "--attach"][..],
        &["iii", "compose", "up"][..],
        &["iii", "compose", "--up", "--no-engine"][..],
    ] {
        assert!(
            Wrapper::try_parse_from(removed).is_err(),
            "{removed:?} should no longer parse"
        );
    }
}

#[test]
fn a_programmatic_file_without_up_is_refused() {
    let err = ComposeCli {
        engine: None,
        ns: None,
        up: false,
        file: Some("other.yaml".into()),
        command: None,
    }
    .plan()
    .expect_err("a public ComposeCli must enforce the same rule as clap");

    assert_eq!(err.code(), "FILE_REQUIRES_UP");
}

#[test]
fn logs_resolves_to_a_remote_read_without_starting_a_daemon() {
    let ComposeCommand::Logs {
        container,
        tail,
        follow,
        stream,
        ..
    } = parse(&[
        "iii",
        "compose",
        "logs",
        "queue",
        "--namespace",
        "dev",
        "--tail",
        "50",
        "--follow",
        "--stream",
        "stderr",
    ])
    .plan()
    .unwrap()
    else {
        panic!("expected logs command");
    };

    assert_eq!(container.as_deref(), Some("queue"));
    assert_eq!(tail, 50);
    assert!(follow);
    assert_eq!(stream, Some(iii_compose::logs::LogStream::Stderr));
}

#[test]
fn logs_rejects_an_unbounded_initial_tail() {
    let error = Wrapper::try_parse_from(["iii", "compose", "logs", "--tail", "1001"])
        .expect_err("the CLI should bound one log response");

    assert!(error.to_string().contains("tail must not exceed 1000"));
}

#[test]
fn build_uses_the_default_compose_file() {
    let ComposeCommand::Build { file } = parse(&["iii", "compose", "build"]).plan().unwrap() else {
        panic!("expected build command");
    };
    assert_eq!(file, std::path::Path::new("worker-compose.yaml"));
}

#[test]
fn build_accepts_its_own_file() {
    let ComposeCommand::Build { file } =
        parse(&["iii", "compose", "build", "--file", "other.yaml"])
            .plan()
            .unwrap()
    else {
        panic!("expected build command");
    };
    assert_eq!(file, std::path::Path::new("other.yaml"));
}

#[test]
fn build_conflicts_with_daemon_options() {
    for args in [
        &["iii", "compose", "--up", "build"][..],
        &["iii", "compose", "--engine", "ws://host:1", "build"][..],
        &["iii", "compose", "--namespace", "dev", "build"][..],
    ] {
        assert!(Wrapper::try_parse_from(args).is_err(), "{args:?} must fail");
    }

    let error = ComposeCli {
        engine: None,
        ns: None,
        up: true,
        file: None,
        command: Some(ComposeSubcommand::Build(BuildCli {
            file: "worker-compose.yaml".into(),
        })),
    }
    .plan()
    .unwrap_err();
    assert_eq!(error.code(), "BUILD_CONFLICTS_WITH_SERVE_OPTIONS");
}

#[test]
fn requested_engine_url_reports_only_cli_or_environment_values() {
    // The only test that touches III_URL, so no other test can race it.
    unsafe { std::env::set_var("III_URL", "ws://from-env:1") };

    let from_flag =
        parse(&["iii", "compose", "--engine", "ws://from-flag:2"]).requested_engine_url();
    let from_env = parse(&["iii", "compose"]).requested_engine_url();

    unsafe { std::env::remove_var("III_URL") };
    let from_default = parse(&["iii", "compose"]).requested_engine_url();

    assert_eq!(from_flag.as_deref(), Some("ws://from-flag:2"));
    assert_eq!(from_env.as_deref(), Some("ws://from-env:1"));
    assert_eq!(from_default, None);
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
    let ComposeCommand::Serve { file, start, .. } =
        parse(&["iii", "compose", "--engine", "ws://shared:49134"])
            .plan()
            .unwrap()
    else {
        panic!("expected serve command");
    };
    assert_eq!(file, std::path::Path::new("worker-compose.yaml"));
    assert!(!start, "a bare daemon must not start the default file");
}

#[test]
fn up_names_the_file_in_the_current_directory() {
    // The point of the flag: in a project directory, one option starts it.
    let ComposeCommand::Serve { file, start, .. } =
        parse(&["iii", "compose", "--up"]).plan().unwrap()
    else {
        panic!("expected serve command");
    };
    assert_eq!(file, std::path::Path::new("worker-compose.yaml"));
    assert!(start);
}

#[test]
fn production_docker_compose_command_matches_the_cli() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("engine/docker-compose.prod.yml");
    let text = std::fs::read_to_string(&path).unwrap();
    let document: serde_yaml::Value = serde_yaml::from_str(&text).unwrap();
    let command = document["services"]["iii"]["command"]
        .as_sequence()
        .unwrap();
    let args = std::iter::once("iii".to_string())
        .chain(command.iter().map(|arg| arg.as_str().unwrap().to_string()))
        .collect::<Vec<_>>();

    let Wrapper::Compose(cli) = Wrapper::try_parse_from(args).unwrap();
    let ComposeCommand::Serve {
        explicit_daemon_namespace,
        file,
        start,
        ..
    } = cli.plan().unwrap()
    else {
        panic!("expected serve command");
    };

    assert_eq!(explicit_daemon_namespace.as_deref(), Some("production"));
    assert_eq!(file, std::path::Path::new("/app/worker-compose.yaml"));
    assert!(start);
}

#[test]
fn up_without_a_cli_namespace_defers_to_the_compose_file() {
    let ComposeCommand::Serve {
        explicit_daemon_namespace,
        start,
        ..
    } = parse(&["iii", "compose", "--up"]).plan().unwrap()
    else {
        panic!("expected serve command");
    };

    assert_eq!(explicit_daemon_namespace, None);
    assert!(start);
}

#[test]
fn up_uses_the_file_engine_when_the_cli_does_not_override_it() {
    let managed = ComposeFile::parse(
        "engine: { workers: {} }\ncontainers: {}\n",
        "/srv/app/worker-compose.yaml",
    )
    .unwrap();

    assert_eq!(
        resolve_engine_mode(
            Some(&managed),
            true,
            None,
            Some("ws://global-environment:49134"),
        ),
        EngineMode::Managed {
            url: "ws://127.0.0.1:49134".to_string()
        },
        "a process-wide III_URL must not override a file-owned engine"
    );
}

#[test]
fn explicit_engine_overrides_the_file_engine_with_up() {
    let managed = ComposeFile::parse(
        "engine: { workers: {} }\ncontainers: {}\n",
        "/srv/app/worker-compose.yaml",
    )
    .unwrap();

    assert_eq!(
        resolve_engine_mode(Some(&managed), true, Some("ws://other:1"), None),
        EngineMode::External {
            url: "ws://other:1".to_string()
        }
    );
}

#[test]
fn bare_compose_connects_to_the_file_engine_without_owning_it() {
    let managed = ComposeFile::parse(
        "engine: { url: 'ws://file-engine:49134', workers: {} }\ncontainers: {}\n",
        "/srv/app/worker-compose.yaml",
    )
    .unwrap();

    assert_eq!(
        resolve_engine_mode(Some(&managed), false, None, None),
        EngineMode::External {
            url: "ws://file-engine:49134".to_string()
        }
    );
}

#[test]
fn explicit_engine_beats_the_environment_without_an_engine_section() {
    let external = ComposeFile::parse(
        "containers:\n  api:\n    worker: path://./api\n",
        "/srv/app/worker-compose.yaml",
    )
    .unwrap();

    assert_eq!(
        resolve_engine_mode(
            Some(&external),
            true,
            Some("ws://shared:49134"),
            Some("ws://environment:49134"),
        ),
        EngineMode::External {
            url: "ws://shared:49134".to_string()
        }
    );
}

#[test]
fn compose_without_an_engine_source_uses_the_local_default() {
    let external = ComposeFile::parse(
        "containers:\n  api:\n    worker: path://./api\n",
        "/srv/app/worker-compose.yaml",
    )
    .unwrap();

    assert_eq!(
        resolve_engine_mode(Some(&external), true, None, None),
        EngineMode::External {
            url: "ws://127.0.0.1:49134".to_string()
        }
    );
}

#[test]
fn up_takes_a_file_of_its_own() {
    let ComposeCommand::Serve { file, start, .. } =
        parse(&["iii", "compose", "--up", "-f", "./other.yaml"])
            .plan()
            .unwrap()
    else {
        panic!("expected serve command");
    };
    assert_eq!(file, std::path::Path::new("./other.yaml"));
    assert!(start);
}

#[test]
fn the_namespace_reads_the_same_on_either_side_of_the_up_flag() {
    // Flags can appear in either order, so an operator can put the action
    // before or after its namespace.
    for args in [
        ["iii", "compose", "-n", "orders", "--up"],
        ["iii", "compose", "--up", "-n", "orders"],
    ] {
        let cli = parse(&args);
        assert_eq!(cli.daemon_namespace().unwrap(), "orders");
        let ComposeCommand::Serve {
            explicit_daemon_namespace,
            start,
            ..
        } = cli.plan().unwrap()
        else {
            panic!("expected serve command");
        };
        assert_eq!(explicit_daemon_namespace.as_deref(), Some("orders"));
        assert!(start, "{args:?} should still enable --up");
    }
}
