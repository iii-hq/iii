// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

//! Registry requests that may carry a token.
//!
//! `Authorization: Bearer` goes only to the origin (scheme, host and port) of the registry the
//! token belongs to, and only over https or loopback http. Every redirect is followed here, one
//! hop at a time, rather than by reqwest: each hop decides again whether its own origin may see
//! the token. A private artefact answers `302` to a presigned URL on another host, which gets the
//! request without it. A URL taken from a lock is untrusted in the same way: it is only ever
//! matched against the registry's origin, never trusted for its own host.

use reqwest::{
    Method, StatusCode,
    header::{AUTHORIZATION, HeaderValue, LOCATION},
};

use crate::{
    credentials::{
        Credentials, CredentialsError, Recovery, RegistryOrigin, RegistryToken, TOKEN_ENV,
        TokenSource,
    },
    registry::DEFAULT_REGISTRY,
};

/// The most redirects one request follows, as reqwest's own default policy.
pub(crate) const MAX_REDIRECTS: usize = 10;

/// The token one registry may receive, and the only origin it may be sent to.
#[derive(Debug, Default)]
pub(crate) struct RegistryAuth {
    /// Where the registry is reached: the only origin a request carries the token to.
    endpoint: Option<RegistryOrigin>,
    token: Option<(RegistryToken, TokenSource)>,
}

impl RegistryAuth {
    /// The token `credentials` hold for `registry`, sent only to `registry` itself.
    #[cfg(test)]
    pub(crate) fn for_registry(credentials: &Credentials, registry: &str) -> Self {
        Self::new(credentials, registry, registry).unwrap()
    }

    /// The token `credentials` hold for `registry`, sent only to the origin of `endpoint`.
    ///
    /// The two are the same URL outside tests; a test reaches the registry a reference names
    /// through a local mock server, which is where the token then has to go. An error when the
    /// source that decides the token for `registry` cannot be used: see
    /// [`Credentials::token_for`].
    pub(crate) fn new(
        credentials: &Credentials,
        registry: &str,
        endpoint: &str,
    ) -> Result<Self, CredentialsError> {
        let endpoint = url::Url::parse(endpoint)
            .ok()
            .and_then(|url| RegistryOrigin::of(&url));
        let token = match RegistryOrigin::parse(registry) {
            Ok(origin) => credentials
                .token_for(&origin)?
                .map(|(token, source)| (token.clone(), source)),
            Err(_) => None,
        };
        Ok(Self { endpoint, token })
    }

    /// `token`, sent only to `endpoint`: a token being checked before it is saved.
    pub(crate) fn with_token(
        endpoint: &RegistryOrigin,
        token: RegistryToken,
        source: TokenSource,
    ) -> Self {
        Self {
            endpoint: Some(endpoint.clone()),
            token: Some((token, source)),
        }
    }

    /// The token requests to the registry carry, and where it came from.
    pub(crate) fn sent(&self) -> Option<(&RegistryToken, &TokenSource)> {
        self.token.as_ref().map(|(token, source)| (token, source))
    }

    /// The token a request to `url` carries: only to the registry's own origin, and only over a
    /// transport a token may use.
    pub(crate) fn token_for(&self, url: &url::Url) -> Option<&RegistryToken> {
        let (token, _) = self.token.as_ref()?;
        (self.is_registry(url) && self.endpoint.as_ref()?.allows_tokens()).then_some(token)
    }

    /// Whether `url` is on the registry's own origin, so its answer is the registry's.
    pub(crate) fn is_registry(&self, url: &url::Url) -> bool {
        self.endpoint.is_some() && RegistryOrigin::of(url).as_ref() == self.endpoint.as_ref()
    }

    /// Whether `text` holds the token.
    pub(crate) fn echoes_token(&self, text: &str) -> bool {
        self.token
            .as_ref()
            .is_some_and(|(token, _)| text.contains(token.expose_secret()))
    }

    /// `text` with the token, should it hold it, shown only by its prefix.
    pub(crate) fn scrub(&self, text: &str) -> String {
        match &self.token {
            Some((token, _)) => text.replace(token.expose_secret(), &token.redacted()),
            None => text.to_string(),
        }
    }

    /// `text` from a registry answer, made safe for an error message: the token, should the
    /// answer echo it, shows only its prefix; control characters become `?`; and it is cut at
    /// [`MAX_UNTRUSTED_TEXT`] characters.
    pub(crate) fn untrusted(&self, text: &str) -> String {
        let text = self.scrub(text);
        let mut safe: String = text
            .chars()
            .map(|c| if c.is_control() { '?' } else { c })
            .take(MAX_UNTRUSTED_TEXT)
            .collect();
        if text.chars().count() > MAX_UNTRUSTED_TEXT {
            safe.push('…');
        }
        safe
    }
}

/// The most characters of registry text an error message quotes.
const MAX_UNTRUSTED_TEXT: usize = 300;

