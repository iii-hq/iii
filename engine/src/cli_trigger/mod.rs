// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

pub mod exec;
pub mod help;
pub mod payload;

use clap::Parser;
use iii::workers::worker::DEFAULT_PORT;

/// Errors surfaced by `iii trigger`.
///
/// `RemoteAlreadyReported` carries no message because `exec::invoke` has
/// already printed a structured JSON error to stderr; main.rs translates
/// this variant into a silent exit-code-1 to avoid double-printing.
#[derive(Debug, thiserror::Error)]
pub enum TriggerCliError {
    #[error("remote function returned an error")]
    RemoteAlreadyReported,
    #[error(transparent)]
    Other(#[from] anyhow::Error),
}

#[derive(Parser, Debug, Clone)]
#[command(disable_help_flag = true)]
pub struct TriggerArgs {
    /// Function path (e.g. `my::fn`, `sandbox::create`). Positional.
    #[arg(value_name = "FUNCTION_PATH")]
    pub function_path: Option<String>,

    /// Key=value payload tokens (`a=10 b="hello world"`).
    /// Combinable with `--json`: kv pairs override individual keys of the json object.
    #[arg(value_name = "KV", num_args = 0..)]
    pub kv: Vec<String>,

    /// JSON payload (`--json '{"a":1}'`). When combined with kv pairs the json must be an object;
    /// kv pairs override its keys (shallow merge).
    #[arg(long)]
    pub json: Option<String>,

    /// Engine WebSocket address (`ws://host:port`). Overrides `III_URL` and
    /// the working directory's compose file. The local default is used when
    /// none of them supplies a URL. Encrypted (`wss://`) connections are not
    /// supported.
    #[arg(long, value_name = "URL")]
    pub engine: Option<String>,

    /// DEPRECATED: use `--engine ws://host:port`. Engine host address. Taken
    /// from `--engine`, `III_URL` or the working directory's compose file when
    /// omitted, else `localhost`.
    #[arg(long)]
    pub address: Option<String>,

    /// DEPRECATED: use `--engine ws://host:port`. Engine WebSocket port. Taken
    /// from `--engine`, `III_URL` or the working directory's compose file when
    /// omitted, else 49134.
    #[arg(long)]
    pub port: Option<u16>,

    /// Max time to wait for the invocation result (milliseconds).
    #[arg(long, default_value_t = 30_000)]
    pub timeout_ms: u64,

    /// Namespace to resolve FUNCTION_PATH in. Omit to resolve in the engine's
    /// `default` namespace; routing is strict, so a function registered in
    /// another namespace is only reachable with this flag.
    #[arg(short = 'n', long, value_name = "NS")]
    pub namespace: Option<String>,

