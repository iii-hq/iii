// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

//! Registry tokens: where they come from and which origin each one belongs to.
//!
//! A token belongs to exactly one registry origin (scheme, host and port), never to a host name a
//! compose file or a lock happens to mention:
//!
//! - `III_REGISTRY_TOKEN` belongs to [`DEFAULT_REGISTRY`] only. A reference that names another
//!   host never receives it.
//! - `~/.iii/credentials.toml` holds one token per origin, written by `iii registry login`:
//!
//!   ```toml
//!   [registries."https://api.workers.iii.dev"]
//!   token = "wdt_…"
//!   ```
//!
//! The environment variable wins over the file. Both are read again for every registry
//! operation, so a login takes effect without restarting the Compose daemon. The variable has to
//! be set on the process that runs Compose (the `iii compose` daemon, or `iii compose build`),
//! not on the engine nor the shell that types `iii call`. Compose keeps it to itself: no process
//! it starts inherits it from the machine ([`CREDENTIAL_ENV`], filtered in `spawn` and
//! `managed_engine`).
//!
//! [`RegistryToken`] never prints its secret: `Debug` and `Display` show at most its first 8
//! characters (`wdt_AbCd…`), the prefix the registry shows to tell tokens apart. The file is
//! written owner-only (`0600` in a `0700` directory) by an atomic replace under a lock that
//! `credentials.toml.lock` holds, and never over a file that cannot be read or a symbolic link:
//! every error says where the problem is (path, line and column) and how to fix it, never quoting
//! the file. On Windows the file inherits the ACL of the user's profile
//! directory, which already limits it to that user and the administrators.

use std::{
    collections::BTreeMap,
    fmt,
    io::Write,
    path::{Path, PathBuf},
};

use crate::registry::DEFAULT_REGISTRY;

/// The environment variable holding a token for [`DEFAULT_REGISTRY`].
pub const TOKEN_ENV: &str = "III_REGISTRY_TOKEN";

/// Every environment variable that carries a registry credential. Compose reads them in its own
/// process and never hands one to a process it starts: a worker, a hook, a build step or the
/// managed engine. A container that needs one declares it in its own `environment` or `env_file`.
pub const CREDENTIAL_ENV: &[&str] = &[TOKEN_ENV];

/// The file `iii registry login` writes, under the iii home directory.
pub const CREDENTIALS_FILE: &str = "credentials.toml";

/// The table of `credentials.toml` that maps an origin to its token.
const REGISTRIES_TABLE: &str = "registries";

/// A token longer than this is not one the registry issues (a JWT is about 1 KiB).
const MAX_TOKEN_LEN: usize = 8 * 1024;

/// Why a value is not a usable registry origin or token, or why `credentials.toml` cannot be
/// read or written. `Display` says what happened in one sentence, never quoting a token or the
/// file's content; [`CredentialsError::fix`] says how to fix it, with commands to copy.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum CredentialsError {
    /// `shown` is the value without its credentials, query and fragment, `None` when it is not
    /// a URL at all: the value itself may hold a password (`https://user:pass@host`).
    #[error("{}", describe_invalid_origin(.shown, .reason))]
    InvalidOrigin {
        shown: Option<String>,
        reason: String,
    },

    #[error("the registry token {reason}")]
    InvalidToken { reason: &'static str },

    #[error("{TOKEN_ENV} {reason}")]
    InvalidEnvironmentToken { reason: &'static str },

    /// `credentials.toml` exists but cannot be used.
    #[error("{}", describe_file_problem(.path, .problem))]
    InvalidFile { path: PathBuf, problem: FileProblem },

    /// A token could not be saved or removed: `path` is the file or directory that refused.
    #[error("{}", describe_write_failure(.path, .step, .message))]
    WriteFailed {
        path: PathBuf,
        step: WriteStep,
        kind: std::io::ErrorKind,
        message: String,
        owner: Option<Ownership>,
    },

    /// A token could not be saved or removed because the file is a symbolic link: replacing it
    /// would replace the link and leave `target`, the file it points to, as it was.
    #[error(
        "{} is a symbolic link to {}: iii reads through it, but never saves or removes a token \
         through it, as replacing the link would leave {} as it was",
        .path.display(),
        .target.display(),
        .target.display()
    )]
    LinkedFile { path: PathBuf, target: PathBuf },

    #[error("there is no home directory to keep ~/.iii/{CREDENTIALS_FILE} in (HOME is not set)")]
    NoHomeDirectory,
}

/// What is wrong with `credentials.toml`. Lines and columns count from 1; nothing here holds any
/// of the file's content.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FileProblem {
    /// It is not TOML: the first syntax error.
    Syntax { line: usize, column: usize },
    /// `registries` is not a table.
    RegistriesNotATable { line: usize },
    /// A key under `registries` is not an origin such as `https://api.workers.iii.dev`.
    NotAnOrigin { line: usize, reason: String },
    /// An entry under `registries` is not a table.
    EntryNotATable { line: usize },
    /// An entry has no `token`.
    NoToken { line: usize },
    /// A `token` is not a string.
    TokenNotAString { line: usize },
    /// A `token` is a string no registry issues.
    BadToken { line: usize, reason: &'static str },
    /// The path is a directory.
    Directory,
    /// The path is neither a file nor a directory, such as a named pipe, which a read would
    /// wait on forever: `kind` says what it is.
    NotAFile { kind: &'static str },
    /// The path is a symbolic link to nothing.
    BrokenSymlink { target: PathBuf },
    /// Reading it failed.
    Unreadable {
        kind: std::io::ErrorKind,
        message: String,
        owner: Option<Ownership>,
    },
}

impl FileProblem {
    /// The line to fix, when the problem is on one.
    pub fn line(&self) -> Option<usize> {
        match self {
            Self::Syntax { line, .. }
            | Self::RegistriesNotATable { line }
            | Self::NotAnOrigin { line, .. }
            | Self::EntryNotATable { line }
            | Self::NoToken { line }
            | Self::TokenNotAString { line }
            | Self::BadToken { line, .. } => Some(*line),
            Self::Directory
            | Self::NotAFile { .. }
            | Self::BrokenSymlink { .. }
            | Self::Unreadable { .. } => None,
        }
    }
}

/// Who owns a file iii could not use, and who iii runs as.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Ownership {
    pub owner: u32,
    pub you: u32,
}

/// The step of saving or removing a token that failed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WriteStep {
    /// Creating the directory the file lives in.
    CreateDirectory,
    /// Taking group and other access away from that directory, whose mode was `from`.
    Tighten { from: u32 },
    /// The directory's path is something else.
    NotADirectory,
    /// Creating the temporary file the new contents go to.
    Create,
    /// Writing and flushing it.
    Write,
    /// Renaming it over the file.
    Replace,
    /// Locking the file beside it that keeps two saves or removals from running at once.
    Lock,
}

/// Which command met a credentials problem, so its fix says what to run once it is out of the
/// way.
#[derive(Debug, Clone, Copy)]
pub enum Recovery<'a> {
    /// A Compose operation or `whoami`: fixing the file is enough. Starting over means signing
    /// in again with this command (`iii registry login`, with `--registry` when needed).
    SignIn(&'a str),
    /// This `iii registry login` itself, to run again once the file is fixed.
    LoginAgain(&'a str),
    /// This `iii registry logout`, to run again once the file is fixed.
    SignOut(&'a str),
}

/// `credentials.toml`'s directory was opened to other users and has been closed: its mode was
/// `from` and is now `0700`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tightened {
    pub directory: PathBuf,
    pub from: u32,
}

impl fmt::Display for Tightened {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Changed the permissions of {} from {:04o} ({}) to 0700 (drwx------), so that only \
             you can open it: it holds your registry tokens.",
            self.directory.display(),
            self.from & 0o7777,
            symbolic_mode(self.from),
        )
    }
}

/// What [`CredentialStore::remove`] did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Removed {
    /// A token was saved for the origin and is gone.
    pub removed: bool,
    pub tightened: Option<Tightened>,
}

fn describe_invalid_origin(shown: &Option<String>, reason: &str) -> String {
    match shown {
        Some(shown) => format!("'{shown}' is not a registry URL: {reason}"),
        None => format!("the registry is not a valid URL: {reason}"),
    }
}

fn describe_file_problem(path: &Path, problem: &FileProblem) -> String {
    let path = path.display();
    let invalid = format!("{path} is not a valid credentials file");
    match problem {
        FileProblem::Syntax { line, column } => {
            format!("{path} is not valid TOML: syntax error at line {line}, column {column}")
        }
        FileProblem::RegistriesNotATable { line } => {
            format!("{invalid}: `{REGISTRIES_TABLE}` on line {line} is not a table")
        }
        FileProblem::NotAnOrigin { line, reason } => format!(
            "{invalid}: the registry key on line {line} is not an origin such as \
             \"{DEFAULT_REGISTRY}\" ({reason})"
        ),
        FileProblem::EntryNotATable { line } => {
            format!("{invalid}: the registry on line {line} is not a table holding a `token`")
        }
        FileProblem::NoToken { line } => {
            format!("{invalid}: the registry on line {line} has no `token`")
        }
        FileProblem::TokenNotAString { line } => {
            format!("{invalid}: the `token` on line {line} is not a quoted string")
        }
        FileProblem::BadToken { line, reason } => {
            format!("{invalid}: the `token` on line {line} {reason}")
        }
        FileProblem::Directory => format!("{path} is a directory, not a credentials file"),
        FileProblem::NotAFile { kind } => format!("{path} is {kind}, not a credentials file"),
        FileProblem::BrokenSymlink { target } => format!(
            "{path} is a symbolic link to {}, which does not exist",
            target.display()
        ),
        FileProblem::Unreadable { message, owner, .. } => {
            format!("cannot read {path}: {message}{}", ownership_note(owner))
        }
    }
}

fn describe_write_failure(path: &Path, step: &WriteStep, message: &str) -> String {
    let path = path.display();
    match step {
        WriteStep::CreateDirectory => format!("cannot create the directory {path}: {message}"),
        WriteStep::Tighten { from } => format!(
            "cannot make {path} private: its mode is {:04o}, and iii closes it to other users \
             (0700) before it keeps a token there, but changing it failed: {message}",
            from & 0o7777
        ),
        WriteStep::NotADirectory => {
            format!("{path} exists and is not a directory, so no {CREDENTIALS_FILE} can go in it")
        }
        WriteStep::Create => format!("cannot create a file in {path}: {message}"),
        WriteStep::Write => format!("cannot write the new {CREDENTIALS_FILE} in {path}: {message}"),
        WriteStep::Replace => format!("cannot replace {path}: {message}"),
        WriteStep::Lock => format!(
            "cannot lock {path}, which keeps two iii processes from saving or removing tokens at \
             once: {message}"
        ),
    }
}

fn ownership_note(owner: &Option<Ownership>) -> String {
    match owner {
        Some(Ownership { owner, you }) if owner != you => {
            format!(" (it belongs to uid {owner}, and iii runs as uid {you})")
        }
        _ => String::new(),
    }
}

impl CredentialsError {
    /// How to fix it, with commands to copy that name the real path. `None` for an error that
    /// needs no more than its message (an invalid `--registry`, a token rejected as typed).
    pub fn fix(&self, recovery: Recovery<'_>) -> Option<String> {
        match self {
            Self::InvalidFile { path, problem } => Some(file_fix(path, problem, recovery)),
            Self::WriteFailed {
                path,
                step,
                kind,
                owner,
                ..
            } => Some(write_fix(path, *step, *kind, owner)),
            Self::LinkedFile { path, target } => Some(link_fix(path, target, recovery)),
            Self::NoHomeDirectory => Some(format!(
                "Set HOME to your home directory, or give Compose a token through {TOKEN_ENV} in \
                 the environment of the process that runs it."
            )),
            Self::InvalidOrigin { .. }
            | Self::InvalidToken { .. }
            | Self::InvalidEnvironmentToken { .. } => None,
        }
    }
}