/// The registry's code and message from a `401` body. `/resolve` answers
/// `{error: {code, message, …}}`; the other endpoints `{error, code}`. `unauthorized` when it
/// names no code, or one that is not a plain identifier or echoes the token. The message is
/// untrusted text for an error: see [`RegistryAuth::untrusted`].
pub(crate) fn auth_refusal(body: &str, auth: &RegistryAuth) -> (String, Option<String>) {
    let parsed = serde_json::from_str::<serde_json::Value>(body).unwrap_or_default();
    let text = |value: Option<&serde_json::Value>| {
        value
            .and_then(serde_json::Value::as_str)
            .map(str::trim)
            .filter(|text| !text.is_empty())
            .map(str::to_string)
    };
    let error = parsed.get("error");
    let nested = error.filter(|error| error.is_object());
    let code = text(nested.and_then(|error| error.get("code")))
        .or_else(|| text(parsed.get("code")))
        .filter(|code| {
            code.len() <= 64
                && code
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
                && !auth.echoes_token(code)
        })
        .unwrap_or_else(|| "unauthorized".to_string());
    let message = text(nested.and_then(|error| error.get("message")))
        .or_else(|| text(error))
        .map(|message| auth.untrusted(&message));
    (code, message)
}

/// `registry` as its origin, for messages.
fn origin_text(registry: &str) -> String {
    RegistryOrigin::parse(registry)
        .map(|origin| origin.as_str().to_string())
        .unwrap_or_else(|_| registry.to_string())
}

fn is_default_registry(registry: &str) -> bool {
    RegistryOrigin::parse(registry).is_ok_and(|origin| origin == RegistryOrigin::default_registry())
}

/// `iii registry <subcommand>` for `registry`: `--registry` only when it is not the default.
pub(crate) fn registry_command(subcommand: &str, registry: &str) -> String {
    if is_default_registry(registry) {
        format!("iii registry {subcommand}")
    } else {
        format!(
            "iii registry {subcommand} --registry {}",
            origin_text(registry)
        )
    }
}

/// Who reads a message about a token. It decides where `III_REGISTRY_TOKEN` is read, what the
/// failure stops, and how to fix it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Reader {
    /// A Compose operation, in the Compose process (the `iii compose` daemon, or
    /// `iii compose build`): its own environment and file.
    Compose,
    /// `iii registry login`, checking a token before it saves it.
    Login,
    /// `iii registry whoami`, in a shell whose environment may differ from Compose's.
    Whoami,
}

/// A failure explained: what happened, from which source and with which consequence, then how
/// to fix it. Compose joins the two into one message; the CLI prints `fix` as its hint.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Explained {
    pub what: String,
    pub fix: String,
}

impl Explained {
    pub(crate) fn sentence(&self) -> String {
        format!("{} {}", self.what, self.fix)
    }
}

/// Where Compose reads `III_REGISTRY_TOKEN`: in its own process, which is the Compose daemon
/// (`iii compose`, with or without `--up`) for the `compose::*` functions and `iii compose build`
/// itself for a build. Never the engine: the daemon may run beside an engine started on its own,
/// and the engine it starts with `--up` does not even inherit the variable.
pub(crate) const WHERE_COMPOSE_RUNS: &str = "where Compose runs (on the `iii compose` daemon \
                                             process, then restarted, or in the shell that runs \
                                             `iii compose build`)";

/// "the token wdt_AbCd… from III_REGISTRY_TOKEN": the token by its prefix and where it came from,
/// never the token.
pub(crate) fn token_from(token: &RegistryToken, source: &TokenSource, reader: Reader) -> String {
    let token = token.redacted();
    match (source, reader) {
        (TokenSource::Environment, Reader::Compose) => {
            format!("the token {token} from {TOKEN_ENV}")
        }
        (TokenSource::Environment, _) => {
            format!("the token {token} from {TOKEN_ENV} in this shell")
        }
        (TokenSource::File(path), _) => format!("the token {token} saved in {}", path.display()),
        (TokenSource::Given, _) => format!("the token {token} given to `iii registry login`"),
    }
}

/// Where to get a new token for `registry`.
fn new_token(registry: &str) -> String {
    if is_default_registry(registry) {
        "create a new one on the Download tokens tab of the organization on workers.iii.dev"
            .to_string()
    } else {
        format!("get a new one from {}", origin_text(registry))
    }
}

/// How to give Compose a token for `registry` when it has none. The environment variable applies
/// only to the default registry, and only on the process that runs Compose.
fn no_token_fix(registry: &str) -> String {
    let login = registry_command("login", registry);
    if is_default_registry(registry) {
        format!(
            "Save one with `{login}`, or set {TOKEN_ENV} {WHERE_COMPOSE_RUNS}: setting it in the \
             shell that runs `iii call` has no effect."
        )
    } else {
        format!("Save one with `{login}` ({TOKEN_ENV} applies only to {DEFAULT_REGISTRY}).")
    }
}