    /// Print help. With a FUNCTION_PATH, queries a running engine for that
    /// function's description and request schema.
    #[arg(short = 'h', long = "help", action = clap::ArgAction::SetTrue)]
    pub help: bool,
}

/// Host used when neither a flag nor `III_URL` names one.
const DEFAULT_ADDRESS: &str = "localhost";

/// Told to the caller when a URL asks for TLS, which no iii engine serves.
const WSS_UNSUPPORTED: &str = "Encrypted websocket connections are not currently supported.";

/// Replaces any `user:password@` in a URL-shaped string with `***@`.
///
/// Applied to every value quoted back in an error. The authority is whatever
/// sits between the scheme and the first `/`, `?` or `#`, which is where the
/// `url` crate reads credentials from, and this runs on strings that never
/// parsed, so it cannot rely on that crate to find them.
fn redact_credentials(raw: &str) -> String {
    let rest_start = match raw.find("://") {
        Some(index) => index + 3,
        None => match raw.find(':') {
            Some(index) => index + 1,
            None => 0,
        },
    };
    let rest = &raw[rest_start..];
    let authority_end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let authority = &rest[..authority_end];
    match authority.rfind('@') {
        Some(at) => format!(
            "{}***@{}{}",
            &raw[..rest_start],
            &authority[at + 1..],
            &rest[authority_end..]
        ),
        None => raw.to_string(),
    }
}

/// Splits a `ws://` engine URL into host and port.
///
/// The accepted shapes are exactly `ws://host` and `ws://host:port`. An engine
/// address is a host and a port and nothing else, and every part this CLI
/// cannot honour is refused rather than dropped: the endpoint is rebuilt as
/// `ws://{host}:{port}` further down, so a path, a query, or credentials would
/// vanish without a word and connect somewhere other than the value says.
/// Silently ignored credentials are the worst of those, because the caller is
/// left believing the connection was authenticated.
///
/// Every unusable value is an error, named by the `source` that carried it. An
/// endpoint that cannot be parsed used to fall back to `localhost:49134`, which
/// sent the call to whichever engine happened to own that port while the
/// operator believed it went where they had pointed it.
///
/// `wss://` is refused with its own message. No iii engine terminates TLS, so
/// accepting one would send a caller who asked for TLS over the wire in the
/// clear.
///
/// A URL without a port keeps `DEFAULT_PORT`: the scheme defaults the `url`
/// crate knows (80 and 443) are not this engine's.
fn engine_url_endpoint(raw: &str, source: &str) -> anyhow::Result<(String, u16)> {
    let raw = raw.trim();
    // Quoted back with any `user:password@` hidden. An error line reaches
    // terminals, CI logs and bug reports, and a value this function rejects is
    // exactly the one nobody checked for a secret.
    let shown = redact_credentials(raw);
    let malformed = |detail: &str| {
        anyhow::anyhow!("{source} {shown:?} {detail}; expected ws://host or ws://host:port")
    };

    // `str::get` rather than a slice: an index into a multi-byte character
    // panics, and this is user input. Checked before parsing, so `wss:` in any
    // form is named for what it is.
    if raw
        .get(..4)
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case("wss:"))
    {
        anyhow::bail!(WSS_UNSUPPORTED);
    }
    if !raw
        .get(..5)
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case("ws://"))
    {
        return Err(malformed("must start with ws://"));
    }

    let url = url::Url::parse(raw).map_err(|_| malformed("is not a valid URL"))?;
    let host = url.host().ok_or_else(|| malformed("names no host"))?;
    if !url.username().is_empty() || url.password().is_some() {
        // Dropping these quietly would leave the caller believing the
        // connection carried them.
        return Err(malformed("must not carry credentials"));
    }
    if !matches!(url.path(), "" | "/") {
        return Err(malformed("must not have a path"));
    }
    if url.query().is_some() {
        return Err(malformed("must not have a query string"));
    }
    if url.fragment().is_some() {
        return Err(malformed("must not have a fragment"));
    }

    let host = match host {
        url::Host::Ipv6(address) => format!("[{address}]"),
        host => host.to_string(),
    };
    Ok((host, url.port().unwrap_or(DEFAULT_PORT)))
}

/// Reads `engine.url` from the compose file in the working directory.
///
/// Returns `None` whenever the file is absent, unreadable, invalid, or
/// declares no engine. Finding the file is a resolution step, not a
/// validation one: `iii trigger` has to keep working outside any project, so
/// a broken compose file must never fail a command that did not ask about it.
/// A file that does state an engine address is held to the same rule as every
/// other source, and an unusable one there fails the call.
fn compose_file_engine_url() -> Option<String> {
    let path = std::path::Path::new(iii_compose::cli::DEFAULT_COMPOSE_FILE);
    let text = std::fs::read_to_string(path).ok()?;
    // Only the engine section, so a container the running binary cannot parse
    // does not cost the caller the address the file plainly states.
    let engine = iii_compose::config::parse_engine_section(&text, path).ok()??;
    (!engine.url.trim().is_empty()).then_some(engine.url)
}

