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
    registry_http::{self, Explained, Reader, RegistryAuth, WHERE_COMPOSE_RUNS},
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
    /// `III_REGISTRY_TOKEN` environment variable, when it is set on the process that runs
    /// Compose (the `iii compose` daemon, or `iii compose build`), wins over the saved token for
    /// the default registry.
    Login(LoginCli),
    /// Remove the token saved for a worker registry.
    Logout(RegistryTarget),
    /// Show the organization, kind, access and expiry of the token this shell selects for a
    /// worker registry.
    ///
    /// For the default registry, that is `III_REGISTRY_TOKEN` when it is set here, and the saved
    /// token otherwise. Compose selects its own the same way, from its own environment.
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
    /// The token by its first characters (`wdt_AbCd…`), as [`RegistryToken::redacted`] shows
    /// it. [`token_identity`] puts it here in place of the prefix the registry sent, which could
    /// be the whole token; `None` for a token whose first characters tell it apart from no other.
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

impl TokenIdentity {
    /// The identity as it may be printed: the token shown by its prefix computed here, never by
    /// what the registry sent, and, should another field echo the token, by its prefix there too.
    fn redacted(mut self, token: &RegistryToken, auth: &RegistryAuth) -> Self {
        let scrub = |text: &mut String| *text = auth.scrub(text);
        scrub(&mut self.kind);
        scrub(&mut self.org.handle);
        self.org.name.iter_mut().for_each(scrub);
        match &mut self.access {
            // The registry's word for every worker, whatever the token's own text.
            TokenAccess::Named(all) if all == "all" => {}
            TokenAccess::Named(name) => scrub(name),
            TokenAccess::Workers { workers } => workers.iter_mut().for_each(scrub),
        }
        self.expires_at.iter_mut().for_each(scrub);
        self.prefix = Some(token.redacted()).filter(|shown| shown != "…");
        self
    }
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
    /// The request was redirected to another origin, whose answer says nothing of the token.
    #[error(
        "the registry at {registry} redirected the request about {token} to another origin, so \
         its answer cannot confirm the token"
    )]
    Redirected { registry: String, token: String },
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
            Self::Redirected { .. } => format!(
                "Give --registry the origin that answers /tokens/self itself (the default is \
                 {DEFAULT_REGISTRY}), then run `{again}` again."
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
/// redirect to another one, and only that origin's answer counts: a redirect elsewhere is an
/// error. `reader` says which command asks, for the messages.
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
    // Only the registry itself can confirm the token: an answer from wherever a redirect led
    // says nothing of it, whatever its status and body.
    if !auth.is_registry(response.url()) {
        return Err(IdentityError::Redirected {
            registry,
            token: described,
        });
    }
    let status = response.status();
    if status == reqwest::StatusCode::UNAUTHORIZED {
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
        .map(|identity| identity.redacted(token, &auth))
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
    /// Why `III_REGISTRY_TOKEN`, set in this shell for this registry, is no token: wherever
    /// Compose reads the same value, it refuses the registry before it reads the token just
    /// saved.
    pub environment_invalid: Option<&'static str>,
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
/// never overwritten, as that would drop the tokens of other registries it may hold. Nor is a
/// symbolic link, which a save would replace rather than update.
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
    store.check_writable()?;
    let identity = token_identity(&origin, &token, TokenSource::Given, Reader::Login).await?;
    let tightened = store.save(&origin, &token)?;
    let environment = Credentials::from_sources(environment, None);
    Ok(LoggedIn {
        environment_wins: environment.environment_applies_to(&origin),
        environment_invalid: environment.environment_problem(&origin),
        origin,
        identity,
        path: store.path().to_path_buf(),
        tightened,
    })
}

/// Removes the token `store` holds for `registry`. A file that cannot be read is left as it is.
pub(crate) fn logout(registry: &str, store: &CredentialStore) -> Result<Removed, LoginError> {
    let origin = registry_origin(registry)?;
    Ok(store.remove(&origin)?)
}

/// Who the token this shell selects for `registry` belongs to, and where it came from. Compose
/// uses the same token only when its own environment selects it too.
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
    if let Err(error) = store.check_writable() {
        let message = match error {
            CredentialsError::LinkedFile { .. } => {
                format!("{error}. `iii registry login` stopped before asking for the token.")
            }
            _ => format!(
                "{error}. `iii registry login` never overwrites a credentials file it cannot \
                 read, as that would drop the tokens it holds for other registries: it stopped \
                 before asking for the token."
            ),
        };
        return fail(&message, error.fix(Recovery::LoginAgain(&command)));
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
        crate::credentials::environment_token(),
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
            if let Some(note) = environment_note(&done) {
                println!("{note}");
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
    let environment = Credentials::from_sources(crate::credentials::environment_token(), None);
    if let Some(note) = logout_note(&origin, &environment) {
        println!("{note}");
    }
    0
}

/// What `logout` says about `III_REGISTRY_TOKEN`, read as Compose reads it: that it still
/// applies to `origin`, or that it is set but no token.
fn logout_note(origin: &RegistryOrigin, environment: &Credentials) -> Option<String> {
    if let Some(reason) = environment.environment_problem(origin) {
        return Some(format!(
            "{TOKEN_ENV} is still set in this shell, but it {reason}. Wherever Compose reads the \
             same value, it sends no request to {origin} at all: set it to the whole token, or \
             unset it, {WHERE_COMPOSE_RUNS}."
        ));
    }
    environment.environment_applies_to(origin).then(|| {
        format!(
            "{TOKEN_ENV} is still set in this shell, and still applies wherever it is set (on \
             the `iii compose` daemon process, for instance). Unset it there to stop using it."
        )
    })
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

/// What `login` says about `III_REGISTRY_TOKEN` once the token is saved: that the variable wins
/// over it, or that an invalid one keeps Compose from reaching the registry at all.
fn environment_note(done: &LoggedIn) -> Option<String> {
    if let Some(reason) = done.environment_invalid {
        return Some(format!(
            "Warning: {TOKEN_ENV} is set in this shell, but it {reason}. Wherever Compose reads \
             the same value, it sends no request to {} at all rather than use the token just \
             saved: set it to the whole token, or unset it, {WHERE_COMPOSE_RUNS}.",
            done.origin
        ));
    }
    done.environment_wins.then(|| {
        format!(
            "Note: {TOKEN_ENV} is set in this shell. Where it is set (on the `iii compose` daemon \
             process, for instance), it wins over the saved token."
        )
    })
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
             this shell, while Compose reads it from its own process: set it {WHERE_COMPOSE_RUNS}."
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
    println!("{}", identity_lines(identity).join("\n"));
}

/// The identity of a token, one field per line, from what [`token_identity`] returns.
fn identity_lines(identity: &TokenIdentity) -> Vec<String> {
    let mut lines = vec![format!("Kind: {}", printable(&identity.kind))];
    if let Some(prefix) = &identity.prefix {
        lines.push(format!("Token: {}", printable(prefix)));
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
    lines.push(format!("Access: {access}"));
    lines.push(format!(
        "Expires: {}",
        identity
            .expires_at
            .as_deref()
            .map(printable)
            .unwrap_or_else(|| "never".to_string())
    ));
    lines
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

    /// A linked credentials file stops `login` before the token is sent: the save would
    /// replace the link and leave the file it points to as it was.
    #[cfg(unix)]
    #[tokio::test]
    async fn login_refuses_a_linked_file_before_sending_the_token() {
        let server = registry(200, identity_body()).await;
        let (home, store) = store();
        std::fs::create_dir_all(store.path().parent().unwrap()).unwrap();
        let target = home.path().join("shared.toml");
        std::fs::write(&target, "").unwrap();
        std::os::unix::fs::symlink(&target, store.path()).unwrap();
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
                LoginError::Credentials(CredentialsError::LinkedFile { .. })
            ),
            "{error:?}"
        );
        assert!(server.received_requests().await.unwrap().is_empty());
        assert_eq!(std::fs::read_to_string(&target).unwrap(), "");
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

    /// Only the registry can confirm a token: a `200` from wherever a redirect led saves nothing,
    /// while a redirect within the registry is followed.
    #[tokio::test]
    async fn an_answer_from_another_origin_saves_nothing() {
        let elsewhere = registry(200, identity_body()).await;
        let server = MockServer::start().await;
        Mock::given(matchers::path("/tokens/self"))
            .respond_with(
                ResponseTemplate::new(302)
                    .insert_header("location", format!("{}/tokens/self", elsewhere.uri())),
            )
            .mount(&server)
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
        assert!(
            matches!(
                error,
                LoginError::Identity(IdentityError::Redirected { .. })
            ),
            "{error:?}"
        );
        assert_eq!(
            error.to_string(),
            format!(
                "the registry at {} redirected the request about the token wdt_ExpZ… given to \
                 `iii registry login` to another origin, so its answer cannot confirm the token",
                server.uri()
            )
        );
        assert!(
            error
                .fix(Recovery::LoginAgain("iii registry login"))
                .unwrap()
                .starts_with("Give --registry the origin that answers /tokens/self itself"),
            "{error:?}"
        );
        assert!(!store.path().exists());
        let followed = elsewhere.received_requests().await.unwrap();
        assert_eq!(followed.len(), 1);
        assert!(followed[0].headers.get("authorization").is_none());

        let moved = MockServer::start().await;
        Mock::given(matchers::path("/tokens/self"))
            .respond_with(ResponseTemplate::new(308).insert_header("location", "/v2/tokens/self"))
            .mount(&moved)
            .await;
        Mock::given(matchers::path("/v2/tokens/self"))
            .respond_with(ResponseTemplate::new(200).set_body_json(identity_body()))
            .mount(&moved)
            .await;
        let done = login(
            &moved.uri(),
            RegistryToken::new("wdt_valid").unwrap(),
            &store,
            None,
        )
        .await
        .unwrap();
        assert_eq!(done.identity.org.handle, "acme");
        assert_eq!(
            store.read().unwrap()[&RegistryOrigin::parse(&moved.uri()).unwrap()].expose_secret(),
            "wdt_valid"
        );
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

    /// An invalid `III_REGISTRY_TOKEN` does not stop the login, but Compose refuses the default
    /// registry before it reads the saved token, so the login says so and where to fix it.
    #[tokio::test]
    async fn a_saved_token_an_invalid_variable_blocks_is_named_after_login() {
        let server = registry(200, identity_body()).await;
        let (_home, store) = store();
        let done = login(
            &server.uri(),
            RegistryToken::new("wdt_valid").unwrap(),
            &store,
            Some("wdt_half pasted".into()),
        )
        .await
        .unwrap();
        let origin = RegistryOrigin::parse(&server.uri()).unwrap();
        assert_eq!(store.read().unwrap()[&origin].expose_secret(), "wdt_valid");
        // The variable concerns the default registry only.
        assert_eq!(done.environment_invalid, None);
        assert_eq!(environment_note(&done), None);

        // What `login` finds for the default registry, which no test may reach.
        let default = RegistryOrigin::default_registry();
        let for_default = |environment: &str, done: LoggedIn| {
            let credentials = Credentials::from_sources(Some(environment.into()), None);
            LoggedIn {
                origin: default.clone(),
                environment_wins: credentials.environment_applies_to(&default),
                environment_invalid: credentials.environment_problem(&default),
                ..done
            }
        };
        let done = for_default("wdt_half pasted", done);
        assert!(!done.environment_wins);
        let note = environment_note(&done).unwrap();
        assert_eq!(
            note,
            "Warning: III_REGISTRY_TOKEN is set in this shell, but it holds a character that is \
             not visible ASCII. Wherever Compose reads the same value, it sends no request to \
             https://api.workers.iii.dev at all rather than use the token just saved: set it to \
             the whole token, or unset it, where Compose runs (on the `iii compose` daemon \
             process, then restarted, or in the shell that runs `iii compose build`)."
        );
        assert!(!note.contains("half"), "{note}");

        let done = for_default("wdt_environment", done);
        assert_eq!(
            environment_note(&done).unwrap(),
            "Note: III_REGISTRY_TOKEN is set in this shell. Where it is set (on the `iii compose` \
             daemon process, for instance), it wins over the saved token."
        );
    }

    /// The token shows by the prefix computed here: a registry that echoes the whole token, as
    /// its prefix or in any other field, never gets it printed.
    #[tokio::test]
    async fn a_registry_echoing_the_token_never_gets_it_printed() {
        let server = registry(
            200,
            serde_json::json!({
                "kind": "download",
                "org": {"handle": "acme", "name": format!("Acme {EXPIRED}")},
                "prefix": EXPIRED,
                "access": {"workers": ["@acme/ade", EXPIRED]},
                "expires_at": EXPIRED
            }),
        )
        .await;
        let (_home, store) = store();
        let done = login(
            &server.uri(),
            RegistryToken::new(EXPIRED).unwrap(),
            &store,
            None,
        )
        .await
        .unwrap();
        assert_eq!(done.identity.prefix.as_deref(), Some("wdt_ExpZ…"));
        let lines = identity_lines(&done.identity);
        assert_eq!(
            lines,
            [
                "Kind: download",
                "Token: wdt_ExpZ…",
                "Access: @acme/ade, wdt_ExpZ…",
                "Expires: wdt_ExpZ…"
            ]
        );
        assert_eq!(done.identity.org.name.as_deref(), Some("Acme wdt_ExpZ…"));
        assert!(!format!("{:?}", done.identity).contains("expiredSecret"));

        // A token whose first characters tell it from no other shows no prefix at all, rather
        // than the one the registry sent.
        let jwt = "eyJhbGciOiJFUzI1NiJ9.eyJzdWIiOiJ0ZWFtIn0.c2ln";
        let server = registry(
            200,
            serde_json::json!({
                "kind": "cloud",
                "org": {"handle": "acme"},
                "prefix": jwt,
                "access": "all"
            }),
        )
        .await;
        let credentials = Credentials::for_tests(
            "https://api.workers.iii.dev",
            None,
            &[(server.uri().as_str(), jwt)],
        );
        let (identity, _) = whoami(&server.uri(), &credentials).await.unwrap().unwrap();
        assert_eq!(identity.prefix, None);
        assert!(
            !identity_lines(&identity)
                .iter()
                .any(|line| line.contains("eyJ")),
            "{identity:?}"
        );
    }

    /// `all` is the registry's word for every worker, not an echo of a token that happens to
    /// read `all`.
    #[tokio::test]
    async fn the_all_access_survives_a_token_that_reads_all() {
        let server = registry(
            200,
            serde_json::json!({
                "kind": "download",
                "org": {"handle": "acme"},
                "access": "all"
            }),
        )
        .await;
        let (_home, store) = store();
        let done = login(
            &server.uri(),
            RegistryToken::new("all").unwrap(),
            &store,
            None,
        )
        .await
        .unwrap();
        assert_eq!(done.identity.access, TokenAccess::Named("all".into()));
        assert!(
            identity_lines(&done.identity).contains(&"Access: every worker of @acme".to_string()),
            "{:?}",
            done.identity
        );
    }

    /// `logout` reads `III_REGISTRY_TOKEN` as Compose does: a value that is no token is named
    /// as such, and only the default registry is concerned.
    #[test]
    fn logout_names_a_variable_that_still_applies_or_is_no_token() {
        let default = RegistryOrigin::default_registry();
        let other = RegistryOrigin::parse("https://registry.example").unwrap();
        let invalid = Credentials::from_sources(Some("wdt_half pasted".into()), None);
        assert_eq!(
            logout_note(&default, &invalid).unwrap(),
            "III_REGISTRY_TOKEN is still set in this shell, but it holds a character that is not \
             visible ASCII. Wherever Compose reads the same value, it sends no request to \
             https://api.workers.iii.dev at all: set it to the whole token, or unset it, where \
             Compose runs (on the `iii compose` daemon process, then restarted, or in the shell \
             that runs `iii compose build`)."
        );
        let valid = Credentials::from_sources(Some("wdt_environment".into()), None);
        assert!(
            logout_note(&default, &valid)
                .unwrap()
                .starts_with("III_REGISTRY_TOKEN is still set in this shell, and still applies")
        );
        for credentials in [&invalid, &valid, &Credentials::from_sources(None, None)] {
            assert_eq!(logout_note(&other, credentials), None);
        }
        assert_eq!(
            logout_note(&default, &Credentials::from_sources(None, None)),
            None
        );
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