/// Why the registry refused a token (`code` is its `401` code; `detail` its own words, already
/// made safe), which token and from where, what that stops, and how to replace it. Never the
/// token itself, only its prefix.
pub(crate) fn auth_failure(
    code: &str,
    detail: Option<&str>,
    registry: &str,
    sent: Option<(&RegistryToken, &TokenSource)>,
    reader: Reader,
) -> Explained {
    let origin = origin_text(registry);
    let detail = detail
        .map(|detail| format!(" The registry says: {}.", detail.trim_end_matches('.')))
        .unwrap_or_default();
    let Some((token, source)) = sent else {
        return Explained {
            what: format!(
                "the registry at {origin} needs a token ({code}), and none was sent.{detail}"
            ),
            fix: no_token_fix(registry),
        };
    };
    let reason = match code {
        "invalid_token" => "is not valid",
        "token_expired" => "has expired",
        "token_revoked" => "was revoked",
        _ => "was refused",
    };
    let consequence = match (reader, source) {
        (Reader::Compose, _) => format!(
            " Every request to {origin} fails while this token is sent, public workers included."
        ),
        (Reader::Whoami, TokenSource::Environment) => format!(
            " If the Compose process (the `iii compose` daemon, or `iii compose build`) has this \
             same {TOKEN_ENV}, every install from {origin} fails, public workers included."
        ),
        (Reader::Whoami, _) => format!(
            " If Compose uses this same saved token, every install from {origin} fails, public \
             workers included."
        ),
        (Reader::Login, _) => String::new(),
    };
    let what = format!(
        "the registry at {origin} refused {}: it {reason} ({code}).{detail}{consequence}",
        token_from(token, source, reader)
    );
    let replace = if code == "invalid_token" {
        format!(
            "Check that the whole token was copied, or {}",
            new_token(registry)
        )
    } else {
        capitalize_first(&new_token(registry))
    };
    let login = registry_command("login", registry);
    let fix = match (source, reader) {
        (TokenSource::Given, _) => format!("{replace}, then run `{login}` again."),
        (TokenSource::File(path), _) => format!(
            "{replace}, then run `{login}`: it replaces the token saved in {}. Or run `{}` to \
             stop sending it: public workers need no token.",
            path.display(),
            registry_command("logout", registry)
        ),
        (TokenSource::Environment, Reader::Compose) => format!(
            "{replace}, then set {TOKEN_ENV} to it {WHERE_COMPOSE_RUNS}. Or unset it there: \
             Compose then uses the token `{login}` saves, if any, and public workers need none."
        ),
        (TokenSource::Environment, _) => format!(
            "{replace}, then set {TOKEN_ENV} to it, here and {WHERE_COMPOSE_RUNS} if it has the \
             same value. Or unset it: public workers need no token."
        ),
    };
    Explained { what, fix }
}

fn capitalize_first(text: &str) -> String {
    let mut chars = text.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}

/// What a not-found answer for scoped `name` may mean. Worded as a condition: the registry
/// answers a worker the token cannot see exactly as one that does not exist.
pub(crate) fn private_worker_hint(
    name: &str,
    registry: &str,
    sent: Option<(&RegistryToken, &TokenSource)>,
) -> String {
    let origin = origin_text(registry);
    if RegistryOrigin::parse(registry).is_ok_and(|origin| !origin.allows_tokens()) {
        return format!(
            "If '{name}' is a private worker, it cannot come from {origin}: iii sends a token \
             only over https, or over plain http to localhost, 127.0.0.1 and ::1."
        );
    }
    let Some((token, source)) = sent else {
        return format!(
            "If '{name}' is a private worker, Compose needs a token with access to it, and \
             none was sent. {}",
            no_token_fix(registry)
        );
    };
    let scope = name.split('/').next().unwrap_or(name);
    let whoami = registry_command("whoami", registry);
    let whoami_reads = match source {
        TokenSource::Environment => format!(
            " (it reads {TOKEN_ENV} from the shell it runs in, so set the same value there)"
        ),
        _ => String::new(),
    };
    format!(
        "If '{name}' is a private worker, {} has no access to it: the registry answers alike \
         for a worker that does not exist and for one the token cannot see. `{whoami}` lists \
         the workers a token reaches{whoami_reads}; an owner or admin of {scope} can give the \
         token access to it, or create one that has.",
        token_from(token, source, Reader::Compose)
    )
}

/// Why a token cannot be chosen for `registry`: `III_REGISTRY_TOKEN` or `credentials.toml` is
/// unusable. Says what is wrong and where, that it stops every request to the registry, public
/// workers included, whether `III_REGISTRY_TOKEN` gets around it, and how to fix it.
pub(crate) fn credentials_failure(
    error: &CredentialsError,
    registry: &str,
    reader: Reader,
) -> Explained {
    let origin = origin_text(registry);
    if let CredentialsError::InvalidEnvironmentToken { reason } = error {
        let (at, there) = match reader {
            Reader::Compose => (", as Compose reads it,", format!(" {WHERE_COMPOSE_RUNS}")),
            Reader::Login | Reader::Whoami => (" in this shell", String::new()),
        };
        return Explained {
            what: format!(
                "{TOKEN_ENV}{at} {reason}. Compose sends no request to {DEFAULT_REGISTRY} while \
                 it is so, public workers included, rather than fall back on another token."
            ),
            fix: format!(
                "Set it to the whole token, with nothing before or after it, or unset it{there}."
            ),
        };
    }
    let bypass = if is_default_registry(registry) {
        format!(
            " Setting {TOKEN_ENV} {WHERE_COMPOSE_RUNS} gets around it for {DEFAULT_REGISTRY} only."
        )
    } else {
        format!(" {TOKEN_ENV} does not get around it: it applies only to {DEFAULT_REGISTRY}.")
    };
    let why = match error {
        CredentialsError::InvalidFile { .. } => format!(
            " Compose resolves and downloads nothing from {origin} until it is fixed, public \
             workers included: without the file it cannot tell which token belongs to which \
             registry, so it sends none rather than guess.{bypass}"
        ),
        _ => String::new(),
    };
    let login = registry_command("login", registry);
    Explained {
        what: format!("{}.{why}", error.to_string().trim_end_matches('.')),
        fix: error
            .fix(Recovery::SignIn(&login))
            .unwrap_or_else(|| no_token_fix(registry)),
    }
}

