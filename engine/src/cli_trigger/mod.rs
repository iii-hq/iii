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

    /// Engine host address. Taken from the working directory's compose file
    /// or `III_URL` when omitted, else `localhost`.
    #[arg(long)]
    pub address: Option<String>,

    /// Engine WebSocket port. Taken from the working directory's compose file
    /// or `III_URL` when omitted, else 49134.
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

/// Splits a `ws://` or `wss://` engine URL into host and port.
///
/// Returns `None` for anything that is not one of those, so a stale or
/// malformed `III_URL` falls back to the defaults instead of failing a command
/// that never asked about it. A URL without a port keeps `DEFAULT_PORT`: the
/// scheme defaults the `url` crate knows (80 and 443) are not this engine's.
fn engine_url_endpoint(raw: &str) -> Option<(String, u16)> {
    let url = url::Url::parse(raw.trim()).ok()?;
    if !matches!(url.scheme(), "ws" | "wss") {
        return None;
    }
    let host = match url.host()? {
        url::Host::Ipv6(address) => format!("[{address}]"),
        host => host.to_string(),
    };
    Some((host, url.port().unwrap_or(DEFAULT_PORT)))
}

/// Reads `engine.url` from the compose file in the working directory.
///
/// Returns `None` whenever the file is absent, unreadable, invalid, or
/// declares no engine. This is only a resolution step: `iii trigger` has to
/// keep working outside any project, so a broken compose file must never fail
/// a command that did not ask about it.
fn compose_file_engine_url() -> Option<String> {
    let path = std::path::Path::new(iii_compose::cli::DEFAULT_COMPOSE_FILE);
    let text = std::fs::read_to_string(path).ok()?;
    // Only the engine section, so a container the running binary cannot parse
    // does not cost the caller the address the file plainly states.
    let engine = iii_compose::config::parse_engine_section(&text, path).ok()??;
    (!engine.url.trim().is_empty()).then_some(engine.url)
}

/// Resolves the engine endpoint from the flags, the compose file, and
/// `III_URL`.
///
/// Order, matching `iii compose`: the flag, then the compose file in the
/// working directory, then `III_URL`, then `localhost:49134`. `iii compose`
/// resolves `--engine`, then the file, then `III_URL` (see
/// `iii_compose::resolve_engine_mode`), so the file is ahead of the
/// environment here for the same reason: a project directory states which
/// engine it owns, and a leftover variable in the operator's shell should not
/// beat it.
///
/// `--address` and `--port` win over whichever source supplied the URL, each
/// over its own half, so a flag can retarget one component and inherit the
/// other.
///
/// Takes both outside values as arguments rather than reading them, so the
/// precedence is testable without touching the filesystem or process state.
fn resolve_endpoint(
    address: Option<&str>,
    port: Option<u16>,
    compose_url: Option<&str>,
    engine_url: Option<&str>,
) -> (String, u16) {
    let from_source = [compose_url, engine_url]
        .into_iter()
        .flatten()
        .map(str::trim)
        .filter(|url| !url.is_empty())
        .find_map(engine_url_endpoint);
    let (source_host, source_port) = match from_source {
        Some((host, port)) => (Some(host), Some(port)),
        None => (None, None),
    };

    (
        address
            .map(str::to_string)
            .or(source_host)
            .unwrap_or_else(|| DEFAULT_ADDRESS.to_string()),
        port.or(source_port).unwrap_or(DEFAULT_PORT),
    )
}

impl TriggerArgs {
    /// The engine this invocation talks to.
    fn endpoint(&self) -> (String, u16) {
        let compose_url = compose_file_engine_url();
        let engine_url = std::env::var("III_URL").ok();
        resolve_endpoint(
            self.address.as_deref(),
            self.port,
            compose_url.as_deref(),
            engine_url.as_deref(),
        )
    }
}