fn file_fix(path: &Path, problem: &FileProblem, recovery: Recovery<'_>) -> String {
    let aside = commands::move_aside(path);
    let start_over = match recovery {
        Recovery::SignIn(login) | Recovery::LoginAgain(login) => format!(
            "move the file aside and sign in again, which drops the tokens saved for every other \
             registry (sign in to those again too): `{aside} && {login}`"
        ),
        Recovery::SignOut(_) => format!(
            "move the whole file aside, which signs you out of every registry at once: `{aside}`"
        ),
    };
    let again = match recovery {
        Recovery::SignIn(_) => String::new(),
        Recovery::LoginAgain(command) | Recovery::SignOut(command) => {
            format!(", then run `{command}` again")
        }
    };
    match problem {
        FileProblem::Directory | FileProblem::NotAFile { .. } => {
            format!("{}.", capitalize(&start_over))
        }
        FileProblem::BrokenSymlink { target } => {
            let remove = commands::remove(path);
            match recovery {
                Recovery::SignIn(login) | Recovery::LoginAgain(login) => format!(
                    "Restore {}, or remove the link (what it points to is not touched) and sign \
                     in again: `{remove} && {login}`.",
                    target.display()
                ),
                Recovery::SignOut(_) => format!(
                    "Restore {}, or remove the link, which signs you out of every registry at \
                     once: `{remove}`.",
                    target.display()
                ),
            }
        }
        FileProblem::Unreadable {
            kind: std::io::ErrorKind::PermissionDenied,
            owner,
            ..
        } => match owner {
            Some(Ownership { owner, you }) if owner != you => format!(
                "Make it yours again: `{}`{again}. Or {start_over}.",
                commands::reclaim(path)
            ),
            _ => format!(
                "Let yourself read it: `{}`{again}. Or {start_over}.",
                commands::owner_only(path)
            ),
        },
        FileProblem::Unreadable { .. } => format!("{}.", capitalize(&start_over)),
        _ => {
            let line = problem.line().unwrap_or(1);
            format!(
                "Fix line {line} of the file: `{}`{again}. Or {start_over}.",
                commands::edit(path, line)
            )
        }
    }
}

fn link_fix(path: &Path, target: &Path, recovery: Recovery<'_>) -> String {
    let edit = commands::edit(target, 1);
    let remove = commands::remove(path);
    match recovery {
        Recovery::SignIn(command) | Recovery::LoginAgain(command) => format!(
            "Save the token in {} itself (`{edit}`), or remove the link (what it points to is not \
             touched) and run `{command}` again, for a file of this machine's own: \
             `{remove} && {command}`.",
            target.display()
        ),
        Recovery::SignOut(_) => format!(
            "Remove the token from {} itself (`{edit}`), or remove the link (what it points to is \
             not touched), which signs you out of every registry it holds: `{remove}`.",
            target.display()
        ),
    }
}

fn write_fix(
    path: &Path,
    step: WriteStep,
    kind: std::io::ErrorKind,
    owner: &Option<Ownership>,
) -> String {
    use std::io::ErrorKind;
    let quoted = commands::quote(path);
    if kind == ErrorKind::ReadOnlyFilesystem {
        return format!(
            "{} is on a read-only file system, so no token can be saved on this machine. Give \
             Compose the token through {TOKEN_ENV} in the environment of the process that runs it \
             instead.",
            path.display()
        );
    }
    if step == WriteStep::NotADirectory {
        return format!(
            "Move it aside, then run the command again: `{}`.",
            commands::move_aside(path)
        );
    }
    match (kind, owner) {
        (ErrorKind::PermissionDenied, Some(Ownership { owner, you })) if owner != you => format!(
            "{} belongs to uid {owner}, not to you (uid {you}). Take it back, then run the \
             command again: `{}`.",
            path.display(),
            commands::reclaim_directory(path)
        ),
        (ErrorKind::PermissionDenied, _) => format!(
            "Give yourself access to it, then run the command again: `{}`.",
            commands::owner_access(path)
        ),
        _ => format!(
            "Check it (`{}`), then run the command again.",
            commands::inspect(&quoted)
        ),
    }
}

fn capitalize(text: &str) -> String {
    let mut chars = text.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}

/// `drwxrwxr-x` for a directory mode.
fn symbolic_mode(mode: u32) -> String {
    let mut text = String::from("d");
    for shift in [6, 3, 0] {
        let bits = (mode >> shift) & 0o7;
        text.push(if bits & 0o4 != 0 { 'r' } else { '-' });
        text.push(if bits & 0o2 != 0 { 'w' } else { '-' });
        text.push(if bits & 0o1 != 0 { 'x' } else { '-' });
    }
    text
}

/// The shell commands a fix suggests, ready to copy: the path quoted for this platform's shell.
pub(crate) mod commands {
    use std::path::Path;

    /// `path`, quoted only when the shell needs it.
    pub(crate) fn quote(path: &Path) -> String {
        let text = path.display().to_string();
        #[cfg(windows)]
        {
            if text.contains([' ', '&', '(', ')', '^', '%', '!', ',', ';']) {
                format!("\"{text}\"")
            } else {
                text
            }
        }
        #[cfg(not(windows))]
        {
            let plain = !text.is_empty()
                && text.chars().all(|c| {
                    c.is_ascii_alphanumeric()
                        || matches!(c, '/' | '.' | '_' | '-' | '+' | ',' | ':' | '@' | '%' | '=')
                });
            if plain {
                text
            } else {
                format!("'{}'", text.replace('\'', r"'\''"))
            }
        }
    }

    fn backup(path: &Path) -> String {
        let mut name = path.as_os_str().to_os_string();
        name.push(".bak");
        quote(Path::new(&name))
    }

    pub(crate) fn move_aside(path: &Path) -> String {
        #[cfg(windows)]
        return format!("move {} {}", quote(path), backup(path));
        #[cfg(not(windows))]
        format!("mv {} {}", quote(path), backup(path))
    }

    pub(crate) fn remove(path: &Path) -> String {
        #[cfg(windows)]
        return format!("del {}", quote(path));
        #[cfg(not(windows))]
        format!("rm {}", quote(path))
    }

    /// Opens `path` at `line`; on Windows, Notepad has no line argument.
    pub(crate) fn edit(path: &Path, line: usize) -> String {
        #[cfg(windows)]
        {
            let _ = line;
            format!("notepad {}", quote(path))
        }
        #[cfg(not(windows))]
        format!("nano +{line} {}", quote(path))
    }

    pub(crate) fn reclaim(path: &Path) -> String {
        #[cfg(windows)]
        return format!("icacls {} /setowner %USERNAME%", quote(path));
        #[cfg(not(windows))]
        format!(
            "sudo chown \"$(id -un)\" {path} && chmod 600 {path}",
            path = quote(path)
        )
    }

    pub(crate) fn reclaim_directory(path: &Path) -> String {
        #[cfg(windows)]
        return format!("icacls {} /setowner %USERNAME%", quote(path));
        #[cfg(not(windows))]
        format!(
            "sudo chown \"$(id -un)\" {path} && chmod 700 {path}",
            path = quote(path)
        )
    }

    pub(crate) fn owner_only(path: &Path) -> String {
        #[cfg(windows)]
        return format!("icacls {} /grant %USERNAME%:F", quote(path));
        #[cfg(not(windows))]
        format!("chmod 600 {}", quote(path))
    }

    pub(crate) fn owner_access(path: &Path) -> String {
        #[cfg(windows)]
        return format!("icacls {} /grant %USERNAME%:F", quote(path));
        #[cfg(not(windows))]
        format!("chmod u+rwx {}", quote(path))
    }

    pub(crate) fn inspect(quoted: &str) -> String {
        #[cfg(windows)]
        return format!("dir {quoted}");
        #[cfg(not(windows))]
        format!("ls -ld {quoted}")
    }
}

/// What a path that is neither a file nor a directory is, for a message.
fn special_file_kind(file_type: &std::fs::FileType) -> &'static str {
    #[cfg(unix)]
    {
        use std::os::unix::fs::FileTypeExt;
        if file_type.is_fifo() {
            return "a named pipe";
        }
        if file_type.is_socket() {
            return "a socket";
        }
        if file_type.is_char_device() {
            return "a character device";
        }
        if file_type.is_block_device() {
            return "a block device";
        }
    }
    let _ = file_type;
    "a special file"
}

/// Who owns `path`, and who iii runs as. `None` off unix, or when `path` cannot be examined.
fn ownership(path: &Path) -> Option<Ownership> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let metadata = std::fs::symlink_metadata(path).ok()?;
        Some(Ownership {
            owner: metadata.uid(),
            you: nix::unistd::geteuid().as_raw(),
        })
    }
    #[cfg(not(unix))]
    {
        let _ = path;
        None
    }
}

/// A registry origin: scheme, host and port, as `https://api.workers.iii.dev` (the default port
/// is left out). Tokens are keyed and sent by origin.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RegistryOrigin {
    serialized: String,
    /// `https`, or plain `http` to a loopback host: a token may travel over it.
    secure: bool,
}

impl RegistryOrigin {
    /// Parses a registry base URL given by an operator (`--registry`) or a credentials file.
    ///
    /// Only an origin is accepted: `http` or `https`, a host, an optional port, and nothing
    /// else. Credentials (`user@`), a path, a query or a fragment are refused rather than
    /// dropped, so a typo never silently names another registry.
    pub fn parse(value: &str) -> Result<Self, CredentialsError> {
        let url =
            url::Url::parse(value.trim()).map_err(|error| CredentialsError::InvalidOrigin {
                shown: None,
                reason: error.to_string(),
            })?;
        // The value as an error may show it: never its credentials, query or fragment.
        let invalid = |reason: &str| {
            let mut shown = url.clone();
            let _ = shown.set_username("");
            let _ = shown.set_password(None);
            shown.set_query(None);
            shown.set_fragment(None);
            CredentialsError::InvalidOrigin {
                shown: Some(shown.to_string()),
                reason: reason.to_string(),
            }
        };
        if !url.username().is_empty() || url.password().is_some() {
            return Err(invalid("it carries credentials"));
        }
        if !matches!(url.path(), "" | "/") || url.query().is_some() || url.fragment().is_some() {
            return Err(invalid(
                "it has a path, query or fragment; give only scheme, host and port",
            ));
        }
        Self::of(&url).ok_or_else(|| invalid("it needs an http or https scheme and a host"))
    }

    /// The origin `url` is sent to. `None` for a URL a token must never go to: not `http` or
    /// `https`, without a host, or carrying credentials of its own.
    pub fn of(url: &url::Url) -> Option<Self> {
        if !matches!(url.scheme(), "http" | "https")
            || !url.username().is_empty()
            || url.password().is_some()
        {
            return None;
        }
        let host = url.host()?;
        let loopback = match host {
            url::Host::Domain(domain) => domain.eq_ignore_ascii_case("localhost"),
            url::Host::Ipv4(address) => address.is_loopback(),
            url::Host::Ipv6(address) => address.is_loopback(),
        };
        Some(Self {
            serialized: url.origin().ascii_serialization(),
            secure: url.scheme() == "https" || loopback,
        })
    }