/// `origin` is plain http to a host that is not loopback: no token goes there.
pub(crate) fn insecure(origin: &RegistryOrigin, subcommand: &str) -> Explained {
    let secure = origin.as_str().replacen("http://", "https://", 1);
    Explained {
        what: format!(
            "refusing to send a token to {origin} over plain http: anyone on the network between \
             here and there could read it, so nothing was sent."
        ),
        fix: format!(
            "Use https: `iii registry {subcommand} --registry {secure}`. Plain http may carry a \
             token only to localhost, 127.0.0.1 and ::1."
        ),
    }
}

/// Why a request got no final answer. The message never holds a header, and never a URL's
/// path or query: [`SendError::Http`] holds the error without its URL, and
/// [`SendError::Redirect`] describes a hop by its origin, or not at all once it left the first
/// URL's origin.
#[derive(Debug)]
pub(crate) enum SendError {
    Http(reqwest::Error),
    Redirect(String),
}

/// `url` for a message: its origin and path, never its user, query or fragment. A presigned URL
/// carries its signature in the query, and a URL from a lock may carry anything.
pub(crate) fn display_url(url: &str) -> String {
    match url::Url::parse(url) {
        Ok(parsed) if matches!(parsed.scheme(), "http" | "https") && parsed.has_host() => {
            format!("{}{}", parsed.origin().ascii_serialization(), parsed.path())
        }
        _ => {
            let end = url.find(['?', '#']).unwrap_or(url.len());
            let url = &url[..end];
            // `user:password@` before the host.
            match (url.find("://"), url.rfind('@')) {
                (Some(scheme), Some(at)) if at > scheme => {
                    format!("{}{}", &url[..scheme + 3], &url[at + 1..])
                }
                _ => url.to_string(),
            }
        }
    }
}

/// A hop of [`send`] for a message: its origin while it is on the first URL's origin, never its
/// path, which a redirect may fill with a token or a signature; and nothing about it past a
/// redirect to another one, where a presigned URL lives.
fn describe_hop(url: &url::Url, first: &url::Url) -> String {
    if url.origin() == first.origin() {
        url.origin().ascii_serialization()
    } else {
        "the location it was redirected to".to_string()
    }
}

impl SendError {
    pub(crate) fn is_timeout(&self) -> bool {
        matches!(self, Self::Http(error) if error.is_timeout())
    }
}

impl std::fmt::Display for SendError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Http(error) => error.fmt(f),
            Self::Redirect(message) => f.write_str(message),
        }
    }
}

/// The client every registry request goes through: redirects are followed by [`send`], never by
/// reqwest.
pub(crate) fn client_builder() -> reqwest::ClientBuilder {
    let builder = reqwest::Client::builder().redirect(reqwest::redirect::Policy::none());
    match crate::host_version() {
        Some(version) => builder.user_agent(format!("iii/{version}")),
        None => builder,
    }
}