/// Resolves the engine endpoint from the flags and `III_URL`.
///
/// `III_URL` is how every managed context already names its engine: compose
/// exports it to each worker it spawns, and `iii compose --engine` reads it.
/// This CLI used to ignore it, so on a project that does not own port 49134
/// every call silently reached whatever engine did.
///
/// Order is `--engine`, then `III_URL`, then the compose file in the working
/// directory, then `localhost:49134`. The deprecated `--address` and `--port`
/// still win, each over its own half of whichever URL won, so a flag can
/// retarget one component and inherit the other.
///
/// A URL that cannot be used fails the call, whichever source carried it: a
/// `wss://` one with its own message, anything else as malformed. A blank
/// `III_URL` is the exception, read as unset rather than as a URL, so an
/// exported but empty variable still resolves to the default. A blank
/// `--engine` is not: the caller typed the flag, so it fails like any other
/// value that names no engine.
///
/// Takes the environment value as an argument rather than reading it, so the
/// precedence is testable without mutating process state.
fn resolve_endpoint(
    address: Option<&str>,
    port: Option<u16>,
    engine_flag: Option<&str>,
    engine_url: Option<&str>,
    compose_url: Option<&str>,
) -> anyhow::Result<(String, u16)> {
    let named = match engine_flag {
        // Typed by the caller, so even an empty value is a statement: it names
        // no engine, and quietly reading another source would send the call
        // somewhere they did not ask for. Blank values from the other sources
        // mean the opposite, that nothing was set.
        Some(flag) => Some((flag, "--engine")),
        None => [
            (engine_url, "III_URL"),
            (compose_url, "worker-compose.yaml engine.url"),
        ]
        .into_iter()
        .find_map(|(value, source)| {
            value
                .map(str::trim)
                .filter(|url| !url.is_empty())
                .map(|url| (url, source))
        }),
    };
    let (source_host, source_port) = match named {
        Some((url, source)) => {
            let (host, port) = engine_url_endpoint(url, source)?;
            (Some(host), Some(port))
        }
        None => (None, None),
    };

    Ok((
        address
            .map(str::to_string)
            .or(source_host)
            .unwrap_or_else(|| DEFAULT_ADDRESS.to_string()),
        port.or(source_port).unwrap_or(DEFAULT_PORT),
    ))
}

impl TriggerArgs {
    /// The engine this invocation talks to.
    fn endpoint(&self) -> anyhow::Result<(String, u16)> {
        let engine_url = std::env::var("III_URL").ok();
        let compose_url = compose_file_engine_url();
        resolve_endpoint(
            self.address.as_deref(),
            self.port,
            self.engine.as_deref(),
            engine_url.as_deref(),
            compose_url.as_deref(),
        )
    }

    /// Warns once when the call used the superseded flags.
    ///
    /// Written to stderr: `iii trigger` prints the invocation result to
    /// stdout, and a notice about a flag must not end up inside output a
    /// script is parsing.
    fn warn_deprecated_endpoint_flags(&self) {
        let used = match (self.address.is_some(), self.port.is_some()) {
            (true, true) => "--address and --port are",
            (true, false) => "--address is",
            (false, true) => "--port is",
            (false, false) => return,
        };
        eprintln!("warning: {used} deprecated; use `--engine ws://host:port` instead");
    }
}

