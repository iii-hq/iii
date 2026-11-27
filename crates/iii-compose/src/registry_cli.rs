// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

//! `iii registry login | logout | whoami`: the registry tokens of this machine.
//!
//! A token is never an argument or a flag value, where shell history and process listings would
//! keep it: `login` reads it from a hidden prompt, or from standard input with `--token-stdin`.
//! It is checked with `GET /tokens/self` before anything is saved, so a mistyped, expired or
//! revoked token leaves the credentials file as it was. `whoami` asks the same endpoint and
//! prints who the token belongs to, never the token.

use std::io::{IsTerminal, Read};

use clap::{Args, Subcommand};
use colored::Colorize;
use serde::Deserialize;

use crate::{
    credentials::{
        CredentialStore, Credentials, CredentialsError, Recovery, RegistryOrigin, RegistryToken,
        Removed, TOKEN_ENV, Tightened, TokenSource,
    },
    registry::DEFAULT_REGISTRY,
    registry_http::{self, Explained, Reader, RegistryAuth},
};

/// How long one `/tokens/self` request may take.
const IDENTITY_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(20);

/// Exit code of a failed command, and of one given unusable arguments.
const FAILED: i32 = 1;
const USAGE: i32 = 2;

#[derive(Args, Debug, Clone)]
pub struct RegistryCli {
    #[command(subcommand)]
    pub command: RegistrySubcommand,
}

#[derive(Subcommand, Debug, Clone)]
pub enum RegistrySubcommand {
    /// Save a token for a worker registry on this machine, after the registry confirms it.
    ///
    /// The token is read from a hidden prompt, or from standard input with `--token-stdin`;
    /// it is never taken as an argument. It is saved in `~/.iii/credentials.toml`, readable
    /// only by you, and a running Compose daemon uses it from its next operation on. The
    /// `III_REGISTRY_TOKEN` environment variable, when it is set on the engine process, wins
    /// over the saved token for the default registry.
    Login(LoginCli),
    /// Remove the token saved for a worker registry.
    Logout(RegistryTarget),
    /// Show the organization, kind, access and expiry of the token for a worker registry.
    Whoami(RegistryTarget),
}

#[derive(Args, Debug, Clone)]
pub struct RegistryTarget {
    /// The registry: scheme, host and port, such as `https://api.workers.iii.dev`. A bare host
    /// means https.
    #[arg(long, value_name = "URL", default_value = DEFAULT_REGISTRY)]
    pub registry: String,
}

#[derive(Args, Debug, Clone)]
pub struct LoginCli {
    #[command(flatten)]
    pub target: RegistryTarget,

    /// Read the token from standard input instead of prompting for it, for scripts:
    /// `printf '%s' "$TOKEN" | iii registry login --token-stdin`.
    #[arg(long)]
    pub token_stdin: bool,
}

/// What `GET /tokens/self` answers: who a token belongs to.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct TokenIdentity {
    /// `download`, `member`, or the kind of an iii cloud token.
    pub kind: String,
    pub org: TokenOrg,
    /// The token's first characters, as the registry shows them to identify it.
    #[serde(default)]
    pub prefix: Option<String>,
    pub access: TokenAccess,
    /// `None` for a token that does not expire.
    #[serde(default)]
    pub expires_at: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct TokenOrg {
    pub handle: String,
    #[serde(default)]
    pub name: Option<String>,
}

/// `"all"`, or the workers the token is limited to.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(untagged)]
pub enum TokenAccess {
    Workers { workers: Vec<String> },
    Named(String),
}

/// Why a token could not be checked. The messages name the token by its prefix and where it
/// came from, never the token itself; [`IdentityError::fix`] says what to do.
#[derive(Debug, thiserror::Error)]
pub(crate) enum IdentityError {
    /// The registry refused the token (`401`): `code` says why.
    #[error("{}", .explained.what)]
    Refused { code: String, explained: Explained },
    #[error("the registry at {registry} answered HTTP {status} when asked about {token}")]
    Status {
        registry: String,
        status: u16,
        token: String,
    },
    #[error("cannot reach the registry at {registry} to check {token}: {message}")]
    Unreachable {
        registry: String,
        token: String,
        message: String,
    },
    #[error(
        "the registry at {registry} answered with a body iii does not understand when asked \
         about {token}: {message}"
    )]
    Body {
        registry: String,
        token: String,
        message: String,
    },
}