    /// The origin of [`DEFAULT_REGISTRY`].
    pub fn default_registry() -> Self {
        Self::parse(DEFAULT_REGISTRY).expect("DEFAULT_REGISTRY is an https origin")
    }

    /// `https://host[:port]`, the form `credentials.toml` keys by.
    pub fn as_str(&self) -> &str {
        &self.serialized
    }

    /// Whether a token may be sent here: over `https`, or over plain `http` only to a loopback
    /// host (`localhost`, `127.0.0.0/8`, `::1`).
    pub fn allows_tokens(&self) -> bool {
        self.secure
    }
}

impl fmt::Display for RegistryOrigin {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.serialized)
    }
}

impl fmt::Debug for RegistryOrigin {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("RegistryOrigin")
            .field(&self.serialized)
            .finish()
    }
}

/// An opaque registry token (`wdt_…`, `wmt_…` or an iii cloud JWT).
///
/// `Debug` and `Display` print [`RegistryToken::redacted`]; the secret is reachable only through
/// [`RegistryToken::expose_secret`]. It implements no `Serialize`, so it cannot end up in a lock,
/// a report or a compose file by way of a derived serializer.
#[derive(Clone, PartialEq, Eq)]
pub struct RegistryToken(String);

impl RegistryToken {
    /// Checks a token as given: surrounding whitespace is dropped (a pasted token often ends
    /// with a newline), and what remains must be visible ASCII, so it fits an HTTP header as is.
    pub fn new(value: &str) -> Result<Self, CredentialsError> {
        Self::check(value)
            .map(Self)
            .map_err(|reason| CredentialsError::InvalidToken { reason })
    }

    fn check(value: &str) -> Result<String, &'static str> {
        let value = value.trim();
        if value.is_empty() {
            return Err("is empty");
        }
        if value.len() > MAX_TOKEN_LEN {
            return Err("is too long");
        }
        if !value.bytes().all(|byte| byte.is_ascii_graphic()) {
            return Err("holds a character that is not visible ASCII");
        }
        Ok(value.to_string())
    }

    /// The secret itself, for the `Authorization` header and the credentials file only.
    pub fn expose_secret(&self) -> &str {
        &self.0
    }

    /// A registry token by its first 8 characters (`wdt_AbCd…`): the prefix the registry keeps
    /// and shows on the Download tokens tab to tell tokens apart. A token too short for 8
    /// characters to leave most of it hidden shows only its kind (`wdt_…`), and one without a
    /// kind (an iii cloud JWT, whose first characters are the same for every token) only `…`.
    pub fn redacted(&self) -> String {
        const SHOWN: usize = 8;
        let Some((kind, _)) = self.0.split_once('_').filter(|(kind, _)| {
            (2..=4).contains(&kind.len()) && kind.bytes().all(|byte| byte.is_ascii_lowercase())
        }) else {
            return "…".to_string();
        };
        // Visible ASCII only ([`RegistryToken::check`]), so a byte index is a character index.
        if self.0.len() >= 4 * SHOWN {
            format!("{}…", &self.0[..SHOWN])
        } else {
            format!("{kind}_…")
        }
    }
}

impl fmt::Debug for RegistryToken {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("RegistryToken")
            .field(&self.redacted())
            .finish()
    }
}

impl fmt::Display for RegistryToken {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.redacted())
    }
}

/// Where a token came from, for messages: never the token itself.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TokenSource {
    /// `III_REGISTRY_TOKEN`.
    Environment,
    /// The credentials file at this path.
    File(PathBuf),
    /// Typed or piped into `iii registry login`, not saved yet.
    Given,
}

impl fmt::Display for TokenSource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Environment => write!(f, "{TOKEN_ENV}"),
            Self::File(path) => write!(f, "{}", path.display()),
            Self::Given => f.write_str("the token given to `iii registry login`"),
        }
    }
}

/// The tokens one registry operation may use, read once at its start.
///
/// A source that cannot be used is kept as its error, returned only to an operation that needs
/// it: a broken `credentials.toml` stops every registry it could hold a token for, but not the
/// default registry while `III_REGISTRY_TOKEN` is set, since the variable wins there anyway.
#[derive(Clone, Default)]
pub struct Credentials {
    /// `III_REGISTRY_TOKEN`, bound to `default_origin`.
    environment: Option<RegistryToken>,
    environment_error: Option<CredentialsError>,
    default_origin: Option<RegistryOrigin>,
    saved: BTreeMap<RegistryOrigin, RegistryToken>,
    file_error: Option<CredentialsError>,
    file: Option<PathBuf>,
}

impl fmt::Debug for Credentials {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Credentials")
            .field("environment", &self.environment)
            .field("environment_error", &self.environment_error)
            .field("default_origin", &self.default_origin)
            .field("saved", &self.saved)
            .field("file_error", &self.file_error)
            .field("file", &self.file)
            .finish()
    }
}

impl Credentials {
    /// Reads `III_REGISTRY_TOKEN` and `~/.iii/credentials.toml` as they are now.
    ///
    /// A missing file or home directory is no credentials, not an error. A file that cannot be
    /// used is kept as an error for [`Credentials::token_for`]: silently ignoring it would turn
    /// a typo into a 404.
    pub fn load() -> Self {
        let store = CredentialStore::default_location().ok();
        Self::from_sources(environment_token(), store.as_ref())
    }

    /// [`Credentials::load`] from an explicit environment value and store.
    pub fn from_sources(environment: Option<String>, store: Option<&CredentialStore>) -> Self {
        let (environment, environment_error) = match environment.as_deref().map(str::trim) {
            None | Some("") => (None, None),
            Some(value) => match RegistryToken::check(value) {
                Ok(token) => (Some(RegistryToken(token)), None),
                Err(reason) => (
                    None,
                    Some(CredentialsError::InvalidEnvironmentToken { reason }),
                ),
            },
        };
        let (saved, file_error) = match store.map(CredentialStore::read) {
            None => (BTreeMap::new(), None),
            Some(Ok(saved)) => (saved, None),
            Some(Err(error)) => (BTreeMap::new(), Some(error)),
        };
        Self {
            environment,
            environment_error,
            default_origin: Some(RegistryOrigin::default_registry()),
            saved,
            file_error,
            file: store.map(|store| store.path().to_path_buf()),
        }
    }

    /// No token at all: what an operation without credentials, and every unit test, uses.
    pub fn none() -> Self {
        Self::default()
    }

    /// Test seam: the environment token bound to `default_origin` instead of
    /// [`DEFAULT_REGISTRY`], and `saved` instead of a file.
    #[cfg(test)]
    pub(crate) fn for_tests(
        default_origin: &str,
        environment: Option<&str>,
        saved: &[(&str, &str)],
    ) -> Self {
        Self {
            environment: environment.map(|token| RegistryToken::new(token).unwrap()),
            environment_error: None,
            file_error: None,
            default_origin: Some(RegistryOrigin::parse(default_origin).unwrap()),
            saved: saved
                .iter()
                .map(|(origin, token)| {
                    (
                        RegistryOrigin::parse(origin).unwrap(),
                        RegistryToken::new(token).unwrap(),
                    )
                })
                .collect(),
            file: Some(PathBuf::from("/test/credentials.toml")),
        }
    }

    /// The token for `origin`, and where it came from: `III_REGISTRY_TOKEN` for the default
    /// registry, else the one saved for exactly that origin. `None` when there is none, or for
    /// an origin a token may not travel to (plain `http` to a host that is not loopback).
    ///
    /// An error when the source that would decide is unusable: an invalid `III_REGISTRY_TOKEN`
    /// for the default registry, or a `credentials.toml` that cannot be read for any registry
    /// the variable does not cover. Without the file, iii cannot tell which token belongs to
    /// which registry, so it sends none rather than guess.
    pub fn token_for(
        &self,
        origin: &RegistryOrigin,
    ) -> Result<Option<(&RegistryToken, TokenSource)>, CredentialsError> {
        if !origin.allows_tokens() {
            return Ok(None);
        }
        if self.default_origin.as_ref() == Some(origin) {
            if let Some(error) = &self.environment_error {
                return Err(error.clone());
            }
            if let Some(token) = &self.environment {
                return Ok(Some((token, TokenSource::Environment)));
            }
        }
        if let Some(error) = &self.file_error {
            return Err(error.clone());
        }
        let Some(token) = self.saved.get(origin) else {
            return Ok(None);
        };
        let path = self
            .file
            .clone()
            .unwrap_or_else(|| PathBuf::from(CREDENTIALS_FILE));
        Ok(Some((token, TokenSource::File(path))))
    }

    /// Where the saved tokens are read from, when there is a home directory.
    pub fn file(&self) -> Option<&Path> {
        self.file.as_deref()
    }

    /// Whether `III_REGISTRY_TOKEN` is set and applies to `origin`.
    pub fn environment_applies_to(&self, origin: &RegistryOrigin) -> bool {
        self.environment.is_some() && self.default_origin.as_ref() == Some(origin)
    }

    /// Why `III_REGISTRY_TOKEN` is no token, when it is set and would apply to `origin`: a
    /// registry operation on `origin` then fails before any saved token is read.
    pub fn environment_problem(&self, origin: &RegistryOrigin) -> Option<&'static str> {
        match &self.environment_error {
            Some(CredentialsError::InvalidEnvironmentToken { reason })
                if self.default_origin.as_ref() == Some(origin) =>
            {
                Some(reason)
            }
            _ => None,
        }
    }
}

/// `III_REGISTRY_TOKEN` as this process has it, read the one way Compose, `iii registry login`
/// and `iii registry logout` all read it: `None` when it is unset. A value that is not Unicode
/// stays set, its bad bytes replaced, so that [`Credentials::from_sources`] refuses it rather
/// than take it for unset.
pub fn environment_token() -> Option<String> {
    environment_text(std::env::var_os(TOKEN_ENV))
}

fn environment_text(value: Option<std::ffi::OsString>) -> Option<String> {
    value.map(|value| value.to_string_lossy().into_owned())
}

// A unit test's own credentials. Without one a test has none: it never reads the
// developer's `III_REGISTRY_TOKEN` or `~/.iii/credentials.toml`.
#[cfg(test)]
tokio::task_local! {
    pub(crate) static TEST_CREDENTIALS: Credentials;
}

/// The credentials of the registry operation about to start, read now.
pub(crate) fn current() -> Credentials {
    #[cfg(test)]
    return TEST_CREDENTIALS
        .try_with(Clone::clone)
        .unwrap_or_else(|_| Credentials::none());
    #[cfg(not(test))]
    Credentials::load()
}

/// `credentials.toml`: one token per registry origin.
#[derive(Debug, Clone)]
pub struct CredentialStore {
    path: PathBuf,
}

impl CredentialStore {
    /// `~/.iii/credentials.toml`, beside the rest of iii's per-user state.
    pub fn default_location() -> Result<Self, CredentialsError> {
        let home = dirs::home_dir().ok_or(CredentialsError::NoHomeDirectory)?;
        Ok(Self::at(home.join(".iii").join(CREDENTIALS_FILE)))
    }