pub async fn run_trigger(args: &TriggerArgs) -> Result<(), TriggerCliError> {
    args.warn_deprecated_endpoint_flags();
    let (address, port) = args.endpoint()?;
    if args.help {
        help::print(
            args.function_path.as_deref(),
            &address,
            port,
            args.timeout_ms,
            args.namespace.as_deref(),
        )
        .await?;
        return Ok(());
    }
    let function_path = args.function_path.as_deref().ok_or_else(|| {
        anyhow::anyhow!("iii trigger: missing FUNCTION_PATH. Try: `iii trigger <fn-path> [args]`")
    })?;
    // `compose::add` has a list in its JSON contract, while the shell form
    // repeats the readable singular key: `worker=database worker=web`.
    // Keep this adaptation scoped to that function so repeated keys for every
    // other trigger retain their established last-value-wins behaviour.
    let payload = if function_path == "compose::add" {
        payload::parse_collecting(&args.kv, args.json.as_deref(), "worker", "workers")?
    } else {
        payload::parse(&args.kv, args.json.as_deref())?
    };
    exec::invoke(
        function_path,
        payload,
        &address,
        port,
        args.timeout_ms,
        args.namespace.as_deref(),
    )
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn endpoint_defaults_when_nothing_is_set() {
        assert_eq!(
            resolve_endpoint(None, None, None, None, None).unwrap(),
            ("localhost".to_string(), DEFAULT_PORT)
        );
    }

    #[test]
    fn endpoint_reads_engine_url() {
        assert_eq!(
            resolve_endpoint(None, None, None, Some("ws://127.0.0.1:49734"), None).unwrap(),
            ("127.0.0.1".to_string(), 49734)
        );
    }

    #[test]
    fn flags_win_over_engine_url() {
        assert_eq!(
            resolve_endpoint(
                Some("example.test"),
                Some(1234),
                None,
                Some("ws://127.0.0.1:49734"),
                None
            )
            .unwrap(),
            ("example.test".to_string(), 1234)
        );
    }

    #[test]
    fn each_flag_wins_over_its_own_half() {
        assert_eq!(
            resolve_endpoint(None, Some(1234), None, Some("ws://127.0.0.1:49734"), None).unwrap(),
            ("127.0.0.1".to_string(), 1234)
        );
        assert_eq!(
            resolve_endpoint(
                Some("example.test"),
                None,
                None,
                Some("ws://127.0.0.1:49734"),
                None
            )
            .unwrap(),
            ("example.test".to_string(), 49734)
        );
    }

    #[test]
    fn a_blank_engine_url_is_treated_as_unset() {
        for url in ["", "   "] {
            assert_eq!(
                resolve_endpoint(None, None, None, Some(url), None).unwrap(),
                ("localhost".to_string(), DEFAULT_PORT),
                "unexpected endpoint for III_URL {url:?}"
            );
        }
    }

    #[test]
    fn an_unusable_engine_url_errors_instead_of_falling_back() {
        // Falling back sent the call to whichever engine owned 49134 while
        // the operator believed III_URL had pointed it somewhere else.
        for url in ["not a url", "http://127.0.0.1:49734", "ws://"] {
            let err = resolve_endpoint(None, None, None, Some(url), None)
                .unwrap_err()
                .to_string();
            assert!(
                err.starts_with("III_URL") && err.contains("expected ws://host or ws://host:port"),
                "unexpected error for III_URL {url:?}: {err}"
            );
        }
    }

    #[test]
    fn an_unusable_engine_url_errors_even_when_both_flags_supply_the_endpoint() {
        let err = resolve_endpoint(
            Some("example.test"),
            Some(1234),
            None,
            Some("not a url"),
            None,
        )
        .unwrap_err()
        .to_string();
        assert!(
            err.contains("expected ws://host or ws://host:port"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn engine_url_without_a_port_keeps_the_engine_default() {
        assert_eq!(
            resolve_endpoint(None, None, None, Some("ws://engine.test"), None).unwrap(),
            ("engine.test".to_string(), DEFAULT_PORT)
        );
    }

    #[test]
    fn engine_url_keeps_ipv6_brackets() {
        assert_eq!(
            resolve_endpoint(None, None, None, Some("ws://[::1]:49734"), None).unwrap(),
            ("[::1]".to_string(), 49734)
        );
    }

    #[test]
    fn the_engine_flag_wins_over_the_environment() {
        assert_eq!(
            resolve_endpoint(
                None,
                None,
                Some("ws://flag.test:4300"),
                Some("ws://127.0.0.1:49734"),
                None
            )
            .unwrap(),
            ("flag.test".to_string(), 4300)
        );
    }

    #[test]
    fn the_deprecated_flags_still_win_over_the_engine_flag() {
        assert_eq!(
            resolve_endpoint(None, Some(1234), Some("ws://flag.test:4300"), None, None).unwrap(),
            ("flag.test".to_string(), 1234)
        );
    }

    #[test]
    fn a_wss_environment_url_is_refused_rather_than_downgraded() {
        // Falling back would send the payload in the clear to an engine the
        // caller did not name.
        let err = resolve_endpoint(None, None, None, Some("wss://engine.test:443"), None)
            .expect_err("wss must not resolve");
        assert_eq!(err.to_string(), WSS_UNSUPPORTED);
    }

    #[test]
    fn a_wss_engine_flag_is_refused() {
        let err = resolve_endpoint(None, None, Some("wss://engine.test:443"), None, None)
            .expect_err("wss must not resolve");
        assert_eq!(err.to_string(), WSS_UNSUPPORTED);
    }

    #[test]
    fn a_wss_url_is_refused_even_when_both_flags_supply_the_endpoint() {
        // The address never reaches the socket here, but the caller still
        // asked for TLS and must not be told the call was fine.
        let err = resolve_endpoint(
            Some("example.test"),
            Some(1234),
            None,
            Some("wss://engine.test:443"),
            None,
        )
        .expect_err("wss must not resolve");
        assert_eq!(err.to_string(), WSS_UNSUPPORTED);
    }

    #[test]
    fn an_unusable_engine_flag_errors_instead_of_falling_back() {
        // The caller typed this one, so silence would send the call to
        // localhost while the operator believes it went somewhere else.
        for url in ["", "   ", "not a url", "http://127.0.0.1:49734", "ws://"] {
            let err = resolve_endpoint(None, None, Some(url), None, None)
                .unwrap_err()
                .to_string();
            assert!(
                err.starts_with("--engine") && err.contains("expected ws://host or ws://host:port"),
                "unexpected error for --engine {url:?}: {err}"
            );
        }
    }

    #[test]
    fn compose_file_is_used_when_no_other_source_names_an_engine() {
        assert_eq!(
            resolve_endpoint(None, None, None, None, Some("ws://127.0.0.1:49934")).unwrap(),
            ("127.0.0.1".to_string(), 49934)
        );
    }

    #[test]
    fn engine_url_wins_over_the_compose_file() {
        // An exported `III_URL` is the caller's live intent; the file is only
        // what the directory happens to hold.
        assert_eq!(
            resolve_endpoint(
                None,
                None,
                None,
                Some("ws://127.0.0.1:49134"),
                Some("ws://127.0.0.1:49934")
            )
            .unwrap(),
            ("127.0.0.1".to_string(), 49134)
        );
    }

    #[test]
    fn the_engine_flag_wins_over_the_compose_file() {
        assert_eq!(
            resolve_endpoint(
                None,
                None,
                Some("ws://flag.test:4300"),
                None,
                Some("ws://127.0.0.1:49934")
            )
            .unwrap(),
            ("flag.test".to_string(), 4300)
        );
    }

    #[test]
    fn each_deprecated_flag_wins_over_its_own_half_of_the_compose_file() {
        assert_eq!(
            resolve_endpoint(None, Some(1234), None, None, Some("ws://127.0.0.1:49934")).unwrap(),
            ("127.0.0.1".to_string(), 1234)
        );
    }

    #[test]
    fn an_unusable_compose_url_errors_and_names_the_file() {
        // The file is the last source, so nothing is left to fall back to but
        // the local default, which is exactly the wrong engine to reach.
        let err = resolve_endpoint(None, None, None, None, Some("not a url"))
            .unwrap_err()
            .to_string();
        assert!(
            err.starts_with("worker-compose.yaml engine.url"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn a_wss_compose_url_is_refused() {
        let err = resolve_endpoint(None, None, None, None, Some("wss://engine.test:443"))
            .unwrap_err()
            .to_string();
        assert_eq!(err, WSS_UNSUPPORTED);
    }

    #[test]
    fn a_blank_compose_url_is_treated_as_unset() {
        assert_eq!(
            resolve_endpoint(None, None, None, None, Some("   ")).unwrap(),
            ("localhost".to_string(), DEFAULT_PORT)
        );
    }

    #[test]
    fn a_blank_engine_flag_does_not_fall_through_to_the_environment() {
        let err = resolve_endpoint(None, None, Some("  "), Some("ws://127.0.0.1:49734"), None)
            .unwrap_err()
            .to_string();
        assert!(err.starts_with("--engine"), "unexpected error: {err}");
    }

    #[test]
    fn a_url_with_more_than_a_host_and_port_is_refused() {
        // Each of these parses, and every part past the authority would be
        // dropped when the endpoint is rebuilt as `ws://host:port`.
        for (url, detail) in [
            ("ws://127.0.0.1:4400/ws", "must not have a path"),
            ("ws://127.0.0.1:4400/a/b", "must not have a path"),
            ("ws://127.0.0.1:4400?x=1", "must not have a query string"),
            ("ws://127.0.0.1:4400#frag", "must not have a fragment"),
            ("ws://user@127.0.0.1:4400", "must not carry credentials"),
            ("ws:127.0.0.1:4400", "must start with ws://"),
        ] {
            let err = resolve_endpoint(None, None, Some(url), None, None)
                .unwrap_err()
                .to_string();
            assert!(err.contains(detail), "unexpected error for {url:?}: {err}");
        }
    }

    #[test]
    fn a_bare_host_and_a_trailing_slash_are_both_accepted() {
        // `ws://host` and `ws://host:port/` are the same address; the `url`
        // crate normalises both paths to "/".
        for url in ["ws://engine.test:4400", "ws://engine.test:4400/"] {
            assert_eq!(
                resolve_endpoint(None, None, Some(url), None, None).unwrap(),
                ("engine.test".to_string(), 4400),
                "unexpected endpoint for {url:?}"
            );
        }
    }

    #[test]
    fn a_wss_url_is_named_as_tls_whatever_its_shape() {
        for url in [
            "wss://engine.test:443",
            "wss://engine.test/ws",
            "WSS://engine.test",
        ] {
            let err = resolve_endpoint(None, None, Some(url), None, None)
                .unwrap_err()
                .to_string();
            assert_eq!(err, WSS_UNSUPPORTED, "unexpected error for {url:?}");
        }
    }

    #[test]
    fn credentials_never_reach_the_error_text() {
        // An error line reaches terminals, CI logs and bug reports, and the
        // value this rejects is the one nobody checked for a secret. Every
        // rejection path has to hide them, not just the one that names them:
        // a wrong scheme is reported before the URL is ever parsed.
        for url in [
            "ws://user:secret@127.0.0.1:4400",
            "http://user:secret@127.0.0.1:4400",
            "ws://user:secret@127.0.0.1:4400/path",
            "wss://user:secret@127.0.0.1:4400",
            "gibberish://user:secret@host",
        ] {
            let err = resolve_endpoint(None, None, Some(url), None, None)
                .unwrap_err()
                .to_string();
            assert!(
                !err.contains("secret"),
                "password leaked for {url:?}: {err}"
            );
            assert!(!err.contains("user"), "username leaked for {url:?}: {err}");
        }
    }

    #[test]
    fn the_redacted_value_keeps_the_part_that_helps() {
        let err = resolve_endpoint(
            None,
            None,
            Some("http://user:secret@127.0.0.1:4400"),
            None,
            None,
        )
        .unwrap_err()
        .to_string();
        assert!(
            err.contains("***@127.0.0.1:4400"),
            "host and port should survive redaction: {err}"
        );
    }

    #[test]
    fn a_non_ascii_value_is_rejected_rather_than_panicking() {
        // `raw[..4]` panics when the index lands inside a character.
        for url in ["wssé", "ws:é", "wsé", "wss√", "é", "ws"] {
            let err = resolve_endpoint(None, None, Some(url), None, None)
                .unwrap_err()
                .to_string();
            assert!(
                err.contains("expected ws://host or ws://host:port"),
                "unexpected error for {url:?}: {err}"
            );
        }
    }

    #[tokio::test]
    async fn run_trigger_missing_fn_path_errors() {
        let args = TriggerArgs {
            function_path: None,
            kv: vec![],
            json: None,
            engine: None,
            address: None,
            port: None,
            timeout_ms: 800,
            help: false,
            namespace: None,
        };
        let err = run_trigger(&args).await.unwrap_err().to_string();
        assert!(
            err.contains("missing FUNCTION_PATH"),
            "expected missing fn-path error, got: {}",
            err,
        );
    }

    #[tokio::test]
    async fn run_trigger_unreachable_engine_times_out() {
        let args = TriggerArgs {
            function_path: Some("test::fn".to_string()),
            kv: vec![],
            json: None,
            engine: None,
            address: None,
            port: Some(19999),
            timeout_ms: 800,
            help: false,
            namespace: None,
        };
        let err = run_trigger(&args).await.unwrap_err().to_string();
        // The OS may immediately return ECONNREFUSED on closed ports, in which
        // case the application-level timeout never fires. Accept either the
        // application timeout message OR the underlying connection error.
        assert!(
            err.contains("Timed out")
                || err.contains("timeout")
                || err.contains("Connection refused")
                || err.contains("connect")
                || err.contains("WebSocket"),
            "expected timeout or connection error when engine is unreachable, got: {}",
            err,
        );
    }

    #[tokio::test]
    async fn run_trigger_rejects_invalid_json() {
        let args = TriggerArgs {
            function_path: Some("test::fn".to_string()),
            kv: vec![],
            json: Some("not-json".to_string()),
            engine: None,
            address: None,
            port: None,
            timeout_ms: 30_000,
            help: false,
            namespace: None,
        };
        let err = run_trigger(&args).await.unwrap_err().to_string();
        assert!(
            err.contains("--json: invalid JSON"),
            "expected json validation error, got: {}",
            err,
        );
    }
}