impl IdentityError {
    /// How to get past it; `again` is the command to run once it is fixed.
    pub fn fix(&self, again: &str) -> String {
        match self {
            Self::Refused { explained, .. } => explained.fix.clone(),
            Self::Status {
                registry, status, ..
            } if *status == 404 => format!(
                "{registry} has no /tokens/self, so it is not an iii worker registry that hosts \
                 private workers. Check --registry (the default is {DEFAULT_REGISTRY}), then run \
                 `{again}`."
            ),
            Self::Status { status, .. } if *status >= 500 => {
                format!("The registry failed on its side; run `{again}` again in a moment.")
            }
            Self::Status { .. } => format!(
                "Check --registry (the default is {DEFAULT_REGISTRY}), then run `{again}` again."
            ),
            Self::Unreachable { .. } => format!(
                "Check the address and this machine's network (DNS, a proxy, a firewall), then \
                 run `{again}` again."
            ),
            Self::Body { .. } => format!(
                "Check --registry (the default is {DEFAULT_REGISTRY}). If it is right, this iii \
                 may be older than the registry: run `iii update`, then `{again}`."
            ),
        }
    }
}

/// Asks `origin` who `token` belongs to. The token goes to that origin only, never past a
/// redirect to another one. `reader` says which command asks, for the messages.
pub(crate) async fn token_identity(
    origin: &RegistryOrigin,
    token: &RegistryToken,
    source: TokenSource,
    reader: Reader,
) -> Result<TokenIdentity, IdentityError> {
    let registry = origin.as_str().to_string();
    let described = registry_http::token_from(token, &source, reader);
    let unreachable = |message: String| IdentityError::Unreachable {
        registry: registry.clone(),
        token: described.clone(),
        message,
    };
    let auth = RegistryAuth::with_token(origin, token.clone(), source.clone());
    let client = registry_http::client_builder()
        .timeout(IDENTITY_TIMEOUT)
        .build()
        .map_err(|error| unreachable(error.to_string()))?;
    let response = registry_http::send(
        &client,
        reqwest::Method::GET,
        &format!("{registry}/tokens/self"),
        None,
        &auth,
    )
    .await
    .map_err(|error| unreachable(error.to_string()))?;
    let status = response.status();
    if status == reqwest::StatusCode::UNAUTHORIZED && auth.is_registry(response.url()) {
        let body = response.text().await.unwrap_or_default();
        let (code, detail) = registry_http::auth_refusal(&body, &auth);
        return Err(IdentityError::Refused {
            explained: registry_http::auth_failure(
                &code,
                detail.as_deref(),
                &registry,
                Some((token, &source)),
                reader,
            ),
            code,
        });
    }
    if !status.is_success() {
        return Err(IdentityError::Status {
            registry,
            status: status.as_u16(),
            token: described,
        });
    }
    response
        .json::<TokenIdentity>()
        .await
        .map_err(|error| IdentityError::Body {
            registry,
            token: described,
            message: error.without_url().to_string(),
        })
}