/// Sends `method url` with the `json` body, following redirects itself. Each hop carries the
/// token only when [`RegistryAuth::token_for`] allows its URL. `client` must not follow
/// redirects ([`client_builder`]).
///
/// As browsers and reqwest do, `303` (and `301`/`302` after a `POST`) continue as a `GET` without
/// the body; `307` and `308` repeat the method and body.
pub(crate) async fn send(
    client: &reqwest::Client,
    method: Method,
    url: &str,
    json: Option<&serde_json::Value>,
    auth: &RegistryAuth,
) -> Result<reqwest::Response, SendError> {
    let mut url = url::Url::parse(url)
        .map_err(|error| SendError::Redirect(format!("not a valid URL: {error}")))?;
    let first = url.clone();
    let mut method = method;
    let mut json = json;
    for _ in 0..=MAX_REDIRECTS {
        let mut request = client.request(method.clone(), url.clone());
        if let Some(body) = json {
            request = request.json(body);
        }
        if let Some(token) = auth.token_for(&url) {
            let mut value = HeaderValue::from_str(&format!("Bearer {}", token.expose_secret()))
                .map_err(|_| SendError::Redirect("the registry token is not a header".into()))?;
            value.set_sensitive(true);
            request = request.header(AUTHORIZATION, value);
        }
        let response = request
            .send()
            .await
            .map_err(|error| SendError::Http(error.without_url()))?;
        let status = response.status();
        if !matches!(
            status,
            StatusCode::MOVED_PERMANENTLY
                | StatusCode::FOUND
                | StatusCode::SEE_OTHER
                | StatusCode::TEMPORARY_REDIRECT
                | StatusCode::PERMANENT_REDIRECT
        ) {
            return Ok(response);
        }
        let Some(location) = response
            .headers()
            .get(LOCATION)
            .and_then(|location| location.to_str().ok())
        else {
            return Ok(response);
        };
        let next = url.join(location).map_err(|error| {
            SendError::Redirect(format!(
                "{} redirected to an invalid location: {error}",
                describe_hop(&url, &first)
            ))
        })?;
        if !matches!(next.scheme(), "http" | "https") {
            return Err(SendError::Redirect(format!(
                "{} redirected to an unsupported scheme",
                describe_hop(&url, &first)
            )));
        }
        match status {
            StatusCode::SEE_OTHER => {
                if method != Method::HEAD {
                    method = Method::GET;
                }
                json = None;
            }
            StatusCode::MOVED_PERMANENTLY | StatusCode::FOUND if method == Method::POST => {
                method = Method::GET;
                json = None;
            }
            _ => {}
        }
        url = next;
    }
    Err(SendError::Redirect(format!(
        "more than {MAX_REDIRECTS} redirects"
    )))
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use wiremock::{Mock, MockServer, Request, ResponseTemplate, matchers};

    const TOKEN: &str = "wdt_testTokenValue";

    fn auth_for(registry: &str) -> RegistryAuth {
        RegistryAuth::for_registry(
            &Credentials::for_tests("https://api.workers.iii.dev", None, &[(registry, TOKEN)]),
            registry,
        )
    }

    fn authorization(request: &Request) -> Option<String> {
        request
            .headers
            .get("authorization")
            .map(|value| value.to_str().unwrap().to_string())
    }

    async fn get(url: &str, auth: &RegistryAuth) -> reqwest::Response {
        let client = client_builder().build().unwrap();
        send(&client, Method::GET, url, None, auth).await.unwrap()
    }

    #[tokio::test]
    async fn the_registry_origin_gets_the_token() {
        let registry = MockServer::start().await;
        Mock::given(matchers::path("/tokens/self"))
            .respond_with(ResponseTemplate::new(200))
            .mount(&registry)
            .await;
        let auth = auth_for(&registry.uri());
        get(&format!("{}/tokens/self", registry.uri()), &auth).await;
        let requests = registry.received_requests().await.unwrap();
        assert_eq!(
            authorization(&requests[0]).as_deref(),
            Some(format!("Bearer {TOKEN}").as_str())
        );
    }

    #[tokio::test]
    async fn another_origin_never_gets_it() {
        let registry = MockServer::start().await;
        let other = MockServer::start().await;
        Mock::given(matchers::any())
            .respond_with(ResponseTemplate::new(200))
            .mount(&other)
            .await;
        let auth = auth_for(&registry.uri());
        // Another port of the same host is another origin.
        get(&format!("{}/download/a", other.uri()), &auth).await;
        let requests = other.received_requests().await.unwrap();
        assert_eq!(requests.len(), 1);
        assert_eq!(authorization(&requests[0]), None);
    }

    #[tokio::test]
    async fn a_redirect_to_another_host_drops_the_token() {
        let registry = MockServer::start().await;
        let storage = MockServer::start().await;
        Mock::given(matchers::path("/download/%40acme%2Fade/1.0.0/target"))
            .respond_with(
                ResponseTemplate::new(302)
                    .insert_header("location", format!("{}/presigned?sig=x", storage.uri())),
            )
            .mount(&registry)
            .await;
        Mock::given(matchers::path("/presigned"))
            .respond_with(ResponseTemplate::new(200).set_body_string("archive"))
            .mount(&storage)
            .await;
        let auth = auth_for(&registry.uri());
        let response = get(
            &format!("{}/download/%40acme%2Fade/1.0.0/target", registry.uri()),
            &auth,
        )
        .await;
        assert_eq!(response.text().await.unwrap(), "archive");
        let first = registry.received_requests().await.unwrap();
        assert_eq!(
            authorization(&first[0]).as_deref(),
            Some(format!("Bearer {TOKEN}").as_str())
        );
        let second = storage.received_requests().await.unwrap();
        assert_eq!(second.len(), 1);
        assert_eq!(authorization(&second[0]), None);
    }

    #[tokio::test]
    async fn a_redirect_back_to_the_registry_carries_it_again() {
        let registry = MockServer::start().await;
        Mock::given(matchers::path("/old"))
            .respond_with(ResponseTemplate::new(308).insert_header("location", "/new"))
            .mount(&registry)
            .await;
        Mock::given(matchers::path("/new"))
            .respond_with(ResponseTemplate::new(200))
            .mount(&registry)
            .await;
        let auth = auth_for(&registry.uri());
        let client = client_builder().build().unwrap();
        let body = serde_json::json!({"worker": "@acme/ade"});
        send(
            &client,
            Method::POST,
            &format!("{}/old", registry.uri()),
            Some(&body),
            &auth,
        )
        .await
        .unwrap();
        let requests = registry.received_requests().await.unwrap();
        assert_eq!(requests.len(), 2);
        for request in &requests {
            assert_eq!(request.method.as_str(), "POST");
            assert!(authorization(request).is_some());
        }
        // 308 repeats the body.
        assert_eq!(
            serde_json::from_slice::<serde_json::Value>(&requests[1].body).unwrap(),
            body
        );
    }

    #[tokio::test]
    async fn a_redirect_loop_ends() {
        let registry = MockServer::start().await;
        Mock::given(matchers::path("/loop"))
            .respond_with(ResponseTemplate::new(302).insert_header("location", "/loop"))
            .mount(&registry)
            .await;
        let client = client_builder().build().unwrap();
        let error = send(
            &client,
            Method::GET,
            &format!("{}/loop", registry.uri()),
            None,
            &RegistryAuth::default(),
        )
        .await
        .unwrap_err();
        assert!(error.to_string().contains("redirects"), "{error}");
        assert_eq!(
            registry.received_requests().await.unwrap().len(),
            MAX_REDIRECTS + 1
        );
    }

    #[test]
    fn plain_http_off_loopback_never_carries_a_token() {
        let credentials = Credentials::for_tests(
            "https://api.workers.iii.dev",
            None,
            &[("http://registry.example", TOKEN)],
        );
        let auth = RegistryAuth::for_registry(&credentials, "http://registry.example");
        let url = url::Url::parse("http://registry.example/resolve").unwrap();
        assert!(auth.token_for(&url).is_none());
        assert!(auth.sent().is_none());
    }

    #[test]
    fn a_url_with_its_own_credentials_never_carries_a_token() {
        let auth = auth_for("https://registry.example");
        for url in [
            "https://registry.example/download/x",
            "https://user@registry.example/download/x",
            "https://registry.example.evil.test/download/x",
            "http://registry.example/download/x",
        ] {
            let parsed = url::Url::parse(url).unwrap();
            assert_eq!(
                auth.token_for(&parsed).is_some(),
                url == "https://registry.example/download/x",
                "{url}"
            );
        }
    }

    #[test]
    fn a_refusal_reads_both_shapes_and_never_echoes_the_token() {
        let auth = auth_for("https://registry.example");
        let nested = r#"{"error": {"code": "token_expired", "message": "Token expired."}}"#;
        assert_eq!(
            auth_refusal(nested, &auth),
            ("token_expired".into(), Some("Token expired.".into()))
        );
        let flat = r#"{"error": "Token revoked", "code": "token_revoked"}"#;
        assert_eq!(
            auth_refusal(flat, &auth),
            ("token_revoked".into(), Some("Token revoked".into()))
        );
        assert_eq!(auth_refusal("<html>", &auth), ("unauthorized".into(), None));
        let odd = r#"{"error": {"code": "bad code\n", "message": "x"}}"#;
        assert_eq!(auth_refusal(odd, &auth).0, "unauthorized");

        let echoed = format!(r#"{{"error": "Token {TOKEN} is not valid\u001b[2J"}}"#);
        let (_, message) = auth_refusal(&echoed, &auth);
        let message = message.unwrap();
        assert!(!message.contains(TOKEN), "{message}");
        assert!(message.contains("wdt_…"), "{message}");
        assert!(!message.contains('\u{1b}'), "{message}");
        let long = format!(r#"{{"error": "{}"}}"#, "a".repeat(1000));
        let message = auth_refusal(&long, &auth).1.unwrap();
        assert_eq!(message.chars().count(), MAX_UNTRUSTED_TEXT + 1);
    }

    /// A registry that echoes the token as its refusal code gets `unauthorized` printed instead;
    /// a code that does not hold it, such as `token_expired`, is kept.
    #[test]
    fn a_refusal_code_that_echoes_the_token_is_never_printed() {
        let auth = auth_for("https://registry.example");
        for body in [
            format!(r#"{{"error": "Token is not valid", "code": "{TOKEN}"}}"#),
            format!(r#"{{"error": {{"code": "bad_{TOKEN}", "message": "x"}}}}"#),
        ] {
            let (code, _) = auth_refusal(&body, &auth);
            assert_eq!(code, "unauthorized", "{body}");
        }
        let kept = r#"{"error": "Token expired", "code": "token_expired"}"#;
        assert_eq!(auth_refusal(kept, &auth).0, "token_expired");
    }

    /// A token as the registry issues them: long enough for its 8-character prefix to show.
    const LONG_TOKEN: &str = "wdt_AbCdSecretSecretSecretSecretSecret";

    #[test]
    fn a_refusal_names_the_token_by_prefix_its_source_and_what_it_stops() {
        let token = RegistryToken::new(LONG_TOKEN).unwrap();
        let explained = auth_failure(
            "token_revoked",
            Some("Token revoked."),
            DEFAULT_REGISTRY,
            Some((&token, &TokenSource::Environment)),
            Reader::Compose,
        );
        assert_eq!(
            explained.what,
            "the registry at https://api.workers.iii.dev refused the token wdt_AbCd… from \
             III_REGISTRY_TOKEN: it was revoked (token_revoked). The registry says: Token \
             revoked. Every request to https://api.workers.iii.dev fails \
             while this token is sent, public workers included."
        );
        assert_eq!(
            explained.fix,
            "Create a new one on the Download tokens tab of the organization on workers.iii.dev, \
             then set III_REGISTRY_TOKEN to it where Compose runs (on the `iii compose` daemon \
             process, then restarted, or in the shell that runs `iii compose build`). Or unset it \
             there: Compose then uses the token `iii registry login` saves, if any, and public \
             workers need none."
        );

        let saved = TokenSource::File(PathBuf::from("/home/ana/.iii/credentials.toml"));
        let explained = auth_failure(
            "invalid_token",
            None,
            "https://registry.example",
            Some((&token, &saved)),
            Reader::Compose,
        );
        assert!(
            explained.what.contains(
                "refused the token wdt_AbCd… saved in /home/ana/.iii/credentials.toml: it is not \
                 valid (invalid_token)."
            ),
            "{explained:?}"
        );
        assert_eq!(
            explained.fix,
            "Check that the whole token was copied, or get a new one from \
             https://registry.example, then run `iii registry login --registry \
             https://registry.example`: it replaces the token saved in \
             /home/ana/.iii/credentials.toml. Or run `iii registry logout --registry \
             https://registry.example` to stop sending it: public workers need no token."
        );

        let explained = auth_failure(
            "token_expired",
            None,
            DEFAULT_REGISTRY,
            Some((&token, &TokenSource::Given)),
            Reader::Login,
        );
        assert_eq!(
            explained.sentence(),
            "the registry at https://api.workers.iii.dev refused the token wdt_AbCd… given to \
             `iii registry login`: it has expired (token_expired). Create a new one on the \
             Download tokens tab of the organization on workers.iii.dev, then run `iii registry \
             login` again."
        );

        let unknown = auth_failure(
            "account_locked",
            None,
            DEFAULT_REGISTRY,
            Some((&token, &saved)),
            Reader::Whoami,
        );
        assert!(
            unknown.what.contains(
                "it was refused (account_locked). If Compose uses this same saved token, every \
                 install from https://api.workers.iii.dev fails, public workers included."
            ),
            "{unknown:?}"
        );
        for text in [&explained.what, &explained.fix, &unknown.what, &unknown.fix] {
            assert!(!text.contains("Secret"), "{text}");
        }
    }

    #[test]
    fn a_refusal_without_a_token_says_how_to_give_one() {
        let other = auth_failure(
            "unauthorized",
            None,
            "https://registry.example",
            None,
            Reader::Compose,
        );
        assert_eq!(
            other.sentence(),
            "the registry at https://registry.example needs a token (unauthorized), and none was \
             sent. Save one with `iii registry login --registry https://registry.example` \
             (III_REGISTRY_TOKEN applies only to https://api.workers.iii.dev)."
        );
        let default = auth_failure(
            "unauthorized",
            None,
            DEFAULT_REGISTRY,
            None,
            Reader::Compose,
        );
        assert!(
            default
                .fix
                .contains("setting it in the shell that runs `iii call` has no effect"),
            "{default:?}"
        );
    }

    #[test]
    fn a_private_worker_hint_names_the_token_and_how_to_check_it() {
        let token = RegistryToken::new(LONG_TOKEN).unwrap();
        let hint = private_worker_hint(
            "@acme/ade",
            DEFAULT_REGISTRY,
            Some((&token, &TokenSource::Environment)),
        );
        assert_eq!(
            hint,
            "If '@acme/ade' is a private worker, the token wdt_AbCd… from III_REGISTRY_TOKEN has \
             no access to it: the registry answers alike for a worker that does not exist and for one the token cannot see. `iii registry whoami` lists \
             the workers a token reaches (it reads III_REGISTRY_TOKEN from the shell it runs in, \
             so set the same value there); an owner or admin of @acme can give the token access \
             to it, or create one that has."
        );
        let insecure = private_worker_hint("@acme/ade", "http://registry.example", None);
        assert!(
            insecure.contains("cannot come from http://registry.example"),
            "{insecure}"
        );
    }

    #[test]
    fn a_broken_credentials_file_says_whether_the_variable_gets_around_it() {
        let error = CredentialsError::InvalidFile {
            path: PathBuf::from("/home/ana/.iii/credentials.toml"),
            problem: crate::credentials::FileProblem::Syntax { line: 4, column: 7 },
        };
        let default = credentials_failure(&error, DEFAULT_REGISTRY, Reader::Compose);
        assert_eq!(
            default.what,
            "/home/ana/.iii/credentials.toml is not valid TOML: syntax error at line 4, column 7. \
             Compose resolves and downloads nothing from https://api.workers.iii.dev until it is \
             fixed, public workers included: without the file it cannot tell which token belongs \
             to which registry, so it sends none rather than guess. Setting III_REGISTRY_TOKEN \
             where Compose runs (on the `iii compose` daemon process, then restarted, or in the \
             shell that runs `iii compose build`) gets around it for https://api.workers.iii.dev \
             only."
        );
        #[cfg(unix)]
        assert_eq!(
            default.fix,
            "Fix line 4 of the file: `nano +4 /home/ana/.iii/credentials.toml`. Or move the file \
             aside and sign in again, which drops the tokens saved for every other registry \
             (sign in to those again too): `mv /home/ana/.iii/credentials.toml \
             /home/ana/.iii/credentials.toml.bak && iii registry login`."
        );
        let other = credentials_failure(&error, "https://registry.example", Reader::Compose);
        assert!(
            other.what.ends_with(
                "III_REGISTRY_TOKEN does not get around it: it applies only to \
                 https://api.workers.iii.dev."
            ),
            "{other:?}"
        );
        assert!(
            other
                .fix
                .ends_with("&& iii registry login --registry https://registry.example`."),
            "{other:?}"
        );

        let environment = credentials_failure(
            &CredentialsError::InvalidEnvironmentToken {
                reason: "holds a character that is not visible ASCII",
            },
            DEFAULT_REGISTRY,
            Reader::Compose,
        );
        assert_eq!(
            environment.sentence(),
            "III_REGISTRY_TOKEN, as Compose reads it, holds a character that is not visible \
             ASCII. Compose sends no request to https://api.workers.iii.dev while it is so, \
             public workers included, rather than fall back on another token. Set it to the \
             whole token, with nothing before or after it, or unset it where Compose runs (on \
             the `iii compose` daemon process, then restarted, or in the shell that runs `iii \
             compose build`)."
        );
    }

    #[test]
    fn plain_http_is_refused_with_the_https_command() {
        let origin = RegistryOrigin::parse("http://registry.example:8080").unwrap();
        assert_eq!(
            insecure(&origin, "login").sentence(),
            "refusing to send a token to http://registry.example:8080 over plain http: anyone on \
             the network between here and there could read it, so nothing was sent. Use https: \
             `iii registry login --registry https://registry.example:8080`. Plain http may \
             carry a token only to localhost, 127.0.0.1 and ::1."
        );
    }

    #[test]
    fn a_url_in_a_message_keeps_only_its_origin_and_path() {
        for (url, shown) in [
            (
                "https://storage.example/bucket/ade.tar.gz?X-Amz-Signature=secret",
                "https://storage.example/bucket/ade.tar.gz",
            ),
            (
                "https://user:secret@registry.example:8443/download/x#secret",
                "https://registry.example:8443/download/x",
            ),
            ("http://127.0.0.1:9/a", "http://127.0.0.1:9/a"),
            ("not a url?secret", "not a url"),
            ("ftp://user:secret@host/x?y", "ftp://host/x"),
        ] {
            assert_eq!(display_url(url), shown, "{url}");
        }
    }

    #[tokio::test]
    async fn a_connection_error_never_quotes_the_url() {
        let auth = auth_for("https://registry.example");
        let client = client_builder().build().unwrap();
        let error = send(
            &client,
            Method::GET,
            "http://127.0.0.1:1/presigned?X-Amz-Signature=secret",
            None,
            &auth,
        )
        .await
        .unwrap_err();
        assert!(matches!(error, SendError::Http(_)));
        for text in [error.to_string(), format!("{error:?}")] {
            assert!(!text.contains("secret"), "{text}");
        }
    }

    #[tokio::test]
    async fn a_bad_redirect_within_the_registry_names_only_its_origin() {
        let registry = MockServer::start().await;
        Mock::given(matchers::path("/download"))
            .respond_with(
                ResponseTemplate::new(302).insert_header("location", "/signed/s3cr3t-value/x"),
            )
            .mount(&registry)
            .await;
        Mock::given(matchers::path("/signed/s3cr3t-value/x"))
            .respond_with(
                ResponseTemplate::new(302).insert_header("location", "file:///etc/passwd"),
            )
            .mount(&registry)
            .await;
        let client = client_builder().build().unwrap();
        let error = send(
            &client,
            Method::GET,
            &format!("{}/download", registry.uri()),
            None,
            &auth_for(&registry.uri()),
        )
        .await
        .unwrap_err();
        assert_eq!(
            error.to_string(),
            format!("{} redirected to an unsupported scheme", registry.uri())
        );
    }

    #[tokio::test]
    async fn a_bad_redirect_past_another_origin_is_not_described() {
        let registry = MockServer::start().await;
        let storage = MockServer::start().await;
        Mock::given(matchers::path("/download"))
            .respond_with(ResponseTemplate::new(302).insert_header(
                "location",
                format!("{}/presigned?X-Amz-Signature=secret", storage.uri()),
            ))
            .mount(&registry)
            .await;
        Mock::given(matchers::path("/presigned"))
            .respond_with(
                ResponseTemplate::new(302).insert_header("location", "file:///etc/passwd"),
            )
            .mount(&storage)
            .await;
        let client = client_builder().build().unwrap();
        let error = send(
            &client,
            Method::GET,
            &format!("{}/download", registry.uri()),
            None,
            &auth_for(&registry.uri()),
        )
        .await
        .unwrap_err();
        let text = error.to_string();
        assert!(text.contains("the location it was redirected to"), "{text}");
        assert!(
            !text.contains("secret") && !text.contains("presigned"),
            "{text}"
        );
    }
}