    pub fn at(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Every token in the file, by origin; empty when there is no file.
    ///
    /// The file is checked whole: a syntax error, an entry that is not an origin with a `token`
    /// string, or a token no registry issues each make it unusable, with the line and column of
    /// the problem and never its text.
    pub fn read(&self) -> Result<BTreeMap<RegistryOrigin, RegistryToken>, CredentialsError> {
        let Some(text) = self.text()? else {
            return Ok(BTreeMap::new());
        };
        parse_tokens(&text).map_err(|problem| CredentialsError::InvalidFile {
            path: self.path.clone(),
            problem,
        })
    }

    /// The origins a token is saved for.
    pub fn origins(&self) -> Result<Vec<RegistryOrigin>, CredentialsError> {
        Ok(self.read()?.into_keys().collect())
    }

    /// Saves `token` for `origin`, replacing the one there. Other origins and any other content
    /// of the file stay as they were (comments are not kept: the file is machine-written). A
    /// file that cannot be read, or a symbolic link, is never overwritten: the error says how to
    /// fix it. The file is read and replaced under [`CredentialStore::lock`].
    ///
    /// `Some` when the directory had to be closed to other users first.
    pub fn save(
        &self,
        origin: &RegistryOrigin,
        token: &RegistryToken,
    ) -> Result<Option<Tightened>, CredentialsError> {
        // The directory first: the lock lives in it.
        let tightened = private_parent(&self.path)?;
        let _lock = self.lock()?;
        let mut document = self.document()?.unwrap_or_default();
        self.refuse_link()?;
        let registries = document
            .entry(REGISTRIES_TABLE)
            .or_insert_with(|| toml::Value::Table(toml::Table::new()))
            .as_table_mut()
            .expect("read() checked that `registries` is a table");
        // The entry keeps its other fields. One written for the same origin in another
        // spelling is the same registry: it moves under the canonical key, whose own fields
        // win where both have one.
        let mut entry = match registries.remove(origin.as_str()) {
            Some(toml::Value::Table(entry)) => entry,
            _ => toml::Table::new(),
        };
        let spellings: Vec<String> = registries
            .keys()
            .filter(|key| RegistryOrigin::parse(key).is_ok_and(|existing| existing == *origin))
            .cloned()
            .collect();
        for key in spellings {
            if let Some(toml::Value::Table(fields)) = registries.remove(&key) {
                merge_missing(&mut entry, fields);
            }
        }
        entry.insert(
            "token".to_string(),
            toml::Value::String(token.expose_secret().to_string()),
        );
        registries.insert(origin.as_str().to_string(), toml::Value::Table(entry));
        Ok(tightened.or(self.write(&document)?))
    }

    /// Removes the token saved for `origin`. Other origins stay. A file that cannot be read, or
    /// a symbolic link, is left as it is. The file is read and replaced under
    /// [`CredentialStore::lock`].
    pub fn remove(&self, origin: &RegistryOrigin) -> Result<Removed, CredentialsError> {
        let nothing = Removed {
            removed: false,
            tightened: None,
        };
        // Nothing saved for it: nothing to lock or write, so a link or a directory iii may not
        // write in is no obstacle.
        let Some(mut document) = self.document()? else {
            return Ok(nothing);
        };
        if !without(&mut document, origin) {
            return Ok(nothing);
        }
        self.refuse_link()?;
        let _lock = self.lock()?;
        // Again under the lock: another iii may have changed the file since.
        let Some(mut document) = self.document()? else {
            return Ok(nothing);
        };
        if !without(&mut document, origin) {
            return Ok(nothing);
        }
        Ok(Removed {
            removed: true,
            tightened: self.write(&document)?,
        })
    }

    /// The file `save` and `remove` lock: `credentials.toml.lock`, beside it.
    fn lock_path(&self) -> PathBuf {
        let mut name = self.path.as_os_str().to_os_string();
        name.push(".lock");
        PathBuf::from(name)
    }

    /// Holds [`CredentialStore::lock_path`] exclusively until the file returned is dropped, so
    /// that a save or removal in another iii process waits instead of writing back a copy it
    /// read before this one's rename, which would undo it. The directory must exist. `None` on
    /// a file system without locks, where only the atomic rename is left.
    fn lock(&self) -> Result<Option<std::fs::File>, CredentialsError> {
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create(true).truncate(false);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let path = self.lock_path();
        let file = options
            .open(&path)
            .map_err(|error| write_failed(WriteStep::Create, directory_of(&self.path), error))?;
        match file.lock() {
            Ok(()) => Ok(Some(file)),
            Err(error) if error.kind() == std::io::ErrorKind::Unsupported => Ok(None),
            Err(error) => Err(write_failed(WriteStep::Lock, &path, error)),
        }
    }

    /// Refuses a file that is a symbolic link. Saving or removing replaces the file by a rename,
    /// which would replace the link itself and leave the file it points to, maybe shared with
    /// another installation, as it was. Reading through a link is fine.
    fn refuse_link(&self) -> Result<(), CredentialsError> {
        match std::fs::read_link(&self.path) {
            Ok(target) => Err(CredentialsError::LinkedFile {
                path: self.path.clone(),
                target: directory_of(&self.path).join(target),
            }),
            Err(_) => Ok(()),
        }
    }

    /// What [`CredentialStore::save`] refuses before it is given a token: a file it cannot read,
    /// or a symbolic link.
    pub fn check_writable(&self) -> Result<(), CredentialsError> {
        self.read()?;
        self.refuse_link()
    }

    /// The file's text, `None` when there is no file. A directory, a named pipe or another
    /// special file, a link to nothing or a file that cannot be read is an error saying which.
    fn text(&self) -> Result<Option<String>, CredentialsError> {
        let invalid = |problem| CredentialsError::InvalidFile {
            path: self.path.clone(),
            problem,
        };
        let unreadable = |error: std::io::Error| {
            invalid(FileProblem::Unreadable {
                kind: error.kind(),
                message: error.to_string(),
                owner: (error.kind() == std::io::ErrorKind::PermissionDenied)
                    .then(|| ownership(&self.path))
                    .flatten(),
            })
        };
        match std::fs::metadata(&self.path) {
            Ok(metadata) if metadata.is_dir() => return Err(invalid(FileProblem::Directory)),
            // Before it is opened: opening a named pipe waits for a writer, which stalls every
            // registry operation, even one `III_REGISTRY_TOKEN` covers. A link to a file is read.
            Ok(metadata) if !metadata.is_file() => {
                return Err(invalid(FileProblem::NotAFile {
                    kind: special_file_kind(&metadata.file_type()),
                }));
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                // A link whose target is gone is not "no file": a login would replace the link.
                return match std::fs::read_link(&self.path) {
                    Ok(target) => Err(invalid(FileProblem::BrokenSymlink { target })),
                    Err(_) => Ok(None),
                };
            }
            // `~/.iii` is a file, so no credentials file can exist; saving one says how to fix it.
            Err(error) if error.kind() == std::io::ErrorKind::NotADirectory => return Ok(None),
            Err(error) => return Err(unreadable(error)),
        }
        std::fs::read_to_string(&self.path)
            .map(Some)
            .map_err(unreadable)
    }

    /// The file as a table to rewrite, `None` when there is none. Checked by
    /// [`CredentialStore::read`] first, so a file it refuses is never rewritten.
    fn document(&self) -> Result<Option<toml::Table>, CredentialsError> {
        let Some(text) = self.text()? else {
            return Ok(None);
        };
        parse_tokens(&text).map_err(|problem| CredentialsError::InvalidFile {
            path: self.path.clone(),
            problem,
        })?;
        text.parse::<toml::Table>()
            .map(Some)
            .map_err(|error| CredentialsError::InvalidFile {
                path: self.path.clone(),
                problem: syntax_problem(&text, &error),
            })
    }

    fn write(&self, document: &toml::Table) -> Result<Option<Tightened>, CredentialsError> {
        let text = toml::to_string(document).expect("a table of strings serializes");
        write_private(&self.path, text.as_bytes())
    }
}

/// Drops every entry of `document` for `origin`, in any spelling: whether there was one.
fn without(document: &mut toml::Table, origin: &RegistryOrigin) -> bool {
    let Some(registries) = document
        .get_mut(REGISTRIES_TABLE)
        .and_then(toml::Value::as_table_mut)
    else {
        return false;
    };
    let before = registries.len();
    registries
        .retain(|key, _| RegistryOrigin::parse(key).map_or(true, |existing| existing != *origin));
    registries.len() != before
}

/// Adds to `into` what `from` has and it lacks, table by table: `into` wins where both have a
/// value.
fn merge_missing(into: &mut toml::Table, from: toml::Table) {
    for (name, value) in from {
        match (into.get_mut(&name), value) {
            (Some(toml::Value::Table(kept)), toml::Value::Table(more)) => merge_missing(kept, more),
            (Some(_), _) => {}
            (None, value) => {
                into.insert(name, value);
            }
        }
    }
}

/// The tokens in `text`, or what is wrong with it and where. Never quotes it: a parser's own
/// message shows the offending text, which may be a token.
fn parse_tokens(text: &str) -> Result<BTreeMap<RegistryOrigin, RegistryToken>, FileProblem> {
    use toml::de::{DeTable, DeValue};

    let document = DeTable::parse(text).map_err(|error| syntax_problem(text, &error))?;
    let line = |span: std::ops::Range<usize>| position(text, span.start).0;
    let Some((key, registries)) = document
        .get_ref()
        .iter()
        .find(|(key, _)| key.get_ref().as_ref() == REGISTRIES_TABLE)
    else {
        return Ok(BTreeMap::new());
    };
    let DeValue::Table(registries) = registries.get_ref() else {
        return Err(FileProblem::RegistriesNotATable {
            line: line(key.span()),
        });
    };
    let mut tokens = BTreeMap::new();
    for (key, entry) in registries {
        let at = line(key.span());
        let origin =
            RegistryOrigin::parse(key.get_ref()).map_err(|error| FileProblem::NotAnOrigin {
                line: at,
                reason: match error {
                    CredentialsError::InvalidOrigin { reason, .. } => reason,
                    _ => "not an origin".to_string(),
                },
            })?;
        let DeValue::Table(entry) = entry.get_ref() else {
            return Err(FileProblem::EntryNotATable { line: at });
        };
        let Some((_, token)) = entry
            .iter()
            .find(|(key, _)| key.get_ref().as_ref() == "token")
        else {
            return Err(FileProblem::NoToken { line: at });
        };
        let token_line = line(token.span());
        let DeValue::String(value) = token.get_ref() else {
            return Err(FileProblem::TokenNotAString { line: token_line });
        };
        let token = RegistryToken::check(value).map_err(|reason| FileProblem::BadToken {
            line: token_line,
            reason,
        })?;
        tokens.insert(origin, RegistryToken(token));
    }
    Ok(tokens)
}

fn syntax_problem(text: &str, error: &toml::de::Error) -> FileProblem {
    let (line, column) = error
        .span()
        .map(|span| position(text, span.start))
        .unwrap_or((1, 1));
    FileProblem::Syntax { line, column }
}

/// The 1-based line and column (in characters) of byte `offset` in `text`.
fn position(text: &str, offset: usize) -> (usize, usize) {
    let mut offset = offset.min(text.len());
    while !text.is_char_boundary(offset) {
        offset -= 1;
    }
    let before = &text[..offset];
    let line = before.matches('\n').count() + 1;
    let column = before
        .rsplit('\n')
        .next()
        .map_or(0, |last| last.chars().count())
        + 1;
    (line, column)
}

/// Replaces `path` with `contents` so that no reader ever sees a partial file and no other user
/// can read either one: the directory is made owner-only (`0700`), the new contents go to a
/// temporary file created `0600` beside it, are flushed to disk, and are renamed over `path`.
///
/// `Some` when the directory had to be closed to other users.
fn write_private(path: &Path, contents: &[u8]) -> Result<Option<Tightened>, CredentialsError> {
    let dir = directory_of(path);
    let failed = write_failed;
    let tightened = private_parent(path)?;
    let name = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| CREDENTIALS_FILE.to_string());
    let temp = dir.join(format!(".{name}.{}.tmp", uuid::Uuid::new_v4()));
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options
        .open(&temp)
        .map_err(|error| failed(WriteStep::Create, dir, error))?;
    if let Err(error) = file.write_all(contents).and_then(|()| file.sync_all()) {
        drop(file);
        let _ = std::fs::remove_file(&temp);
        return Err(failed(WriteStep::Write, dir, error));
    }
    drop(file);
    if let Err(error) = std::fs::rename(&temp, path) {
        let _ = std::fs::remove_file(&temp);
        return Err(failed(WriteStep::Replace, path, error));
    }
    // The rename is durable only once the directory entry is; best effort, as a failure here
    // leaves either the old or the new file, both complete.
    #[cfg(unix)]
    if let Ok(directory) = std::fs::File::open(dir) {
        let _ = directory.sync_all();
    }
    Ok(tightened)
}