pub async fn run_trigger(args: &TriggerArgs) -> Result<(), TriggerCliError> {
    let (address, port) = args.endpoint();
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
    fn compose_file_is_used_when_no_other_source_names_an_engine() {
        assert_eq!(
            resolve_endpoint(None, None, Some("ws://127.0.0.1:49934"), None),
            ("127.0.0.1".to_string(), 49934)
        );
    }

    #[test]
    fn compose_file_wins_over_engine_url() {
        // `iii compose` resolves the file ahead of `III_URL`; a leftover
        // variable in the operator's shell must not beat the project.
        assert_eq!(
            resolve_endpoint(
                None,
                None,
                Some("ws://127.0.0.1:49934"),
                Some("ws://127.0.0.1:49134")
            ),
            ("127.0.0.1".to_string(), 49934)
        );
    }

    #[test]
    fn flags_win_over_the_compose_file() {
        assert_eq!(
            resolve_endpoint(
                Some("example.test"),
                Some(1234),
                Some("ws://127.0.0.1:49934"),
                None
            ),
            ("example.test".to_string(), 1234)
        );
    }

    #[test]
    fn each_flag_wins_over_its_own_half_of_the_compose_file() {
        assert_eq!(
            resolve_endpoint(None, Some(1234), Some("ws://127.0.0.1:49934"), None),
            ("127.0.0.1".to_string(), 1234)
        );
    }

    #[test]
    fn unusable_compose_url_falls_through_to_engine_url() {
        for compose in ["", "   ", "not a url", "http://127.0.0.1:49934"] {
            assert_eq!(
                resolve_endpoint(None, None, Some(compose), Some("ws://127.0.0.1:49134")),
                ("127.0.0.1".to_string(), 49134),
                "unexpected endpoint for compose url {compose:?}"
            );
        }
    }

    #[test]
    fn unusable_compose_url_and_no_engine_url_falls_back_to_defaults() {
        assert_eq!(
            resolve_endpoint(None, None, Some("not a url"), None),
            ("localhost".to_string(), DEFAULT_PORT)
        );
    }

    #[test]
    fn endpoint_defaults_when_nothing_is_set() {
        assert_eq!(
            resolve_endpoint(None, None, None, None),
            ("localhost".to_string(), DEFAULT_PORT)
        );
    }

    #[test]
    fn endpoint_reads_engine_url() {
        assert_eq!(
            resolve_endpoint(None, None, None, Some("ws://127.0.0.1:49734")),
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
            ),
            ("example.test".to_string(), 1234)
        );
    }

    #[test]
    fn each_flag_wins_over_its_own_half() {
        assert_eq!(
            resolve_endpoint(None, Some(1234), None, Some("ws://127.0.0.1:49734")),
            ("127.0.0.1".to_string(), 1234)
        );
        assert_eq!(
            resolve_endpoint(
                Some("example.test"),
                None,
                None,
                Some("ws://127.0.0.1:49734")
            ),
            ("example.test".to_string(), 49734)
        );
    }

    #[test]
    fn unusable_engine_url_falls_back_to_defaults() {
        for url in ["", "   ", "not a url", "http://127.0.0.1:49734", "ws://"] {
            assert_eq!(
                resolve_endpoint(None, None, None, Some(url)),
                ("localhost".to_string(), DEFAULT_PORT),
                "unexpected endpoint for III_URL {url:?}"
            );
        }
    }

    #[test]
    fn engine_url_without_a_port_keeps_the_engine_default() {
        assert_eq!(
            resolve_endpoint(None, None, None, Some("ws://engine.test")),
            ("engine.test".to_string(), DEFAULT_PORT)
        );
    }

    #[test]
    fn engine_url_keeps_ipv6_brackets() {
        assert_eq!(
            resolve_endpoint(None, None, None, Some("ws://[::1]:49734")),
            ("[::1]".to_string(), 49734)
        );
    }

    #[tokio::test]
    async fn run_trigger_missing_fn_path_errors() {
        let args = TriggerArgs {
            function_path: None,
            kv: vec![],
            json: None,
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
