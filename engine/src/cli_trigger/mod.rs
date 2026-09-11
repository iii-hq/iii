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

    /// Engine WebSocket address (`ws://host:port`). Overrides `III_URL`. The
    /// local default is used when neither supplies a URL. Encrypted
    /// (`wss://`) connections are not supported.
    #[arg(long, value_name = "URL")]
    pub engine: Option<String>,

    /// DEPRECATED: use `--engine ws://host:port`. Engine host address. Taken
    /// from `--engine` or `III_URL` when omitted, else `localhost`.
    #[arg(long)]
    pub address: Option<String>,

    /// DEPRECATED: use `--engine ws://host:port`. Engine WebSocket port. Taken
    /// from `--engine` or `III_URL` when omitted, else 49134.
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

/// Splits a `ws://` engine URL into host and port.
///
/// Every unusable value is an error, named by the `source` that carried it.
/// An endpoint that cannot be parsed used to fall back to `localhost:49134`,
/// which sent the call to whichever engine happened to own that port while the
/// operator believed it went where they had pointed it. That silence is the
/// failure this command already had; a stale value is better reported than
/// worked around.
///
/// `wss://` is refused with its own message. No iii engine terminates TLS, and
/// the address is rebuilt as `ws://` further down, so accepting one would send
/// a caller who asked for TLS over the wire in the clear.
///
/// A URL without a port keeps `DEFAULT_PORT`: the scheme defaults the `url`
/// crate knows (80 and 443) are not this engine's.
fn engine_url_endpoint(raw: &str, source: &str) -> anyhow::Result<(String, u16)> {
    let raw = raw.trim();
    let malformed = || {
        anyhow::anyhow!(
            "{source} {raw:?} must be a ws:// URL with a host, e.g. ws://localhost:49134"
        )
    };
    let url = url::Url::parse(raw).map_err(|_| malformed())?;
    if url.scheme() == "wss" {
        anyhow::bail!(WSS_UNSUPPORTED);
    }
    if url.scheme() != "ws" {
        return Err(malformed());
    }
    let host = match url.host().ok_or_else(malformed)? {
        url::Host::Ipv6(address) => format!("[{address}]"),
        host => host.to_string(),
    };
    Ok((host, url.port().unwrap_or(DEFAULT_PORT)))
}

/// Resolves the engine endpoint from the flags and `III_URL`.
///
/// `III_URL` is how every managed context already names its engine: compose
/// exports it to each worker it spawns, and `iii compose --engine` reads it.
/// This CLI used to ignore it, so on a project that does not own port 49134
/// every call silently reached whatever engine did.
///
/// Order is `--engine`, then `III_URL`, then `localhost:49134`. The deprecated
/// `--address` and `--port` still win, each over its own half of whichever URL
/// won, so a flag can retarget one component and inherit the other.
///
/// A URL that cannot be used fails the call, whichever source carried it: a
/// `wss://` one with its own message, anything else as malformed. An empty or
/// blank value is not a URL at all and is skipped, so an exported but unset
/// `III_URL` still resolves to the default.
///
/// Takes the environment value as an argument rather than reading it, so the
/// precedence is testable without mutating process state.
fn resolve_endpoint(
    address: Option<&str>,
    port: Option<u16>,
    engine_flag: Option<&str>,
    engine_url: Option<&str>,
) -> anyhow::Result<(String, u16)> {
    let named = [(engine_flag, "--engine"), (engine_url, "III_URL")]
        .into_iter()
        .find_map(|(value, source)| {
            value
                .map(str::trim)
                .filter(|url| !url.is_empty())
                .map(|url| (url, source))
        });
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
        resolve_endpoint(
            self.address.as_deref(),
            self.port,
            self.engine.as_deref(),
            engine_url.as_deref(),
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
            resolve_endpoint(None, None, None, None).unwrap(),
            ("localhost".to_string(), DEFAULT_PORT)
        );
    }

    #[test]
    fn endpoint_reads_engine_url() {
        assert_eq!(
            resolve_endpoint(None, None, None, Some("ws://127.0.0.1:49734")).unwrap(),
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
                Some("ws://127.0.0.1:49734")
            )
            .unwrap(),
            ("example.test".to_string(), 1234)
        );
    }

    #[test]
    fn each_flag_wins_over_its_own_half() {
        assert_eq!(
            resolve_endpoint(None, Some(1234), None, Some("ws://127.0.0.1:49734")).unwrap(),
            ("127.0.0.1".to_string(), 1234)
        );
        assert_eq!(
            resolve_endpoint(
                Some("example.test"),
                None,
                None,
                Some("ws://127.0.0.1:49734")
            )
            .unwrap(),
            ("example.test".to_string(), 49734)
        );
    }

    #[test]
    fn a_blank_engine_url_is_treated_as_unset() {
        for url in ["", "   "] {
            assert_eq!(
                resolve_endpoint(None, None, None, Some(url)).unwrap(),
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
            let err = resolve_endpoint(None, None, None, Some(url))
                .unwrap_err()
                .to_string();
            assert!(
                err.starts_with("III_URL") && err.contains("must be a ws:// URL"),
                "unexpected error for III_URL {url:?}: {err}"
            );
        }
    }

    #[test]
    fn an_unusable_engine_url_errors_even_when_both_flags_supply_the_endpoint() {
        let err = resolve_endpoint(Some("example.test"), Some(1234), None, Some("not a url"))
            .unwrap_err()
            .to_string();
        assert!(
            err.contains("must be a ws:// URL"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn engine_url_without_a_port_keeps_the_engine_default() {
        assert_eq!(
            resolve_endpoint(None, None, None, Some("ws://engine.test")).unwrap(),
            ("engine.test".to_string(), DEFAULT_PORT)
        );
    }

    #[test]
    fn engine_url_keeps_ipv6_brackets() {
        assert_eq!(
            resolve_endpoint(None, None, None, Some("ws://[::1]:49734")).unwrap(),
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
                Some("ws://127.0.0.1:49734")
            )
            .unwrap(),
            ("flag.test".to_string(), 4300)
        );
    }

    #[test]
    fn the_deprecated_flags_still_win_over_the_engine_flag() {
        assert_eq!(
            resolve_endpoint(None, Some(1234), Some("ws://flag.test:4300"), None).unwrap(),
            ("flag.test".to_string(), 1234)
        );
    }

    #[test]
    fn a_wss_environment_url_is_refused_rather_than_downgraded() {
        // Falling back would send the payload in the clear to an engine the
        // caller did not name.
        let err = resolve_endpoint(None, None, None, Some("wss://engine.test:443"))
            .expect_err("wss must not resolve");
        assert_eq!(err.to_string(), WSS_UNSUPPORTED);
    }

    #[test]
    fn a_wss_engine_flag_is_refused() {
        let err = resolve_endpoint(None, None, Some("wss://engine.test:443"), None)
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
        )
        .expect_err("wss must not resolve");
        assert_eq!(err.to_string(), WSS_UNSUPPORTED);
    }

    #[test]
    fn an_unusable_engine_flag_errors_instead_of_falling_back() {
        // The caller typed this one, so silence would send the call to
        // localhost while the operator believes it went somewhere else.
        for url in ["not a url", "http://127.0.0.1:49734", "ws://"] {
            let err = resolve_endpoint(None, None, Some(url), None)
                .unwrap_err()
                .to_string();
            assert!(
                err.starts_with("--engine") && err.contains("must be a ws:// URL"),
                "unexpected error for --engine {url:?}: {err}"
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