/// The directory `path` is in.
fn directory_of(path: &Path) -> &Path {
    path.parent()
        .filter(|dir| !dir.as_os_str().is_empty())
        .unwrap_or(Path::new("."))
}

fn write_failed(step: WriteStep, at: &Path, error: std::io::Error) -> CredentialsError {
    CredentialsError::WriteFailed {
        path: at.to_path_buf(),
        step,
        kind: error.kind(),
        message: error.to_string(),
        owner: ownership(at),
    }
}

/// Creates the directory `path` is in owner-only, or closes an existing one to other users
/// ([`private_directory`]). `Some` when it had to be closed.
fn private_parent(path: &Path) -> Result<Option<Tightened>, CredentialsError> {
    let dir = directory_of(path);
    let from = private_directory(dir).map_err(|(step, error)| write_failed(step, dir, error))?;
    Ok(from.map(|from| Tightened {
        directory: dir.to_path_buf(),
        from,
    }))
}

/// Creates `dir` owner-only, or takes group and other access away from an existing one: the
/// mode it had, when it had to change.
#[cfg(unix)]
fn private_directory(dir: &Path) -> Result<Option<u32>, (WriteStep, std::io::Error)> {
    use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
    match std::fs::metadata(dir) {
        Ok(metadata) if !metadata.is_dir() => Err((
            WriteStep::NotADirectory,
            std::io::Error::new(
                std::io::ErrorKind::AlreadyExists,
                "it exists and is not a directory",
            ),
        )),
        Ok(metadata) => {
            let mode = metadata.permissions().mode() & 0o7777;
            if mode & 0o077 == 0 {
                return Ok(None);
            }
            std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))
                .map_err(|error| (WriteStep::Tighten { from: mode }, error))?;
            Ok(Some(mode))
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => std::fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(dir)
            .map(|()| None)
            .map_err(|error| (WriteStep::CreateDirectory, error)),
        Err(error) => Err((WriteStep::CreateDirectory, error)),
    }
}