/// What `login` did.
#[derive(Debug)]
pub(crate) struct LoggedIn {
    pub origin: RegistryOrigin,
    pub identity: TokenIdentity,
    pub path: std::path::PathBuf,
    /// `III_REGISTRY_TOKEN` is set and applies to this registry, so it still wins over the
    /// token just saved wherever it is set.
    pub environment_wins: bool,
    /// The credentials directory had to be closed to other users first.
    pub tightened: Option<Tightened>,
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum LoginError {
    #[error(transparent)]
    Credentials(#[from] CredentialsError),
    #[error(transparent)]
    Identity(#[from] IdentityError),
    /// Plain http to a host that is not loopback: no token goes there.
    #[error("{}", .0.what)]
    Insecure(Explained),
}

impl LoginError {
    /// How to fix it, with commands to copy. `recovery` names the command that failed.
    pub fn fix(&self, recovery: Recovery<'_>) -> Option<String> {
        let again = match recovery {
            Recovery::SignIn(command)
            | Recovery::LoginAgain(command)
            | Recovery::SignOut(command) => command,
        };
        match self {
            Self::Credentials(error) => error.fix(recovery),
            Self::Identity(error) => Some(error.fix(again)),
            Self::Insecure(explained) => Some(explained.fix.clone()),
        }
    }
}

/// Checks `token` with `registry` and, only once it is confirmed, saves it in `store`.
///
/// A credentials file that cannot be read stops it before the token is sent anywhere: it is
/// never overwritten, as that would drop the tokens of other registries it may hold.
pub(crate) async fn login(
    registry: &str,
    token: RegistryToken,
    store: &CredentialStore,
    environment: Option<String>,
) -> Result<LoggedIn, LoginError> {
    let origin = registry_origin(registry)?;
    if !origin.allows_tokens() {
        return Err(LoginError::Insecure(registry_http::insecure(
            &origin, "login",
        )));
    }
    store.read()?;
    let identity = token_identity(&origin, &token, TokenSource::Given, Reader::Login).await?;
    let tightened = store.save(&origin, &token)?;
    let environment_wins =
        Credentials::from_sources(environment, None).environment_applies_to(&origin);
    Ok(LoggedIn {
        origin,
        identity,
        path: store.path().to_path_buf(),
        environment_wins,
        tightened,
    })
}

/// Removes the token `store` holds for `registry`. A file that cannot be read is left as it is.
pub(crate) fn logout(registry: &str, store: &CredentialStore) -> Result<Removed, LoginError> {
    let origin = registry_origin(registry)?;
    Ok(store.remove(&origin)?)
}

/// Who the token the engine would use for `registry` belongs to, and where it came from.
pub(crate) async fn whoami(
    registry: &str,
    credentials: &Credentials,
) -> Result<Option<(TokenIdentity, TokenSource)>, LoginError> {
    let origin = registry_origin(registry)?;
    if !origin.allows_tokens() {
        return Err(LoginError::Insecure(registry_http::insecure(
            &origin, "whoami",
        )));
    }
    let Some((token, source)) = credentials.token_for(&origin)? else {
        return Ok(None);
    };
    let identity = token_identity(&origin, token, source.clone(), Reader::Whoami).await?;
    Ok(Some((identity, source)))
}

/// `--registry` as an origin; a bare host is https.
fn registry_origin(registry: &str) -> Result<RegistryOrigin, CredentialsError> {
    let registry = registry.trim();
    if registry.contains("://") {
        RegistryOrigin::parse(registry)
    } else {
        RegistryOrigin::parse(&format!("https://{registry}"))
    }
}

/// Entry point behind `iii registry`. Returns the process exit code.
pub async fn run(cli: RegistryCli) -> i32 {
    match cli.command {
        RegistrySubcommand::Login(args) => run_login(args).await,
        RegistrySubcommand::Logout(target) => run_logout(target),
        RegistrySubcommand::Whoami(target) => run_whoami(target).await,
    }
}

/// `--registry` as an origin, or the usage error that says why it is not one.
fn target_origin(target: &RegistryTarget) -> Result<RegistryOrigin, i32> {
    registry_origin(&target.registry).map_err(|error| {
        report(
            &error.to_string(),
            Some(&format!(
                "Give the registry's scheme, host and port only, such as {DEFAULT_REGISTRY}."
            )),
        );
        USAGE
    })
}

async fn run_login(args: LoginCli) -> i32 {
    let origin = match target_origin(&args.target) {
        Ok(origin) => origin,
        Err(code) => return code,
    };
    let command = registry_http::registry_command("login", origin.as_str());
    let store = match CredentialStore::default_location() {
        Ok(store) => store,
        Err(error) => {
            return fail(
                &error.to_string(),
                error.fix(Recovery::LoginAgain(&command)),
            );
        }
    };
    if !origin.allows_tokens() {
        let explained = registry_http::insecure(&origin, "login");
        return fail(&explained.what, Some(explained.fix));
    }
    // Before the token is asked for: a file login would refuse to rewrite makes typing it moot.
    if let Err(error) = store.read() {
        return fail(
            &format!(
                "{error}. `iii registry login` never overwrites a credentials file it cannot \
                 read, as that would drop the tokens it holds for other registries: it stopped \
                 before asking for the token."
            ),
            error.fix(Recovery::LoginAgain(&command)),
        );
    }
    let token = match read_token(&args) {
        Ok(token) => token,
        Err(message) => {
            report(&message, None);
            return USAGE;
        }
    };
    match login(
        &args.target.registry,
        token,
        &store,
        std::env::var(TOKEN_ENV).ok(),
    )
    .await
    {
        Ok(done) => {
            if let Some(tightened) = &done.tightened {
                println!("{tightened}");
            }
            println!(
                "Logged in to {} as @{}.",
                done.origin,
                printable(&done.identity.org.handle)
            );
            print_identity(&done.identity);
            println!("Saved to {}.", done.path.display());
            println!(
                "A running Compose daemon uses it from its next operation; nothing needs a restart."
            );
            if done.environment_wins {
                println!(
                    "Note: {TOKEN_ENV} is set in this shell. Where it is set (on the engine \
                     process, for instance), it wins over the saved token."
                );
            }
            0
        }
        Err(error) => fail(
            &format!(
                "{} Nothing was saved, and {} is as it was.",
                sentence(&error.to_string()),
                store.path().display()
            ),
            error.fix(Recovery::LoginAgain(&command)),
        ),
    }
}

fn run_logout(target: RegistryTarget) -> i32 {
    let origin = match target_origin(&target) {
        Ok(origin) => origin,
        Err(code) => return code,
    };
    let command = registry_http::registry_command("logout", origin.as_str());
    let store = match CredentialStore::default_location() {
        Ok(store) => store,
        Err(error) => return fail(&error.to_string(), error.fix(Recovery::SignOut(&command))),
    };
    let path = store.path().display();
    match logout(&target.registry, &store) {
        Ok(Removed {
            removed: true,
            tightened,
        }) => {
            if let Some(tightened) = tightened {
                println!("{tightened}");
            }
            println!("Removed the token for {origin} from {path}.");
        }
        Ok(Removed { removed: false, .. }) => {
            let saved = store.origins().unwrap_or_default();
            let holds = if !store.path().exists() {
                format!("{path} does not exist")
            } else if saved.is_empty() {
                format!("{path} holds no token")
            } else {
                format!(
                    "{path} holds tokens for {} only",
                    saved
                        .iter()
                        .map(RegistryOrigin::as_str)
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            };
            println!("No token is saved for {origin}, so there is nothing to remove: {holds}.");
        }
        Err(LoginError::Credentials(error @ CredentialsError::InvalidFile { .. })) => {
            return fail(
                &format!(
                    "{error}. `iii registry logout` never rewrites a credentials file it cannot \
                     read, so nothing was removed."
                ),
                error.fix(Recovery::SignOut(&command)),
            );
        }
        Err(error) => {
            return fail(
                &format!("{} Nothing was removed.", sentence(&error.to_string())),
                error.fix(Recovery::SignOut(&command)),
            );
        }
    }
    if std::env::var(TOKEN_ENV).is_ok_and(|value| !value.trim().is_empty())
        && origin == RegistryOrigin::default_registry()
    {
        println!(
            "{TOKEN_ENV} is still set in this shell, and still applies wherever it is set (on \
             the engine process, for instance). Unset it there to stop using it."
        );
    }
    0
}

async fn run_whoami(target: RegistryTarget) -> i32 {
    let origin = match target_origin(&target) {
        Ok(origin) => origin,
        Err(code) => return code,
    };
    let command = registry_http::registry_command("whoami", origin.as_str());
    let credentials = Credentials::load();
    match whoami(&target.registry, &credentials).await {
        Ok(Some((identity, source))) => {
            println!("Registry: {origin}");
            println!("Token from: {source}");
            println!(
                "Org: @{}{}",
                printable(&identity.org.handle),
                identity
                    .org
                    .name
                    .as_deref()
                    .map(|name| format!(" ({})", printable(name)))
                    .unwrap_or_default()
            );
            print_identity(&identity);
            0
        }
        Ok(None) => {
            let explained = no_token(&origin, credentials.file());
            fail(&explained.what, Some(explained.fix))
        }
        Err(LoginError::Credentials(error)) => {
            let explained =
                registry_http::credentials_failure(&error, origin.as_str(), Reader::Whoami);
            fail(&explained.what, Some(explained.fix))
        }
        Err(error) => fail(
            &sentence(&error.to_string()),
            error.fix(Recovery::SignIn(&command)),
        ),
    }
}

/// `whoami` found no token for `origin`: where it looked, what that means, and how to add one.
fn no_token(origin: &RegistryOrigin, file: Option<&std::path::Path>) -> Explained {
    let default = *origin == RegistryOrigin::default_registry();
    let environment = if default {
        format!("{TOKEN_ENV} is not set in this shell")
    } else {
        format!("{TOKEN_ENV} applies only to {DEFAULT_REGISTRY}")
    };
    let file = match file {
        Some(path) if path.exists() => format!("{} holds none for it", path.display()),
        Some(path) => format!("{} does not exist", path.display()),
        None => "there is no home directory to keep a credentials file in".to_string(),
    };
    let login = registry_http::registry_command("login", origin.as_str());
    let fix = if default {
        format!(
            "Run `{login}`, or set {TOKEN_ENV}. `iii registry whoami` reads {TOKEN_ENV} from \
             this shell, while the Compose daemon reads it from the environment of the engine \
             process: set it there for Compose."
        )
    } else {
        format!("Run `{login}`.")
    };
    Explained {
        what: format!(
            "no token for {origin}: {environment}, and {file}. Without one, Compose installs \
             only public workers from {origin}."
        ),
        fix,
    }
}

fn print_identity(identity: &TokenIdentity) {
    println!("Kind: {}", printable(&identity.kind));
    if let Some(prefix) = &identity.prefix {
        println!("Token: {}…", printable(prefix));
    }
    let access = match &identity.access {
        TokenAccess::Named(all) if all == "all" => {
            format!("every worker of @{}", printable(&identity.org.handle))
        }
        TokenAccess::Named(other) => printable(other),
        TokenAccess::Workers { workers } if workers.is_empty() => "no worker".to_string(),
        TokenAccess::Workers { workers } => workers
            .iter()
            .map(|worker| printable(worker))
            .collect::<Vec<_>>()
            .join(", "),
    };
    println!("Access: {access}");
    println!(
        "Expires: {}",
        identity
            .expires_at
            .as_deref()
            .map(printable)
            .unwrap_or_else(|| "never".to_string())
    );
}

/// Prints an error and, when there is one, the hint that fixes it, as the rest of the CLI does.
fn report(what: &str, fix: Option<&str>) {
    eprintln!("{} {what}", "error:".red());
    if let Some(fix) = fix {
        eprintln!("  {} {fix}", "hint:".dimmed());
    }
}

fn fail(what: &str, fix: Option<String>) -> i32 {
    report(what, fix.as_deref());
    FAILED
}

/// `text` ending in a full stop, to be followed by another sentence.
fn sentence(text: &str) -> String {
    if text.ends_with(['.', '!', '?']) {
        text.to_string()
    } else {
        format!("{text}.")
    }
}

/// Text the registry sent, safe to print: control characters (terminal escapes among them)
/// become `?`, and it is cut at 200 characters.
fn printable(text: &str) -> String {
    text.chars()
        .take(200)
        .map(|c| if c.is_control() { '?' } else { c })
        .collect()
}

/// The token to log in with: standard input with `--token-stdin`, else a hidden prompt on the
/// terminal. Never an argument.
fn read_token(args: &LoginCli) -> Result<RegistryToken, String> {
    let text = if args.token_stdin {
        let mut text = String::new();
        std::io::stdin()
            .take(64 * 1024)
            .read_to_string(&mut text)
            .map_err(|error| format!("cannot read the token from standard input: {error}"))?;
        text
    } else if std::io::stdin().is_terminal() && std::io::stderr().is_terminal() {
        let term = console::Term::stderr();
        term.write_str(&format!(
            "Token for {} (input is hidden): ",
            registry_origin(&args.target.registry)
                .map(|origin| origin.to_string())
                .unwrap_or_else(|_| args.target.registry.clone())
        ))
        .map_err(|error| error.to_string())?;
        term.read_secure_line()
            .map_err(|error| format!("cannot read the token: {error}"))?
    } else {
        return Err(
            "no terminal to prompt for the token on: pipe it in with `--token-stdin`".to_string(),
        );
    };
    RegistryToken::new(&text).map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::credentials::{CREDENTIALS_FILE, commands};
    use wiremock::{Mock, MockServer, ResponseTemplate, matchers};

    fn identity_body() -> serde_json::Value {
        serde_json::json!({
            "kind": "download",
            "org": {"handle": "acme", "name": "Acme"},
            "prefix": "wdt_AbCd",
            "access": {"workers": ["@acme/ade"]},
            "expires_at": null
        })
    }

    async fn registry(status: u16, body: serde_json::Value) -> MockServer {
        let server = MockServer::start().await;
        Mock::given(matchers::method("GET"))
            .and(matchers::path("/tokens/self"))
            .respond_with(ResponseTemplate::new(status).set_body_json(body))
            .mount(&server)
            .await;
        server
    }

    fn store() -> (tempfile::TempDir, CredentialStore) {
        let home = tempfile::tempdir().unwrap();
        let store = CredentialStore::at(home.path().join(".iii").join(CREDENTIALS_FILE));
        (home, store)
    }

    #[test]
    fn the_token_is_never_an_argument() {
        use clap::Parser;
        #[derive(Parser)]
        struct Root {
            #[command(subcommand)]
            registry: RegistrySubcommand,
        }
        assert!(Root::try_parse_from(["iii", "login", "wdt_secret"]).is_err());
        assert!(Root::try_parse_from(["iii", "login", "--token", "wdt_secret"]).is_err());
        assert!(Root::try_parse_from(["iii", "login", "--token-stdin", "wdt_secret"]).is_err());
        assert!(Root::try_parse_from(["iii", "login", "--web"]).is_err());
        let parsed = Root::try_parse_from(["iii", "login", "--token-stdin"]).unwrap();
        let RegistrySubcommand::Login(login) = parsed.registry else {
            panic!("not login")
        };
        assert!(login.token_stdin);
        assert_eq!(login.target.registry, DEFAULT_REGISTRY);
        let parsed =
            Root::try_parse_from(["iii", "whoami", "--registry", "https://registry.example"])
                .unwrap();
        let RegistrySubcommand::Whoami(target) = parsed.registry else {
            panic!("not whoami")
        };
        assert_eq!(target.registry, "https://registry.example");
    }

    /// As long as the tokens the registry issues, so that messages show their 8-character prefix.
    const EXPIRED: &str = "wdt_ExpZexpiredSecret0123456789abcdefgh";

    #[tokio::test]
    async fn a_refused_token_saves_nothing_and_says_why() {
        let server = registry(
            401,
            serde_json::json!({"error": "Token expired", "code": "token_expired"}),
        )
        .await;
        let (_home, store) = store();
        let error = login(
            &server.uri(),
            RegistryToken::new(EXPIRED).unwrap(),
            &store,
            None,
        )
        .await
        .unwrap_err();
        let LoginError::Identity(IdentityError::Refused { code, explained }) = &error else {
            panic!("wrong error: {error}")
        };
        assert_eq!(code, "token_expired");
        assert_eq!(
            explained.what,
            format!(
                "the registry at {} refused the token wdt_ExpZ… given to `iii registry login`: it \
                 has expired (token_expired). The registry says: Token expired.",
                server.uri()
            )
        );
        assert_eq!(
            error
                .fix(Recovery::LoginAgain("iii registry login"))
                .unwrap(),
            format!(
                "Get a new one from {uri}, then run `iii registry login --registry {uri}` again.",
                uri = server.uri()
            )
        );
        assert!(!format!("{error} {error:?}").contains("expiredSecret"));
        assert!(!store.path().exists());
        // The registry received the token, and only on /tokens/self.
        let requests = server.received_requests().await.unwrap();
        assert_eq!(requests.len(), 1);
        assert_eq!(
            requests[0].headers.get("authorization").unwrap(),
            format!("Bearer {EXPIRED}").as_str()
        );
    }

    #[tokio::test]
    async fn a_registry_that_cannot_check_the_token_says_what_to_check() {
        let (_home, store) = store();
        let token = || RegistryToken::new(EXPIRED).unwrap();

        // A port nothing listens on any more (a dropped MockServer goes back to a pool instead).
        let address = {
            let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
            format!("http://{}", listener.local_addr().unwrap())
        };
        let error = login(&address, token(), &store, None).await.unwrap_err();
        let message = error.to_string();
        assert!(
            message.starts_with(&format!(
                "cannot reach the registry at {address} to check the token wdt_ExpZ… given to \
                 `iii registry login`: "
            )),
            "{message}"
        );
        assert!(
            error
                .fix(Recovery::LoginAgain("iii registry login"))
                .unwrap()
                .starts_with("Check the address and this machine's network"),
            "{error:?}"
        );

        let strange = registry(200, serde_json::json!({"hello": "world"})).await;
        let error = login(&strange.uri(), token(), &store, None)
            .await
            .unwrap_err();
        assert!(
            error.to_string().starts_with(&format!(
                "the registry at {} answered with a body iii does not understand when asked \
                 about the token wdt_ExpZ…",
                strange.uri()
            )),
            "{error}"
        );
        assert!(
            error
                .fix(Recovery::LoginAgain("iii registry login"))
                .unwrap()
                .contains("run `iii update`, then `iii registry login`"),
            "{error:?}"
        );

        let elsewhere = registry(404, serde_json::json!({"error": "Not found"})).await;
        let error = login(&elsewhere.uri(), token(), &store, None)
            .await
            .unwrap_err();
        assert!(error.to_string().contains("answered HTTP 404"), "{error}");
        assert!(
            error
                .fix(Recovery::LoginAgain("iii registry login"))
                .unwrap()
                .contains("has no /tokens/self, so it is not an iii worker registry"),
            "{error:?}"
        );
        assert!(!format!("{error} {error:?}").contains("expiredSecret"));
        assert!(!store.path().exists());
    }

    #[tokio::test]
    async fn login_never_overwrites_a_file_it_cannot_read_nor_sends_the_token() {
        let server = registry(200, identity_body()).await;
        let (_home, store) = store();
        std::fs::create_dir_all(store.path().parent().unwrap()).unwrap();
        let text =
            "[registries.\"https://registry.example\"]\ntoken = \"wdt_otherSecret\"\nbroken\n";
        std::fs::write(store.path(), text).unwrap();
        let error = login(
            &server.uri(),
            RegistryToken::new("wdt_new").unwrap(),
            &store,
            None,
        )
        .await
        .unwrap_err();
        assert!(
            matches!(
                error,
                LoginError::Credentials(CredentialsError::InvalidFile { .. })
            ),
            "{error:?}"
        );
        assert!(server.received_requests().await.unwrap().is_empty());
        assert_eq!(std::fs::read_to_string(store.path()).unwrap(), text);
        let fix = error
            .fix(Recovery::LoginAgain("iii registry login"))
            .unwrap();
        assert!(
            fix.starts_with(&format!(
                "Fix line 3 of the file: `{}`, then run `iii registry login` again.",
                commands::edit(store.path(), 3)
            )),
            "{fix}"
        );
        assert!(
            fix.contains(&format!(
                "`{} && iii registry login`",
                commands::move_aside(store.path())
            )),
            "{fix}"
        );
        assert!(!format!("{error} {fix}").contains("otherSecret"));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn login_says_when_it_closes_the_directory() {
        use std::os::unix::fs::PermissionsExt;
        let server = registry(200, identity_body()).await;
        let (_home, store) = store();
        let dir = store.path().parent().unwrap().to_path_buf();
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o755)).unwrap();
        let done = login(
            &server.uri(),
            RegistryToken::new("wdt_valid").unwrap(),
            &store,
            None,
        )
        .await
        .unwrap();
        let tightened = done.tightened.expect("the directory was open");
        assert_eq!(tightened.directory, dir);
        assert_eq!(tightened.from, 0o755);
    }

    #[tokio::test]
    async fn an_existing_token_survives_a_refused_login() {
        let server = registry(
            401,
            serde_json::json!({"error": "Token revoked", "code": "token_revoked"}),
        )
        .await;
        let (_home, store) = store();
        let origin = RegistryOrigin::parse(&server.uri()).unwrap();
        store
            .save(&origin, &RegistryToken::new("wdt_previous").unwrap())
            .unwrap();
        login(
            &server.uri(),
            RegistryToken::new("wdt_new").unwrap(),
            &store,
            None,
        )
        .await
        .unwrap_err();
        assert_eq!(
            store.read().unwrap()[&origin].expose_secret(),
            "wdt_previous"
        );
    }

    #[tokio::test]
    async fn a_confirmed_token_is_saved_for_its_origin() {
        let server = registry(200, identity_body()).await;
        let (_home, store) = store();
        let done = login(
            &server.uri(),
            RegistryToken::new("wdt_valid").unwrap(),
            &store,
            Some("wdt_environment".into()),
        )
        .await
        .unwrap();
        assert_eq!(done.identity.org.handle, "acme");
        assert_eq!(
            done.identity.access,
            TokenAccess::Workers {
                workers: vec!["@acme/ade".into()]
            }
        );
        // The environment variable belongs to the default registry, not this one.
        assert!(!done.environment_wins);
        let origin = RegistryOrigin::parse(&server.uri()).unwrap();
        assert_eq!(store.read().unwrap()[&origin].expose_secret(), "wdt_valid");
    }

    #[tokio::test]
    async fn plain_http_off_loopback_is_refused_before_any_request() {
        let (_home, store) = store();
        let error = login(
            "http://registry.example",
            RegistryToken::new("wdt_x").unwrap(),
            &store,
            None,
        )
        .await
        .unwrap_err();
        assert!(matches!(error, LoginError::Insecure(_)), "{error}");
        assert!(!store.path().exists());
    }

    #[tokio::test]
    async fn logout_removes_only_that_origin() {
        let (_home, store) = store();
        let first = RegistryOrigin::parse("https://registry.example").unwrap();
        let second = RegistryOrigin::default_registry();
        store
            .save(&first, &RegistryToken::new("wdt_a").unwrap())
            .unwrap();
        store
            .save(&second, &RegistryToken::new("wdt_b").unwrap())
            .unwrap();
        assert!(logout("registry.example", &store).unwrap().removed);
        assert!(!logout("https://registry.example", &store).unwrap().removed);
        let tokens = store.read().unwrap();
        assert_eq!(tokens.keys().collect::<Vec<_>>(), [&second]);
    }

    #[tokio::test]
    async fn whoami_names_the_owner_and_never_the_token() {
        let server = registry(
            200,
            serde_json::json!({
                "kind": "download",
                "org": {"handle": "acme", "name": "Acme"},
                "prefix": "wdt_AbCd",
                "access": "all",
                "expires_at": "2027-01-01T00:00:00Z"
            }),
        )
        .await;
        let credentials = Credentials::for_tests(
            "https://api.workers.iii.dev",
            None,
            &[(server.uri().as_str(), "wdt_whoamiSecret")],
        );
        let (identity, source) = whoami(&server.uri(), &credentials).await.unwrap().unwrap();
        assert_eq!(identity.access, TokenAccess::Named("all".into()));
        assert_eq!(identity.expires_at.as_deref(), Some("2027-01-01T00:00:00Z"));
        assert!(matches!(source, TokenSource::File(_)));
        assert!(!format!("{identity:?}").contains("whoamiSecret"));

        let nobody = Credentials::for_tests("https://api.workers.iii.dev", None, &[]);
        assert!(whoami(&server.uri(), &nobody).await.unwrap().is_none());
    }

    #[test]
    fn logout_leaves_a_file_it_cannot_read_as_it_is() {
        let (_home, store) = store();
        std::fs::create_dir_all(store.path().parent().unwrap()).unwrap();
        std::fs::write(store.path(), "registries = 1\n").unwrap();
        let error = logout(DEFAULT_REGISTRY, &store).unwrap_err();
        assert_eq!(
            std::fs::read_to_string(store.path()).unwrap(),
            "registries = 1\n"
        );
        let fix = error.fix(Recovery::SignOut("iii registry logout")).unwrap();
        assert!(
            fix.contains("then run `iii registry logout` again"),
            "{fix}"
        );
        assert!(
            fix.ends_with(&format!("`{}`.", commands::move_aside(store.path()))),
            "{fix}"
        );
    }

    #[tokio::test]
    async fn whoami_with_a_broken_file_says_so_rather_than_no_token() {
        let (_home, store) = store();
        std::fs::create_dir_all(store.path().parent().unwrap()).unwrap();
        std::fs::write(store.path(), "registries = [\n").unwrap();
        let credentials = Credentials::from_sources(None, Some(&store));
        let error = whoami(DEFAULT_REGISTRY, &credentials).await.unwrap_err();
        let LoginError::Credentials(error) = error else {
            panic!("{error:?}")
        };
        let explained =
            registry_http::credentials_failure(&error, DEFAULT_REGISTRY, Reader::Whoami);
        assert!(
            explained.what.starts_with(&format!(
                "{} is not valid TOML: syntax error at line 1, column 15.",
                store.path().display()
            )),
            "{explained:?}"
        );
        assert!(
            explained.fix.contains(&commands::edit(store.path(), 1)),
            "{explained:?}"
        );
    }

    #[test]
    fn whoami_without_a_token_says_where_it_looked() {
        let path = std::path::Path::new("/nowhere/.iii/credentials.toml");
        let default = no_token(&RegistryOrigin::default_registry(), Some(path));
        assert_eq!(
            default.what,
            "no token for https://api.workers.iii.dev: III_REGISTRY_TOKEN is not set in this \
             shell, and /nowhere/.iii/credentials.toml does not exist. Without one, Compose \
             installs only public workers from https://api.workers.iii.dev."
        );
        assert!(
            default
                .fix
                .starts_with("Run `iii registry login`, or set III_REGISTRY_TOKEN."),
            "{default:?}"
        );
        let other = no_token(
            &RegistryOrigin::parse("https://registry.example").unwrap(),
            Some(path),
        );
        assert!(
            other.what.contains(
                "III_REGISTRY_TOKEN applies only to https://api.workers.iii.dev, and \
                 /nowhere/.iii/credentials.toml does not exist"
            ),
            "{other:?}"
        );
        assert_eq!(
            other.fix,
            "Run `iii registry login --registry https://registry.example`."
        );
    }

    #[test]
    fn registry_text_cannot_drive_the_terminal() {
        assert_eq!(printable("acme\u{1b}[2J"), "acme?[2J");
        assert_eq!(printable(&"a".repeat(500)).len(), 200);
    }

    #[test]
    fn a_bare_host_is_https() {
        assert_eq!(
            registry_origin("api.workers.iii.dev").unwrap(),
            RegistryOrigin::default_registry()
        );
        assert_eq!(
            registry_origin("localhost:4000").unwrap().as_str(),
            "https://localhost:4000"
        );
        assert!(registry_origin("https://api.workers.iii.dev/v1").is_err());
    }
}