/// Windows has no mode bits: the directory inherits the ACL of the user's profile.
#[cfg(not(unix))]
fn private_directory(dir: &Path) -> Result<Option<u32>, (WriteStep, std::io::Error)> {
    std::fs::create_dir_all(dir)
        .map(|()| None)
        .map_err(|error| (WriteStep::CreateDirectory, error))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn origins_normalize_and_refuse_anything_but_an_origin() {
        for (input, expected) in [
            ("https://api.workers.iii.dev", "https://api.workers.iii.dev"),
            (
                "https://API.Workers.iii.dev/",
                "https://api.workers.iii.dev",
            ),
            (
                "https://api.workers.iii.dev:443",
                "https://api.workers.iii.dev",
            ),
            (
                "https://registry.example:8443",
                "https://registry.example:8443",
            ),
            ("http://127.0.0.1:4000", "http://127.0.0.1:4000"),
            ("  https://registry.example  ", "https://registry.example"),
        ] {
            assert_eq!(RegistryOrigin::parse(input).unwrap().as_str(), expected);
        }
        for input in [
            "api.workers.iii.dev",
            "ftp://registry.example",
            "https://user:secret@registry.example",
            "https://user@registry.example",
            "https://registry.example/api",
            "https://registry.example/?a=1",
            "https://registry.example/#x",
            "file:///etc/passwd",
            "",
        ] {
            assert!(RegistryOrigin::parse(input).is_err(), "{input}");
        }
    }

    #[test]
    fn a_refused_registry_url_never_prints_its_credentials_or_query() {
        for (input, expected) in [
            (
                "https://user:s3cr3t@registry.example",
                "'https://registry.example/' is not a registry URL: it carries credentials",
            ),
            (
                "https://registry.example/?token=s3cr3t#s3cr3t",
                "'https://registry.example/' is not a registry URL: it has a path, query or \
                 fragment; give only scheme, host and port",
            ),
            (
                "https://user:s3cr3t@registry.example/api?token=s3cr3t",
                "'https://registry.example/api' is not a registry URL: it carries credentials",
            ),
            // Not a URL at all, so nothing of it can be told apart from a password.
            (
                "https://user:s3cr3t@[::1",
                "the registry is not a valid URL: invalid IPv6 address",
            ),
        ] {
            let error = RegistryOrigin::parse(input).unwrap_err();
            let printed = format!("{error} {error:?}");
            assert!(!printed.contains("s3cr3t"), "{printed}");
            assert!(!printed.contains("user"), "{printed}");
            assert_eq!(error.to_string(), expected, "{input}");
        }
        // A path alone is shown, so the operator sees what to drop.
        assert_eq!(
            RegistryOrigin::parse("https://registry.example/api")
                .unwrap_err()
                .to_string(),
            "'https://registry.example/api' is not a registry URL: it has a path, query or \
             fragment; give only scheme, host and port"
        );
    }

    #[test]
    fn only_https_or_loopback_http_may_carry_a_token() {
        for (origin, allowed) in [
            ("https://registry.example", true),
            ("http://127.0.0.1:4000", true),
            ("http://127.1.2.3", true),
            ("http://localhost:8080", true),
            ("http://[::1]:8080", true),
            ("http://registry.example", false),
            ("http://10.0.0.1", false),
            ("http://localhost.example.com", false),
        ] {
            assert_eq!(
                RegistryOrigin::parse(origin).unwrap().allows_tokens(),
                allowed,
                "{origin}"
            );
        }
    }

    #[test]
    fn a_token_never_prints_its_secret() {
        let token = RegistryToken::new("wdt_s3cr3tValueThatMustNotLeak").unwrap();
        for printed in [
            format!("{token}"),
            format!("{token:?}"),
            format!("{token:#?}"),
            format!("{:?}", Some(&token)),
        ] {
            assert!(!printed.contains("s3cr3t"), "{printed}");
            assert!(printed.contains("wdt_…"), "{printed}");
        }
        let jwt = RegistryToken::new("eyJhbGciOiJFUzI1NiJ9.eyJzdWIiOiJ0ZWFtIn0.c2ln").unwrap();
        assert_eq!(jwt.redacted(), "…");
        assert!(!format!("{jwt:?}").contains("eyJ"));
        let odd = RegistryToken::new("NOTAPREFIX_secret").unwrap();
        assert_eq!(odd.redacted(), "…");

        let credentials = Credentials::for_tests(
            "https://api.workers.iii.dev",
            Some("wdt_environmentSecret"),
            &[("https://registry.example", "wmt_savedSecret")],
        );
        let printed = format!("{credentials:?}");
        assert!(!printed.contains("environmentSecret"), "{printed}");
        assert!(!printed.contains("savedSecret"), "{printed}");
    }

    #[test]
    fn tokens_are_trimmed_and_must_fit_a_header() {
        assert_eq!(
            RegistryToken::new("  wdt_abc\n").unwrap().expose_secret(),
            "wdt_abc"
        );
        for bad in [
            "",
            "   ",
            "wdt_a b",
            "wdt_\u{7f}",
            "wdt_é",
            "wdt\r\nX-Evil: 1",
        ] {
            assert!(RegistryToken::new(bad).is_err(), "{bad:?}");
        }
        assert!(RegistryToken::new(&"a".repeat(MAX_TOKEN_LEN + 1)).is_err());
        let error = RegistryToken::new("wdt_a b").unwrap_err().to_string();
        assert!(!error.contains("wdt_a"), "{error}");
    }

    #[test]
    fn the_environment_token_belongs_to_the_default_registry_only() {
        let credentials = Credentials::for_tests(
            "https://api.workers.iii.dev",
            Some("wdt_environment"),
            &[
                ("https://api.workers.iii.dev", "wdt_saved"),
                ("https://registry.example", "wdt_other"),
            ],
        );
        let default = RegistryOrigin::parse("https://api.workers.iii.dev").unwrap();
        let (token, source) = credentials.token_for(&default).unwrap().unwrap();
        assert_eq!(token.expose_secret(), "wdt_environment");
        assert_eq!(source, TokenSource::Environment);

        // Another registry gets its own saved token, never the environment's.
        let other = RegistryOrigin::parse("https://registry.example").unwrap();
        let (token, source) = credentials.token_for(&other).unwrap().unwrap();
        assert_eq!(token.expose_secret(), "wdt_other");
        assert!(matches!(source, TokenSource::File(_)));

        // A host named only by a reference has no token at all.
        let named = RegistryOrigin::parse("https://evil.example").unwrap();
        assert!(credentials.token_for(&named).unwrap().is_none());
        // Nor does the default host on another port or scheme.
        for origin in [
            "https://api.workers.iii.dev:8443",
            "http://api.workers.iii.dev",
        ] {
            let origin = RegistryOrigin::parse(origin).unwrap();
            assert!(
                credentials.token_for(&origin).unwrap().is_none(),
                "{origin}"
            );
        }

        let without_environment = Credentials::for_tests(
            "https://api.workers.iii.dev",
            None,
            &[("https://api.workers.iii.dev", "wdt_saved")],
        );
        let (token, _) = without_environment.token_for(&default).unwrap().unwrap();
        assert_eq!(token.expose_secret(), "wdt_saved");
    }

    #[test]
    fn a_saved_token_for_plain_http_off_loopback_is_never_used() {
        let credentials = Credentials::for_tests(
            "https://api.workers.iii.dev",
            None,
            &[("http://registry.example", "wdt_saved")],
        );
        let origin = RegistryOrigin::parse("http://registry.example").unwrap();
        assert!(credentials.token_for(&origin).unwrap().is_none());
    }

    /// A value that is not Unicode is set, and refused as Compose refuses it, never taken for
    /// unset: `login` and `logout` read it the same way.
    #[cfg(unix)]
    #[test]
    fn a_non_unicode_variable_is_refused_rather_than_taken_for_unset() {
        use std::os::unix::ffi::OsStringExt;
        assert_eq!(environment_text(None), None);
        let value = environment_text(Some(std::ffi::OsString::from_vec(
            b"wdt_\xffSecret".to_vec(),
        )));
        let credentials = Credentials::from_sources(value, None);
        let default = RegistryOrigin::default_registry();
        assert_eq!(
            credentials.environment_problem(&default),
            Some("holds a character that is not visible ASCII")
        );
        assert!(matches!(
            credentials.token_for(&default),
            Err(CredentialsError::InvalidEnvironmentToken { .. })
        ));
    }

    #[test]
    fn an_empty_environment_variable_is_no_token_and_a_bad_one_is_an_error() {
        let default = RegistryOrigin::default_registry();
        let credentials = Credentials::from_sources(Some("  ".into()), None);
        assert!(credentials.token_for(&default).unwrap().is_none());
        let credentials = Credentials::from_sources(Some("wdt_bad token".into()), None);
        let error = credentials.token_for(&default).unwrap_err().to_string();
        assert!(error.contains(TOKEN_ENV), "{error}");
        assert!(!error.contains("bad token"), "{error}");
        // It concerns the default registry only.
        let other = RegistryOrigin::parse("https://registry.example").unwrap();
        assert!(credentials.token_for(&other).unwrap().is_none());
    }

    #[cfg(unix)]
    fn mode(path: &Path) -> u32 {
        use std::os::unix::fs::PermissionsExt;
        std::fs::metadata(path).unwrap().permissions().mode() & 0o777
    }

    #[test]
    fn the_file_is_created_owner_only_in_an_owner_only_directory() {
        let home = tempfile::tempdir().unwrap();
        let path = home.path().join(".iii").join(CREDENTIALS_FILE);
        let store = CredentialStore::at(&path);
        let origin = RegistryOrigin::default_registry();
        store
            .save(&origin, &RegistryToken::new("wdt_first").unwrap())
            .unwrap();
        #[cfg(unix)]
        {
            assert_eq!(mode(&path), 0o600);
            assert_eq!(mode(path.parent().unwrap()), 0o700);
        }
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(
            text.contains("[registries.\"https://api.workers.iii.dev\"]"),
            "{text}"
        );
        assert_eq!(store.read().unwrap()[&origin].expose_secret(), "wdt_first");
    }

    #[cfg(unix)]
    #[test]
    fn an_existing_open_directory_is_closed_before_the_file_is_written() {
        use std::os::unix::fs::PermissionsExt;
        let home = tempfile::tempdir().unwrap();
        let dir = home.path().join(".iii");
        std::fs::create_dir(&dir).unwrap();
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o775)).unwrap();
        let store = CredentialStore::at(dir.join(CREDENTIALS_FILE));
        let default = RegistryOrigin::default_registry();
        let tightened = store
            .save(&default, &RegistryToken::new("wdt_x").unwrap())
            .unwrap()
            .expect("the directory was open to others");
        assert_eq!(mode(&dir), 0o700);
        assert_eq!(mode(&dir.join(CREDENTIALS_FILE)), 0o600);
        assert_eq!(
            tightened.to_string(),
            format!(
                "Changed the permissions of {} from 0775 (drwxrwxr-x) to 0700 (drwx------), so \
                 that only you can open it: it holds your registry tokens.",
                dir.display()
            )
        );
        // Once closed, nothing more to say.
        assert_eq!(
            store
                .save(&default, &RegistryToken::new("wdt_y").unwrap())
                .unwrap(),
            None
        );
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o755)).unwrap();
        let removed = store.remove(&default).unwrap();
        assert!(removed.removed);
        assert_eq!(removed.tightened.unwrap().from, 0o755);
        assert_eq!(mode(&dir), 0o700);
    }

    #[test]
    fn saving_replaces_atomically_and_keeps_other_origins_and_content() {
        let home = tempfile::tempdir().unwrap();
        let path = home.path().join(".iii").join(CREDENTIALS_FILE);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(
            &path,
            "future = { keep = true }\n\n[registries.\"https://registry.example\"]\ntoken = \"wdt_other\"\n\n[registries.\"https://API.workers.iii.dev/\"]\ntoken = \"wdt_old\"\n",
        )
        .unwrap();
        #[cfg(unix)]
        let inode = {
            use std::os::unix::fs::MetadataExt;
            std::fs::metadata(&path).unwrap().ino()
        };
        let store = CredentialStore::at(&path);
        let default = RegistryOrigin::default_registry();
        store
            .save(&default, &RegistryToken::new("wdt_new").unwrap())
            .unwrap();
        // A new file was renamed into place, rather than the old one rewritten in place.
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            assert_ne!(std::fs::metadata(&path).unwrap().ino(), inode);
            assert_eq!(mode(&path), 0o600);
        }
        let tokens = store.read().unwrap();
        assert_eq!(tokens.len(), 2);
        assert_eq!(tokens[&default].expose_secret(), "wdt_new");
        assert_eq!(
            tokens[&RegistryOrigin::parse("https://registry.example").unwrap()].expose_secret(),
            "wdt_other"
        );
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(!text.contains("wdt_old"), "{text}");
        assert!(text.contains("keep = true"), "{text}");
        // No temporary file is left beside it, only the lock.
        let mut names: Vec<_> = std::fs::read_dir(path.parent().unwrap())
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        assert_eq!(names, [CREDENTIALS_FILE, "credentials.toml.lock"]);
    }

    #[test]
    fn saving_keeps_the_other_fields_of_the_entry_it_replaces() {
        let home = tempfile::tempdir().unwrap();
        let path = home.path().join(".iii").join(CREDENTIALS_FILE);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        // The same origin in two spellings, each with a field of its own.
        std::fs::write(
            &path,
            "[registries.\"https://API.workers.iii.dev/\"]\ntoken = \"wdt_older\"\nlabel = \"old \
             spelling\"\nnote = \"kept\"\n\n[registries.\"https://api.workers.iii.dev\"]\ntoken = \
             \"wdt_old\"\nlabel = \"laptop\"\n",
        )
        .unwrap();
        let store = CredentialStore::at(&path);
        let default = RegistryOrigin::default_registry();
        store
            .save(&default, &RegistryToken::new("wdt_new").unwrap())
            .unwrap();
        let document: toml::Table = std::fs::read_to_string(&path).unwrap().parse().unwrap();
        let registries = document[REGISTRIES_TABLE].as_table().unwrap();
        assert_eq!(
            registries.keys().collect::<Vec<_>>(),
            ["https://api.workers.iii.dev"]
        );
        let entry = registries["https://api.workers.iii.dev"]
            .as_table()
            .unwrap();
        assert_eq!(entry["token"].as_str(), Some("wdt_new"));
        assert_eq!(entry["label"].as_str(), Some("laptop"));
        assert_eq!(entry["note"].as_str(), Some("kept"));
        assert_eq!(entry.len(), 3);
    }

    /// Two spellings of one origin merge table by table: the canonical entry wins a conflict,
    /// and what only the other one has is kept, however deep.
    #[test]
    fn merging_two_spellings_keeps_nested_fields_of_both() {
        let home = tempfile::tempdir().unwrap();
        let path = home.path().join(".iii").join(CREDENTIALS_FILE);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(
            &path,
            "[registries.\"https://api.workers.iii.dev\"]\ntoken = \"wdt_old\"\nlabel = \"laptop\"\n\
             [registries.\"https://api.workers.iii.dev\".metadata]\ndevice = \"mac\"\n\n\
             [registries.\"https://API.workers.iii.dev/\"]\ntoken = \"wdt_older\"\nlabel = \"old\"\n\
             [registries.\"https://API.workers.iii.dev/\".metadata]\ndevice = \"pc\"\nnote = \"kept\"\n",
        )
        .unwrap();
        CredentialStore::at(&path)
            .save(
                &RegistryOrigin::default_registry(),
                &RegistryToken::new("wdt_new").unwrap(),
            )
            .unwrap();
        let document: toml::Table = std::fs::read_to_string(&path).unwrap().parse().unwrap();
        let registries = document[REGISTRIES_TABLE].as_table().unwrap();
        assert_eq!(registries.len(), 1);
        let entry = registries["https://api.workers.iii.dev"]
            .as_table()
            .unwrap();
        assert_eq!(entry["token"].as_str(), Some("wdt_new"));
        assert_eq!(entry["label"].as_str(), Some("laptop"));
        let metadata = entry["metadata"].as_table().unwrap();
        assert_eq!(metadata["device"].as_str(), Some("mac"));
        assert_eq!(metadata["note"].as_str(), Some("kept"));
    }

    /// A save waits for the lock another iii holds, then reads the file as that one left it: a
    /// token saved meanwhile is kept, not overwritten by a copy read before.
    #[test]
    fn a_save_waits_for_another_and_keeps_what_it_wrote() {
        let home = tempfile::tempdir().unwrap();
        let path = home.path().join(".iii").join(CREDENTIALS_FILE);
        let store = CredentialStore::at(&path);
        let default = RegistryOrigin::default_registry();
        store
            .save(&default, &RegistryToken::new("wdt_first").unwrap())
            .unwrap();

        // Another process: it holds the lock while it writes a token of its own.
        let held = std::fs::OpenOptions::new()
            .write(true)
            .open(store.lock_path())
            .unwrap();
        held.lock().unwrap();
        let (done, finished) = std::sync::mpsc::channel();
        let saving = store.clone();
        let other = RegistryOrigin::parse("https://registry.example").unwrap();
        let saved_origin = other.clone();
        std::thread::spawn(move || {
            let saved = saving.save(&saved_origin, &RegistryToken::new("wdt_mine").unwrap());
            let _ = done.send(saved);
        });
        assert!(
            finished
                .recv_timeout(std::time::Duration::from_millis(300))
                .is_err(),
            "the save went ahead while another one held the lock"
        );
        std::fs::write(
            &path,
            "[registries.\"https://api.workers.iii.dev\"]\ntoken = \"wdt_theirs\"\n",
        )
        .unwrap();
        held.unlock().unwrap();
        finished
            .recv_timeout(std::time::Duration::from_secs(10))
            .expect("the save goes ahead once the lock is free")
            .unwrap();

        let tokens = store.read().unwrap();
        assert_eq!(tokens[&default].expose_secret(), "wdt_theirs");
        assert_eq!(tokens[&other].expose_secret(), "wdt_mine");
        #[cfg(unix)]
        assert_eq!(mode(&store.lock_path()), 0o600);
    }

    #[test]
    fn removing_drops_only_that_origin() {
        let home = tempfile::tempdir().unwrap();
        let store = CredentialStore::at(home.path().join(".iii").join(CREDENTIALS_FILE));
        let default = RegistryOrigin::default_registry();
        let other = RegistryOrigin::parse("https://registry.example").unwrap();
        assert!(!store.remove(&default).unwrap().removed);
        store
            .save(&default, &RegistryToken::new("wdt_a").unwrap())
            .unwrap();
        store
            .save(&other, &RegistryToken::new("wdt_b").unwrap())
            .unwrap();
        assert!(store.remove(&default).unwrap().removed);
        assert!(!store.remove(&default).unwrap().removed);
        let tokens = store.read().unwrap();
        assert_eq!(tokens.keys().collect::<Vec<_>>(), [&other]);
    }

    #[test]
    fn credentials_are_read_again_each_time() {
        let home = tempfile::tempdir().unwrap();
        let store = CredentialStore::at(home.path().join(".iii").join(CREDENTIALS_FILE));
        let default = RegistryOrigin::default_registry();
        let before = Credentials::from_sources(None, Some(&store));
        assert!(before.token_for(&default).unwrap().is_none());
        store
            .save(&default, &RegistryToken::new("wdt_logged_in").unwrap())
            .unwrap();
        let after = Credentials::from_sources(None, Some(&store));
        assert_eq!(
            after
                .token_for(&default)
                .unwrap()
                .unwrap()
                .0
                .expose_secret(),
            "wdt_logged_in"
        );
        // The environment still wins over the file.
        let overridden = Credentials::from_sources(Some("wdt_environment".into()), Some(&store));
        let (token, source) = overridden.token_for(&default).unwrap().unwrap();
        assert_eq!(token.expose_secret(), "wdt_environment");
        assert_eq!(source, TokenSource::Environment);
    }

    /// Every test token below holds "Secret": no message may print it, nor any other text of
    /// the file.
    fn assert_quotes_nothing(text: &str) {
        for secret in ["Secret", "wdt_"] {
            assert!(!text.contains(secret), "{secret:?} leaked: {text}");
        }
    }

    fn broken_file(dir: &Path, text: &str) -> (PathBuf, CredentialsError) {
        let path = dir.join(CREDENTIALS_FILE);
        std::fs::write(&path, text).unwrap();
        let error = CredentialStore::at(&path).read().unwrap_err();
        (path, error)
    }

    const LOGIN: Recovery<'static> = Recovery::SignIn("iii registry login");

    #[test]
    fn a_syntax_error_names_the_line_and_column_and_never_quotes_the_file() {
        let home = tempfile::tempdir().unwrap();
        let (path, error) = broken_file(
            home.path(),
            "# saved by iii\n[registries.\"https://api.workers.iii.dev\"]\ntoken = wdt_unquotedSecret\n",
        );
        assert!(
            matches!(
                &error,
                CredentialsError::InvalidFile {
                    problem: FileProblem::Syntax { line: 3, column: 9 },
                    ..
                }
            ),
            "{error:?}"
        );
        let message = error.to_string();
        assert_eq!(
            message,
            format!(
                "{} is not valid TOML: syntax error at line 3, column 9",
                path.display()
            )
        );
        let fix = error.fix(LOGIN).unwrap();
        assert!(
            fix.contains(&format!(
                "Fix line 3 of the file: `{}`.",
                commands::edit(&path, 3)
            )),
            "{fix}"
        );
        assert!(
            fix.contains(&format!(
                "`{} && iii registry login`",
                commands::move_aside(&path)
            )),
            "{fix}"
        );
        #[cfg(unix)]
        assert!(
            fix.contains(&format!(
                "`nano +3 {shown}`. Or move the file aside and sign in again, which drops the \
                 tokens saved for every other registry (sign in to those again too): `mv {shown} \
                 {shown}.bak && iii registry login`.",
                shown = path.display()
            )),
            "{fix}"
        );
        assert!(
            fix.contains("drops the tokens saved for every other registry"),
            "{fix}"
        );
        assert_quotes_nothing(&message);
        assert_quotes_nothing(&fix);
    }

    #[test]
    fn columns_count_characters_not_bytes() {
        assert_eq!(position("ab\ncdé = x", 8), (2, 5));
        assert_eq!(position("", 0), (1, 1));
        assert_eq!(position("a\n", 2), (2, 1));
    }

    #[test]
    fn a_wrong_structure_names_its_line_and_never_quotes_the_file() {
        let home = tempfile::tempdir().unwrap();
        for (text, expected, says) in [
            (
                "# comment\nregistries = \"wdt_flatSecret\"\n",
                FileProblem::RegistriesNotATable { line: 2 },
                "`registries` on line 2 is not a table",
            ),
            (
                // A token pasted where the registry goes.
                "\n[registries.\"wdt_keySecret\"]\ntoken = \"wdt_goodSecret\"\n",
                FileProblem::NotAnOrigin {
                    line: 2,
                    reason: "relative URL without a base".to_string(),
                },
                "the registry key on line 2 is not an origin such as \"https://api.workers.iii.dev\" (relative URL without a base)",
            ),
            (
                "[registries]\n\"https://registry.example\" = \"wdt_flatSecret\"\n",
                FileProblem::EntryNotATable { line: 2 },
                "the registry on line 2 is not a table holding a `token`",
            ),
            (
                "[registries.\"https://registry.example\"]\ntokn = \"wdt_typoSecret\"\n",
                FileProblem::NoToken { line: 1 },
                "the registry on line 1 has no `token`",
            ),
            (
                "[registries.\"https://registry.example\"]\n\ntoken = 42\n",
                FileProblem::TokenNotAString { line: 3 },
                "the `token` on line 3 is not a quoted string",
            ),
            (
                "[registries.\"https://registry.example\"]\ntoken = \"wdt_bad Secret\"\n",
                FileProblem::BadToken {
                    line: 2,
                    reason: "holds a character that is not visible ASCII",
                },
                "the `token` on line 2 holds a character that is not visible ASCII",
            ),
        ] {
            let (path, error) = broken_file(home.path(), text);
            let line = expected.line().unwrap();
            assert_eq!(
                error,
                CredentialsError::InvalidFile {
                    path: path.clone(),
                    problem: expected,
                }
            );
            let message = error.to_string();
            assert_eq!(
                message,
                format!("{} is not a valid credentials file: {says}", path.display())
            );
            let fix = error.fix(LOGIN).unwrap();
            assert!(
                fix.contains(&format!("`{}`", commands::edit(&path, line))),
                "{fix}"
            );
            assert!(fix.contains(".bak && iii registry login`"), "{fix}");
            assert_quotes_nothing(&message);
            assert_quotes_nothing(&fix);
        }
    }

    #[test]
    fn a_fix_quotes_a_path_the_shell_would_split() {
        let home = tempfile::tempdir().unwrap();
        let dir = home.path().join("my home");
        std::fs::create_dir(&dir).unwrap();
        let (path, error) = broken_file(&dir, "registries = 1\n");
        let fix = error
            .fix(Recovery::SignIn(
                "iii registry login --registry https://registry.example",
            ))
            .unwrap();
        #[cfg(unix)]
        assert!(
            fix.contains(&format!(
                "`mv '{shown}' '{shown}.bak' && iii registry login --registry https://registry.example`",
                shown = path.display()
            )),
            "{fix}"
        );
        assert!(fix.contains(&commands::edit(&path, 1)), "{fix}");
        assert_ne!(commands::quote(&path), path.display().to_string());
    }

    #[test]
    fn signing_out_of_a_broken_file_says_how_to_sign_out_of_everything() {
        let home = tempfile::tempdir().unwrap();
        let (path, error) = broken_file(home.path(), "\n\nregistries = 1\n");
        let fix = error.fix(Recovery::SignOut("iii registry logout")).unwrap();
        assert!(
            fix.contains(&format!(
                "`{}`, then run `iii registry logout` again",
                commands::edit(&path, 3)
            )),
            "{fix}"
        );
        assert!(
            fix.contains(&format!(
                "move the whole file aside, which signs you out of every registry at once: `{}`",
                commands::move_aside(&path)
            )),
            "{fix}"
        );
        assert!(!fix.contains("login"), "{fix}");
    }

    #[test]
    fn a_directory_in_place_of_the_file_is_named_as_such() {
        let home = tempfile::tempdir().unwrap();
        let path = home.path().join(CREDENTIALS_FILE);
        std::fs::create_dir(&path).unwrap();
        let error = CredentialStore::at(&path).read().unwrap_err();
        assert_eq!(
            error.to_string(),
            format!("{} is a directory, not a credentials file", path.display())
        );
        assert_eq!(
            error.fix(LOGIN).unwrap(),
            format!(
                "Move the file aside and sign in again, which drops the tokens saved for every \
                 other registry (sign in to those again too): `{} && iii registry login`.",
                commands::move_aside(&path)
            )
        );
    }

    /// A named pipe in place of the file is refused before it is opened: opening it would wait
    /// for a writer forever, stalling even an operation `III_REGISTRY_TOKEN` covers. A link to a
    /// regular file is still read.
    #[cfg(unix)]
    #[test]
    fn a_named_pipe_in_place_of_the_file_is_refused_without_waiting_on_it() {
        let home = tempfile::tempdir().unwrap();
        let path = home.path().join(CREDENTIALS_FILE);
        let made = std::process::Command::new("mkfifo")
            .arg(&path)
            .status()
            .unwrap();
        assert!(made.success());
        let store = CredentialStore::at(&path);
        let (done, waited) = std::sync::mpsc::channel();
        let reading = store.clone();
        std::thread::spawn(move || {
            let credentials =
                Credentials::from_sources(Some("wdt_environment".into()), Some(&reading));
            let _ = done.send((reading.read(), credentials));
        });
        let (read, credentials) = waited
            .recv_timeout(std::time::Duration::from_secs(10))
            .expect("reading a named pipe must not wait for a writer");
        let error = read.unwrap_err();
        assert_eq!(
            error,
            CredentialsError::InvalidFile {
                path: path.clone(),
                problem: FileProblem::NotAFile {
                    kind: "a named pipe"
                },
            }
        );
        assert_eq!(
            error.to_string(),
            format!("{} is a named pipe, not a credentials file", path.display())
        );
        assert_eq!(
            error.fix(LOGIN).unwrap(),
            format!(
                "Move the file aside and sign in again, which drops the tokens saved for every \
                 other registry (sign in to those again too): `{} && iii registry login`.",
                commands::move_aside(&path)
            )
        );
        // The variable still covers the default registry; another one gets the error.
        let (token, source) = credentials
            .token_for(&RegistryOrigin::default_registry())
            .unwrap()
            .unwrap();
        assert_eq!(token.expose_secret(), "wdt_environment");
        assert_eq!(source, TokenSource::Environment);
        assert!(matches!(
            credentials.token_for(&RegistryOrigin::parse("https://registry.example").unwrap()),
            Err(CredentialsError::InvalidFile { .. })
        ));
        // Saving refuses too, and leaves the pipe where it is.
        assert!(
            store
                .save(
                    &RegistryOrigin::default_registry(),
                    &RegistryToken::new("wdt_x").unwrap()
                )
                .is_err()
        );
        use std::os::unix::fs::FileTypeExt;
        assert!(
            std::fs::symlink_metadata(&path)
                .unwrap()
                .file_type()
                .is_fifo()
        );

        // A link to a regular file reads as that file.
        let target = home.path().join("dotfiles.toml");
        std::fs::write(
            &target,
            "[registries.\"https://registry.example\"]\ntoken = \"wdt_linked\"\n",
        )
        .unwrap();
        let linked = home.path().join("linked.toml");
        std::os::unix::fs::symlink(&target, &linked).unwrap();
        let tokens = CredentialStore::at(&linked).read().unwrap();
        assert_eq!(
            tokens[&RegistryOrigin::parse("https://registry.example").unwrap()].expose_secret(),
            "wdt_linked"
        );
    }

    /// A file linked from elsewhere is read through the link, but a save or removal would
    /// replace the link and leave the file it points to as it was: both refuse, and say how to
    /// change that file or stop sharing it.
    #[cfg(unix)]
    #[test]
    fn a_linked_file_is_read_but_never_replaced() {
        let home = tempfile::tempdir().unwrap();
        let dir = home.path().join(".iii");
        std::fs::create_dir(&dir).unwrap();
        let target = home.path().join("shared.toml");
        let text = "[registries.\"https://registry.example\"]\ntoken = \"wdt_shared\"\n";
        std::fs::write(&target, text).unwrap();
        let path = dir.join(CREDENTIALS_FILE);
        std::os::unix::fs::symlink("../shared.toml", &path).unwrap();
        let store = CredentialStore::at(&path);
        let shared = RegistryOrigin::parse("https://registry.example").unwrap();
        assert_eq!(store.read().unwrap()[&shared].expose_secret(), "wdt_shared");

        let error = store
            .save(
                &RegistryOrigin::default_registry(),
                &RegistryToken::new("wdt_x").unwrap(),
            )
            .unwrap_err();
        let shown = dir.join("../shared.toml");
        assert_eq!(
            error.to_string(),
            format!(
                "{} is a symbolic link to {shown}: iii reads through it, but never saves or \
                 removes a token through it, as replacing the link would leave {shown} as it was",
                path.display(),
                shown = shown.display()
            )
        );
        assert_eq!(
            error.fix(LOGIN).unwrap(),
            format!(
                "Save the token in {shown} itself (`nano +1 {shown}`), or remove the link (what \
                 it points to is not touched) and run `iii registry login` again, for a file of \
                 this machine's own: `rm {path} && iii registry login`.",
                shown = shown.display(),
                path = path.display()
            )
        );
        assert_eq!(store.check_writable(), Err(error));

        let error = store.remove(&shared).unwrap_err();
        assert!(
            matches!(error, CredentialsError::LinkedFile { .. }),
            "{error:?}"
        );
        assert_eq!(
            error.fix(Recovery::SignOut("iii registry logout")).unwrap(),
            format!(
                "Remove the token from {shown} itself (`nano +1 {shown}`), or remove the link \
                 (what it points to is not touched), which signs you out of every registry it \
                 holds: `rm {path}`.",
                shown = shown.display(),
                path = path.display()
            )
        );
        // Nothing to remove needs no write, so the link is no obstacle.
        assert!(
            !store
                .remove(&RegistryOrigin::default_registry())
                .unwrap()
                .removed
        );
        assert_eq!(std::fs::read_to_string(&target).unwrap(), text);
        assert!(
            std::fs::symlink_metadata(&path)
                .unwrap()
                .file_type()
                .is_symlink()
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_link_to_nothing_is_not_mistaken_for_no_file() {
        let home = tempfile::tempdir().unwrap();
        let path = home.path().join(CREDENTIALS_FILE);
        let target = home.path().join("dotfiles").join(CREDENTIALS_FILE);
        std::os::unix::fs::symlink(&target, &path).unwrap();
        let error = CredentialStore::at(&path).read().unwrap_err();
        assert_eq!(
            error.to_string(),
            format!(
                "{} is a symbolic link to {}, which does not exist",
                path.display(),
                target.display()
            )
        );
        let fix = error.fix(LOGIN).unwrap();
        assert!(
            fix.contains(&format!("`rm {} && iii registry login`", path.display())),
            "{fix}"
        );
        // Saving would write through the link; it refuses instead.
        assert!(
            CredentialStore::at(&path)
                .save(
                    &RegistryOrigin::default_registry(),
                    &RegistryToken::new("wdt_x").unwrap()
                )
                .is_err()
        );
        assert!(!target.exists());
    }

    #[cfg(unix)]
    fn running_as_root() -> bool {
        nix::unistd::geteuid().is_root()
    }

    #[cfg(unix)]
    #[test]
    fn a_file_iii_may_not_read_says_how_to_read_it_again() {
        use std::os::unix::fs::PermissionsExt;
        if running_as_root() {
            return; // root reads anything
        }
        let home = tempfile::tempdir().unwrap();
        let path = home.path().join(CREDENTIALS_FILE);
        std::fs::write(&path, "").unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o000)).unwrap();
        let error = CredentialStore::at(&path).read().unwrap_err();
        assert!(
            matches!(
                &error,
                CredentialsError::InvalidFile {
                    problem: FileProblem::Unreadable {
                        kind: std::io::ErrorKind::PermissionDenied,
                        ..
                    },
                    ..
                }
            ),
            "{error:?}"
        );
        let message = error.to_string();
        assert!(
            message.starts_with(&format!(
                "cannot read {}: Permission denied",
                path.display()
            )),
            "{message}"
        );
        assert!(!message.contains("uid"), "{message}");
        let fix = error.fix(LOGIN).unwrap();
        assert!(
            fix.starts_with(&format!(
                "Let yourself read it: `chmod 600 {}`. Or move the file aside",
                path.display()
            )),
            "{fix}"
        );
    }

    #[test]
    fn a_file_of_another_user_says_whose_it_is() {
        let path = PathBuf::from("/home/ana/.iii/credentials.toml");
        let error = CredentialsError::InvalidFile {
            path: path.clone(),
            problem: FileProblem::Unreadable {
                kind: std::io::ErrorKind::PermissionDenied,
                message: "Permission denied (os error 13)".to_string(),
                owner: Some(Ownership {
                    owner: 0,
                    you: 1000,
                }),
            },
        };
        assert_eq!(
            error.to_string(),
            "cannot read /home/ana/.iii/credentials.toml: Permission denied (os error 13) (it \
             belongs to uid 0, and iii runs as uid 1000)"
        );
        #[cfg(unix)]
        assert!(
            error.fix(LOGIN).unwrap().starts_with(
                "Make it yours again: `sudo chown \"$(id -un)\" /home/ana/.iii/credentials.toml \
                 && chmod 600 /home/ana/.iii/credentials.toml`."
            ),
            "{:?}",
            error.fix(LOGIN)
        );
    }

    #[test]
    fn saving_or_removing_never_overwrites_a_file_it_cannot_read() {
        let home = tempfile::tempdir().unwrap();
        let text = "[registries.\"https://registry.example\"]\ntoken = \"wdt_keptSecret\"\n[registries.\"https://registry.example\"]\n";
        let (path, _) = broken_file(home.path(), text);
        let store = CredentialStore::at(&path);
        let default = RegistryOrigin::default_registry();
        let saved = store.save(&default, &RegistryToken::new("wdt_new").unwrap());
        assert!(
            matches!(saved, Err(CredentialsError::InvalidFile { .. })),
            "{saved:?}"
        );
        let removed = store.remove(&RegistryOrigin::parse("https://registry.example").unwrap());
        assert!(
            matches!(removed, Err(CredentialsError::InvalidFile { .. })),
            "{removed:?}"
        );
        assert_eq!(std::fs::read_to_string(&path).unwrap(), text);
    }

    #[test]
    fn a_broken_file_stops_every_registry_but_the_one_the_variable_covers() {
        let home = tempfile::tempdir().unwrap();
        let (_, _) = broken_file(home.path(), "registries = 1\n");
        let store = CredentialStore::at(home.path().join(CREDENTIALS_FILE));
        let default = RegistryOrigin::default_registry();
        let other = RegistryOrigin::parse("https://registry.example").unwrap();

        let without = Credentials::from_sources(None, Some(&store));
        for origin in [&default, &other] {
            assert!(
                matches!(
                    without.token_for(origin),
                    Err(CredentialsError::InvalidFile { .. })
                ),
                "{origin}"
            );
        }
        // Plain http off loopback never gets a token, so the file cannot matter there.
        let insecure = RegistryOrigin::parse("http://registry.example").unwrap();
        assert!(without.token_for(&insecure).unwrap().is_none());

        let with = Credentials::from_sources(Some("wdt_environment".into()), Some(&store));
        let (token, source) = with.token_for(&default).unwrap().unwrap();
        assert_eq!(token.expose_secret(), "wdt_environment");
        assert_eq!(source, TokenSource::Environment);
        assert!(matches!(
            with.token_for(&other),
            Err(CredentialsError::InvalidFile { .. })
        ));
    }

    #[cfg(unix)]
    #[test]
    fn a_directory_iii_may_not_close_says_why_and_whose_it_is() {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        if running_as_root() {
            return; // root may change any directory's mode
        }
        let you = nix::unistd::geteuid().as_raw();
        // A directory open to others that belongs to someone else: chmod is refused before
        // anything is written.
        let Some(dir) = ["/tmp", "/"].into_iter().map(Path::new).find(|dir| {
            std::fs::metadata(dir).is_ok_and(|metadata| {
                metadata.uid() != you && metadata.permissions().mode() & 0o077 != 0
            })
        }) else {
            return;
        };
        let path = dir.join(format!("iii-never-written-{}.toml", uuid::Uuid::new_v4()));
        let error = write_private(&path, b"").unwrap_err();
        let CredentialsError::WriteFailed {
            step: WriteStep::Tighten { from },
            kind: std::io::ErrorKind::PermissionDenied,
            owner: Some(owner),
            ..
        } = &error
        else {
            panic!("{error:?}");
        };
        assert_ne!(owner.owner, owner.you);
        assert!(!path.exists());
        let message = error.to_string();
        assert!(
            message.starts_with(&format!(
                "cannot make {} private: its mode is {:04o}, and iii closes it to other users \
                 (0700) before it keeps a token there, but changing it failed: ",
                dir.display(),
                from & 0o7777
            )),
            "{message}"
        );
        let fix = error.fix(LOGIN).unwrap();
        assert!(
            fix.contains(&format!(
                "`sudo chown \"$(id -un)\" {dir} && chmod 700 {dir}`",
                dir = dir.display()
            )),
            "{fix}"
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_directory_iii_may_not_write_in_says_how_to_open_it() {
        use std::os::unix::fs::PermissionsExt;
        if running_as_root() {
            return;
        }
        let home = tempfile::tempdir().unwrap();
        let dir = home.path().join(".iii");
        std::fs::create_dir(&dir).unwrap();
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o500)).unwrap();
        let error = CredentialStore::at(dir.join(CREDENTIALS_FILE))
            .save(
                &RegistryOrigin::default_registry(),
                &RegistryToken::new("wdt_x").unwrap(),
            )
            .unwrap_err();
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700)).unwrap();
        assert!(
            error
                .to_string()
                .starts_with(&format!("cannot create a file in {}: ", dir.display())),
            "{error}"
        );
        assert_eq!(
            error.fix(LOGIN).unwrap(),
            format!(
                "Give yourself access to it, then run the command again: `chmod u+rwx {}`.",
                dir.display()
            )
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_file_in_place_of_the_directory_is_no_credentials_until_a_save_says_how_to_fix_it() {
        let home = tempfile::tempdir().unwrap();
        let dir = home.path().join(".iii");
        std::fs::write(&dir, "not a directory").unwrap();
        let store = CredentialStore::at(dir.join(CREDENTIALS_FILE));
        assert!(store.read().unwrap().is_empty());
        let error = store
            .save(
                &RegistryOrigin::default_registry(),
                &RegistryToken::new("wdt_x").unwrap(),
            )
            .unwrap_err();
        assert_eq!(
            error.to_string(),
            format!(
                "{} exists and is not a directory, so no credentials.toml can go in it",
                dir.display()
            )
        );
        assert_eq!(
            error.fix(LOGIN).unwrap(),
            format!(
                "Move it aside, then run the command again: `mv {shown} {shown}.bak`.",
                shown = dir.display()
            )
        );
        assert_eq!(std::fs::read_to_string(&dir).unwrap(), "not a directory");
    }

    #[test]
    fn a_read_only_file_system_points_to_the_environment_variable() {
        let error = CredentialsError::WriteFailed {
            path: PathBuf::from("/home/ana/.iii"),
            step: WriteStep::Create,
            kind: std::io::ErrorKind::ReadOnlyFilesystem,
            message: "Read-only file system (os error 30)".to_string(),
            owner: None,
        };
        let fix = error.fix(LOGIN).unwrap();
        assert!(fix.contains("read-only file system"), "{fix}");
        assert!(fix.contains(TOKEN_ENV), "{fix}");
    }

    #[test]
    fn a_long_token_shows_the_prefix_the_registry_shows() {
        let token = RegistryToken::new("wdt_AbCdSecretSecretSecretSecretSecret").unwrap();
        assert_eq!(token.redacted(), "wdt_AbCd…");
        assert_eq!(format!("{token:?}"), "RegistryToken(\"wdt_AbCd…\")");
    }
}
