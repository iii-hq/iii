// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0.

//! Scoped names and registry tokens, against local mock registries only: no real registry and
//! no real token is ever involved. Every test that sends a token sets its own
//! [`TEST_CREDENTIALS`]; without one a test has none.

use super::*;
use crate::credentials::{Credentials, TEST_CREDENTIALS};
use wiremock::{Mock, MockServer, Request, ResponseTemplate, matchers};

// As long as the tokens the registry issues, so that messages show their 8-character prefix.
const ENV_TOKEN: &str = "wdt_EnvXenvironmentTokenSecret0123456789";
const SAVED_TOKEN: &str = "wdt_SavYsavedTokenSecret0123456789abcdef";

fn authorization(request: &Request) -> Option<String> {
    request
        .headers
        .get("authorization")
        .map(|value| value.to_str().unwrap().to_string())
}

/// `III_REGISTRY_TOKEN` as the daemon would read it: bound to the real default registry, which
/// [`TEST_REGISTRY`] then reaches through a mock server.
fn environment_token() -> Credentials {
    Credentials::for_tests(DEFAULT_REGISTRY, Some(ENV_TOKEN), &[])
}

fn executable_archive() -> Vec<u8> {
    let encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    let mut archive = tar::Builder::new(encoder);
    let body = b"#!/bin/sh\nexit 0\n";
    let mut header = tar::Header::new_gnu();
    header.set_size(body.len() as u64);
    header.set_mode(0o755);
    header.set_cksum();
    let name = format!("worker{}", std::env::consts::EXE_SUFFIX);
    archive.append_data(&mut header, name, &body[..]).unwrap();
    archive.into_inner().unwrap().finish().unwrap()
}

/// A registry that resolves `name` to a binary whose artefact is its own
/// `/download/<encoded name>/1.0.0/<target>`, answered by `download`.
async fn private_registry(name: &str, download: ResponseTemplate) -> (MockServer, String) {
    let server = MockServer::start().await;
    let archive = executable_archive();
    let digest = hex::encode(Sha256::digest(&archive));
    let encoded = name.replace('@', "%40").replace('/', "%2F");
    let path = format!("/download/{encoded}/1.0.0/{}", host_target());
    let url = format!("{}{path}", server.uri());
    Mock::given(matchers::method("POST"))
        .and(matchers::path("/resolve"))
        .and(matchers::body_partial_json(
            serde_json::json!({ "worker": name }),
        ))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(serde_json::json!({"graph": [{
                "name": name, "version": "1.0.0", "type": "binary",
                "binaries": { (host_target()): { "sha256": digest, "url": url } },
            }]})),
        )
        .mount(&server)
        .await;
    Mock::given(matchers::method("GET"))
        .and(matchers::path(path))
        .respond_with(download)
        .mount(&server)
        .await;
    (server, digest)
}

/// A storage host serving the archive at `/presigned`, as a presigned URL would.
async fn storage() -> MockServer {
    let server = MockServer::start().await;
    Mock::given(matchers::method("GET"))
        .and(matchers::path("/presigned"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(executable_archive()))
        .mount(&server)
        .await;
    server
}

fn redirect_to(storage: &MockServer) -> ResponseTemplate {
    ResponseTemplate::new(302).insert_header(
        "location",
        format!("{}/presigned?X-Amz-Signature=abc", storage.uri()),
    )
}

async fn with_credentials<F: std::future::Future>(
    registry: &MockServer,
    credentials: Credentials,
    future: F,
) -> F::Output {
    TEST_REGISTRY
        .scope(registry.uri(), TEST_CREDENTIALS.scope(credentials, future))
        .await
}

// --- Names ---

#[test]
fn every_reference_form_splits_into_its_registry_and_name() {
    for (reference, registry, name, prefix) in [
        ("state", DEFAULT_REGISTRY, "state", ""),
        (
            "workers.iii.dev/state",
            "https://workers.iii.dev",
            "state",
            "workers.iii.dev",
        ),
        (
            "registry.example/team/worker",
            "https://registry.example",
            "team/worker",
            "registry.example/team",
        ),
        ("@acme/ade", DEFAULT_REGISTRY, "@acme/ade", ""),
        (
            "api.workers.iii.dev/@acme/ade",
            "https://api.workers.iii.dev",
            "@acme/ade",
            "api.workers.iii.dev",
        ),
        (
            "registry.example:8443/@acme/ade",
            "https://registry.example:8443",
            "@acme/ade",
            "registry.example:8443",
        ),
    ] {
        assert_eq!(
            split_reference(reference),
            (registry.to_string(), name.to_string()),
            "{reference}"
        );
        assert_eq!(reference_prefix(reference), prefix, "{reference}");
    }
}

#[test]
fn scoped_names_follow_the_registry_grammar() {
    let longest_scope = "a".repeat(39);
    let at_limit = format!("@acme/{}", "a".repeat(MAX_SCOPED_NAME_LEN - 6));
    for valid in [
        "@acme/ade",
        "@a/b",
        "@0/0",
        "@acme-co/my_worker-2",
        &format!("@{longest_scope}/x"),
        &at_limit,
    ] {
        assert!(scoped_name(valid).is_some(), "{valid} must be accepted");
        assert!(is_package_name(valid), "{valid}");
    }
    assert_eq!(scoped_name("@acme/ade"), Some(("acme", "ade")));
    let too_long = format!("@acme/{}", "a".repeat(MAX_SCOPED_NAME_LEN - 5));
    let long_scope = format!("@{}/x", "a".repeat(40));
    for invalid in [
        "@/x",
        "@ACME/x",
        "@acme/X",
        "@acme/../x",
        "@acme/..",
        "@acme/./x",
        "@../x",
        "@acme/",
        "@acme",
        "@",
        "@-acme/x",
        "@acme/-x",
        "@acme/_x",
        "@acme/a/b",
        "@acme/a.b",
        "@ac me/x",
        "@acme/ade\n",
        "@acme\\ade",
        "acme/ade",
        &long_scope,
        &too_long,
    ] {
        assert!(
            scoped_name(invalid).is_none(),
            "{invalid:?} must be refused"
        );
        assert!(!is_package_name(invalid), "{invalid:?}");
    }
    assert_eq!(container_key("@acme/ade"), "ade");
    assert_eq!(container_key("state"), "state");
}

#[test]
fn a_malformed_scoped_reference_is_refused_and_other_forms_are_left_alone() {
    for bad in [
        "@/x",
        "@ACME/x",
        "@acme/../x",
        "@acme/",
        "registry.example/@acme/",
        "registry.example/@ACME/x",
    ] {
        assert!(scoped_reference_problem(bad).is_some(), "{bad}");
    }
    for fine in [
        "state",
        "registry.example/team/x",
        "@acme/ade",
        "registry.example/@acme/ade",
    ] {
        assert_eq!(scoped_reference_problem(fine), None, "{fine}");
    }
}

#[test]
fn a_compose_file_reads_scoped_packages_and_refuses_malformed_ones() {
    let parse = |worker: &str| {
        crate::config::ComposeFile::parse(
            &format!("containers:\n  ade:\n    worker: {worker}\n    version: ^0.4\n"),
            "/srv/app/worker-compose.yaml",
        )
    };
    for worker in [
        "package://@acme/ade",
        "package://api.workers.iii.dev/@acme/ade",
    ] {
        let file = parse(worker).unwrap();
        let crate::WorkerSource::Package { reference } = &file.containers["ade"].worker else {
            panic!("not a package")
        };
        assert_eq!(split_reference(reference).1, "@acme/ade");
    }
    for worker in [
        "package://@ACME/ade",
        "package://@acme/../ade",
        "package://@/ade",
    ] {
        let error = parse(worker).unwrap_err();
        assert_eq!(error.code(), "INVALID_WORKER_SPEC", "{worker}: {error}");
    }
}

fn package(name: &str, version: &str) -> ResolvedPackage {
    ResolvedPackage {
        name: name.to_string(),
        alias_of: None,
        registry: DEFAULT_REGISTRY.to_string(),
        version: version.to_string(),
        kind: "binary".to_string(),
        artifacts: BTreeMap::from([(
            host_target().to_string(),
            RegistryArtifact {
                sha256: "a".repeat(64),
                url: "https://api.workers.iii.dev/download/x".to_string(),
            },
        )]),
        default_config: None,
    }
}

#[test]
fn a_scoped_package_nests_under_its_scope() {
    let root = Path::new("/cache");
    let digest = "a".repeat(64);
    let target = host_target();
    assert_eq!(
        cache_entry("ade", root, &package("@acme/ade", "1.0.0"), target, &digest).unwrap(),
        root.join("@acme")
            .join(format!("ade-1.0.0-{target}-{digest}"))
    );
    assert_eq!(
        cache_entry("ade", root, &package("ade", "1.0.0"), target, &digest).unwrap(),
        root.join(format!("ade-1.0.0-{target}-{digest}"))
    );
}

/// No two package names share a cache entry, and no entry lies inside another's.
#[test]
fn the_cache_mapping_is_injective() {
    let root = Path::new("/cache");
    let digest = "a".repeat(64);
    let names = [
        "ade",
        "acme",
        "acme-ade",
        "@acme/ade",
        "@acme/ade2",
        "@acme2/ade",
        "@acme-ade/x",
        "@ade/acme",
        "@a/b-c",
        "@a-b/c",
    ];
    let entries: Vec<PathBuf> = names
        .iter()
        .map(|name| {
            cache_entry("c", root, &package(name, "1.0.0"), host_target(), &digest).unwrap()
        })
        .collect();
    let unique: std::collections::BTreeSet<_> = entries.iter().collect();
    assert_eq!(unique.len(), names.len());
    for (a, first) in entries.iter().enumerate() {
        assert!(first.starts_with(root));
        for (b, second) in entries.iter().enumerate() {
            if a != b {
                assert!(
                    !second.starts_with(first),
                    "{} is inside {}",
                    second.display(),
                    first.display()
                );
            }
        }
    }
    // An unscoped name never takes a scope's directory: it cannot start with `@`.
    assert!(!is_path_safe("@acme"));
}

#[test]
fn a_name_that_is_no_package_name_never_becomes_a_path() {
    let root = Path::new("/cache");
    let digest = "a".repeat(64);
    for name in [
        "../escape",
        "@acme/../escape",
        "@../escape",
        "@acme/x/../../y",
        "/etc/passwd",
        "@ACME/x",
        "team/tool",
        "..",
        "@acme\\..\\x",
    ] {
        let error =
            cache_entry("c", root, &package(name, "1.0.0"), host_target(), &digest).unwrap_err();
        assert_eq!(error.code(), "REGISTRY_NAME_REFUSED", "{name}");
    }
    let mut alias = package("@acme/ade", "1.0.0");
    alias.alias_of = Some("@acme/../../x".into());
    assert!(cache_entry("c", root, &alias, host_target(), &digest).is_err());
    assert!(
        cache_entry(
            "c",
            root,
            &package("@acme/ade", "../../1.0.0"),
            host_target(),
            &digest
        )
        .is_err()
    );
}

#[test]
fn a_registry_graph_may_hold_scoped_names_but_no_malformed_one() {
    let graph = |name: &str| -> ResolveResponse {
        serde_json::from_value(serde_json::json!({"graph": [
            {"name": "@acme/ade", "version": "1.0.0", "type": "binary"},
            {"name": name, "version": "1.0.0", "type": "binary"},
        ]}))
        .unwrap()
    };
    check_names("ade", DEFAULT_REGISTRY, &graph("@acme/queue")).unwrap();
    check_names("ade", DEFAULT_REGISTRY, &graph("queue")).unwrap();
    for bad in ["@acme/../queue", "@ACME/queue", "@/queue", "acme/queue"] {
        let error = check_names("ade", DEFAULT_REGISTRY, &graph(bad)).unwrap_err();
        assert_eq!(error.code(), "REGISTRY_NAME_REFUSED", "{bad}");
    }
}

// --- Tokens on the wire ---

/// The whole private path: the token goes to `/resolve` and `/download` on the registry, and the
/// presigned URL on the storage host gets none. The package lands under its scope.
#[tokio::test]
async fn a_private_worker_installs_with_the_token_only_on_the_registry() {
    let storage = storage().await;
    let (registry, _) = private_registry("@acme/ade", redirect_to(&storage)).await;
    let cache = tempfile::tempdir().unwrap();
    let installed = with_credentials(
        &registry,
        environment_token(),
        install("ade", "@acme/ade", "1.0.0", cache.path()),
    )
    .await
    .unwrap();
    let Payload::Binary(program) = installed.payload else {
        panic!("not a binary")
    };
    assert!(
        program.starts_with(cache.path().join("@acme")),
        "{}",
        program.display()
    );
    assert_eq!(installed.name, "@acme/ade");

    let bearer = format!("Bearer {ENV_TOKEN}");
    let requests = registry.received_requests().await.unwrap();
    assert_eq!(requests.len(), 2, "one resolve and one download");
    for request in &requests {
        assert_eq!(
            authorization(request).as_deref(),
            Some(bearer.as_str()),
            "{}",
            request.url
        );
    }
    let presigned = storage.received_requests().await.unwrap();
    assert_eq!(presigned.len(), 1);
    assert_eq!(authorization(&presigned[0]), None);
}

/// A lock is untrusted: an artefact URL on another host gets no token, even for a package of
/// the default registry.
#[tokio::test]
async fn a_lock_url_on_another_host_never_gets_the_token() {
    let registry = MockServer::start().await;
    let other = storage().await;
    let mut resolved = package("@acme/ade", "1.0.0");
    let archive = executable_archive();
    resolved.artifacts.insert(
        host_target().to_string(),
        RegistryArtifact {
            sha256: hex::encode(Sha256::digest(&archive)),
            url: format!("{}/presigned", other.uri()),
        },
    );
    let cache = tempfile::tempdir().unwrap();
    with_credentials(
        &registry,
        environment_token(),
        install_resolved("ade", &resolved, cache.path()),
    )
    .await
    .unwrap();
    let requests = other.received_requests().await.unwrap();
    assert_eq!(requests.len(), 1);
    assert_eq!(authorization(&requests[0]), None);
    assert!(registry.received_requests().await.unwrap().is_empty());
}

/// `III_REGISTRY_TOKEN` belongs to the default registry. A reference that names its own host
/// gets none, even when that host is reached at the same address; a token saved for that host
/// is what it gets instead.
#[tokio::test]
async fn the_environment_token_never_goes_to_a_host_named_by_a_reference() {
    let registry = MockServer::start().await;
    Mock::given(matchers::method("POST"))
        .and(matchers::path("/resolve"))
        .respond_with(ResponseTemplate::new(404).set_body_json(serde_json::json!({
            "error": {"code": "worker_not_found", "message": "Worker '@acme/ade' not found."}
        })))
        .mount(&registry)
        .await;
    let reference = "registry.example.test/@acme/ade";
    let error = with_credentials(
        &registry,
        environment_token(),
        resolve_package("ade", reference, "1.0.0"),
    )
    .await
    .unwrap_err();
    let requests = registry.received_requests().await.unwrap();
    assert_eq!(requests.len(), 1);
    assert_eq!(authorization(&requests[0]), None);
    // The hint names that registry's own login; the variable does not apply there.
    let message = error.to_string();
    assert!(
        message.contains("iii registry login --registry https://registry.example.test"),
        "{message}"
    );

    let saved = Credentials::for_tests(
        DEFAULT_REGISTRY,
        Some(ENV_TOKEN),
        &[("https://registry.example.test", SAVED_TOKEN)],
    );
    with_credentials(&registry, saved, resolve_package("ade", reference, "1.0.0"))
        .await
        .unwrap_err();
    let requests = registry.received_requests().await.unwrap();
    assert_eq!(
        authorization(&requests[1]).as_deref(),
        Some(format!("Bearer {SAVED_TOKEN}").as_str())
    );
}

/// Credentials are read for each operation: one without a token, then one after a login.
#[tokio::test]
async fn each_operation_reads_the_credentials_again() {
    let (registry, _) = private_registry("@acme/ade", ResponseTemplate::new(404)).await;
    with_credentials(
        &registry,
        Credentials::none(),
        resolve_package("ade", "@acme/ade", "1.0.0"),
    )
    .await
    .unwrap();
    with_credentials(
        &registry,
        Credentials::for_tests(DEFAULT_REGISTRY, None, &[(DEFAULT_REGISTRY, SAVED_TOKEN)]),
        resolve_package("ade", "@acme/ade", "1.0.0"),
    )
    .await
    .unwrap();
    let requests = registry.received_requests().await.unwrap();
    assert_eq!(authorization(&requests[0]), None);
    assert_eq!(
        authorization(&requests[1]).as_deref(),
        Some(format!("Bearer {SAVED_TOKEN}").as_str())
    );
}

// --- Errors and hints ---

#[tokio::test]
async fn a_refused_token_on_resolve_says_why_and_how_to_replace_it() {
    let registry = MockServer::start().await;
    Mock::given(matchers::method("POST"))
        .and(matchers::path("/resolve"))
        .respond_with(ResponseTemplate::new(401).set_body_json(serde_json::json!({
            "error": {"code": "token_expired", "scope": "auth",
                      "message": "Token expired", "fix": "Create a new token."}
        })))
        .mount(&registry)
        .await;
    let error = with_credentials(
        &registry,
        environment_token(),
        resolve_package("ade", "@acme/ade", "1.0.0"),
    )
    .await
    .unwrap_err();
    let ComposeError::RegistryAuthFailed { code, message, .. } = &error else {
        panic!("wrong error: {error:?}")
    };
    assert_eq!(code, "token_expired");
    assert_eq!(error.code(), "REGISTRY_AUTH_FAILED");
    let text = error.to_string();
    for expected in [
        "container 'ade': the registry at https://api.workers.iii.dev refused the token \
         wdt_EnvX… from III_REGISTRY_TOKEN: it has expired (token_expired). The registry \
         says: Token expired.",
        "Every request to https://api.workers.iii.dev fails while this token is sent, public \
         workers included.",
        "Create a new one on the Download tokens tab of the organization on workers.iii.dev",
        "then set III_REGISTRY_TOKEN to it where Compose runs (on the engine process, then \
         restarted, or in the shell that runs `iii compose build`).",
    ] {
        assert!(text.contains(expected), "{expected}: {text}");
    }
    assert!(message.starts_with("the registry at "), "{message}");
    assert!(!text.contains("environmentTokenSecret"), "{text}");
    assert!(!format!("{error:?}").contains("environmentTokenSecret"));
    // Not retried: another attempt cannot fix a token.
    assert_eq!(registry.received_requests().await.unwrap().len(), 1);
}

/// A `401` from a host a redirect led to is that host's answer, not a refusal of the token it
/// never received.
#[tokio::test]
async fn a_401_after_a_redirect_is_not_blamed_on_the_token() {
    let registry = MockServer::start().await;
    let other = MockServer::start().await;
    Mock::given(matchers::method("POST"))
        .and(matchers::path("/resolve"))
        .respond_with(
            ResponseTemplate::new(307)
                .insert_header("location", format!("{}/resolve", other.uri())),
        )
        .mount(&registry)
        .await;
    Mock::given(matchers::method("POST"))
        .and(matchers::path("/resolve"))
        .respond_with(ResponseTemplate::new(401).set_body_string("sign in"))
        .mount(&other)
        .await;
    let error = with_credentials(
        &registry,
        environment_token(),
        resolve_package("ade", "@acme/ade", "1.0.0"),
    )
    .await
    .unwrap_err();
    assert_eq!(error.code(), "PACKAGE_NOT_RESOLVED", "{error}");
    let requests = other.received_requests().await.unwrap();
    assert_eq!(authorization(&requests[0]), None);
}

#[tokio::test]
async fn a_refused_token_on_download_says_why_and_how_to_replace_it() {
    let (registry, _) = private_registry(
        "@acme/ade",
        ResponseTemplate::new(401)
            .set_body_json(serde_json::json!({"error": "Token revoked", "code": "token_revoked"})),
    )
    .await;
    let cache = tempfile::tempdir().unwrap();
    let saved = Credentials::for_tests(DEFAULT_REGISTRY, None, &[(DEFAULT_REGISTRY, SAVED_TOKEN)]);
    let error = with_credentials(
        &registry,
        saved,
        install("ade", "@acme/ade", "1.0.0", cache.path()),
    )
    .await
    .unwrap_err();
    let ComposeError::RegistryAuthFailed { code, message, .. } = &error else {
        panic!("wrong error: {error:?}")
    };
    assert_eq!(code, "token_revoked");
    for expected in [
        "refused the token wdt_SavY… saved in /test/credentials.toml: it was revoked \
         (token_revoked).",
        "then run `iii registry login`: it replaces the token saved in /test/credentials.toml.",
        "Or run `iii registry logout` to stop sending it: public workers need no token.",
    ] {
        assert!(message.contains(expected), "{expected}: {message}");
    }
    assert!(!error.to_string().contains("savedTokenSecret"));
}

#[tokio::test]
async fn not_found_for_a_scoped_name_hints_at_access_without_claiming_it_exists() {
    let registry = MockServer::start().await;
    Mock::given(matchers::method("POST"))
        .and(matchers::path("/resolve"))
        .respond_with(ResponseTemplate::new(404).set_body_json(serde_json::json!({
            "error": {"code": "worker_not_found", "message": "Worker not found."}
        })))
        .mount(&registry)
        .await;

    let anonymous = with_credentials(
        &registry,
        Credentials::none(),
        resolve_package("ade", "@acme/ade", "1.0.0"),
    )
    .await
    .unwrap_err()
    .to_string();
    for expected in [
        "If '@acme/ade' is a private worker",
        "iii registry login",
        "III_REGISTRY_TOKEN",
        "engine process",
        "setting it in the shell that runs `iii call` has no effect",
    ] {
        assert!(anonymous.contains(expected), "{expected}: {anonymous}");
    }

    let with_token = with_credentials(
        &registry,
        environment_token(),
        resolve_package("ade", "@acme/ade", "1.0.0"),
    )
    .await
    .unwrap_err()
    .to_string();
    for expected in [
        "the token wdt_EnvX… from III_REGISTRY_TOKEN has no access to it",
        "`iii registry whoami` lists the workers a token reaches",
        "an owner or admin of @acme can give the token access to it",
    ] {
        assert!(with_token.contains(expected), "{expected}: {with_token}");
    }
    assert!(
        !with_token.contains("environmentTokenSecret"),
        "{with_token}"
    );

    let unscoped = with_credentials(
        &registry,
        environment_token(),
        resolve_package("ade", "ade", "1.0.0"),
    )
    .await
    .unwrap_err()
    .to_string();
    assert!(!unscoped.contains("private worker"), "{unscoped}");
}

#[tokio::test]
async fn a_download_not_found_for_a_scoped_package_hints_at_access() {
    let (registry, _) = private_registry("@acme/ade", ResponseTemplate::new(404)).await;
    let cache = tempfile::tempdir().unwrap();
    let error = with_credentials(
        &registry,
        Credentials::none(),
        install("ade", "@acme/ade", "1.0.0", cache.path()),
    )
    .await
    .unwrap_err();
    assert_eq!(error.code(), "PACKAGE_DOWNLOAD_FAILED");
    assert!(
        error
            .to_string()
            .contains("If '@acme/ade' is a private worker"),
        "{error}"
    );
}

/// A registry that answers every `/resolve` with "not found".
async fn empty_registry() -> MockServer {
    let registry = MockServer::start().await;
    Mock::given(matchers::method("POST"))
        .and(matchers::path("/resolve"))
        .respond_with(ResponseTemplate::new(404).set_body_json(serde_json::json!({
            "error": {"code": "worker_not_found", "message": "Worker not found."}
        })))
        .mount(&registry)
        .await;
    registry
}

/// A `credentials.toml` that cannot be read stops every operation on a registry it could hold a
/// token for, public workers included, before any request. `III_REGISTRY_TOKEN` gets around it
/// for the default registry only. The message names the file, the line and column, and the
/// commands that fix it, never the file's content.
#[tokio::test]
async fn a_broken_credentials_file_stops_the_operation_and_says_how_to_fix_it() {
    let registry = empty_registry().await;
    let home = tempfile::tempdir().unwrap();
    let path = home.path().join("credentials.toml");
    std::fs::write(
        &path,
        "[registries.\"https://api.workers.iii.dev\"]\ntoken = wdt_oopsSecret\n",
    )
    .unwrap();
    let store = crate::credentials::CredentialStore::at(&path);
    let shown = path.display().to_string();

    // A private worker and a public one alike.
    for reference in ["@acme/ade", "state"] {
        let error = with_credentials(
            &registry,
            Credentials::from_sources(None, Some(&store)),
            resolve_package("ade", reference, "1.0.0"),
        )
        .await
        .unwrap_err();
        assert_eq!(error.code(), "REGISTRY_CREDENTIALS_INVALID", "{error}");
        let text = error.to_string();
        for expected in [
            format!(
                "container 'ade': {shown} is not valid TOML: syntax error at line 2, column 9."
            ),
            "Compose resolves and downloads nothing from https://api.workers.iii.dev until it is \
             fixed, public workers included: without the file it cannot tell which token \
             belongs to which registry"
                .to_string(),
            "Setting III_REGISTRY_TOKEN where Compose runs (on the engine process, then \
             restarted, or in the shell that runs `iii compose build`) gets around it for \
             https://api.workers.iii.dev only."
                .to_string(),
            format!(
                "Fix line 2 of the file: `{}`.",
                crate::credentials::commands::edit(&path, 2)
            ),
            format!(
                "`{} && iii registry login`",
                crate::credentials::commands::move_aside(&path)
            ),
            "drops the tokens saved for every other registry".to_string(),
        ] {
            assert!(text.contains(&expected), "{expected}: {text}");
        }
        assert!(!text.contains("oops"), "{text}");
        assert!(!format!("{error:?}").contains("oops"));
    }
    assert!(registry.received_requests().await.unwrap().is_empty());

    // Another registry: the variable does not help there.
    let error = with_credentials(
        &registry,
        Credentials::from_sources(Some(ENV_TOKEN.into()), Some(&store)),
        resolve_package("ade", "registry.example/@acme/ade", "1.0.0"),
    )
    .await
    .unwrap_err();
    assert_eq!(error.code(), "REGISTRY_CREDENTIALS_INVALID", "{error}");
    let text = error.to_string();
    for expected in [
        "Compose resolves and downloads nothing from https://registry.example",
        "III_REGISTRY_TOKEN does not get around it: it applies only to \
         https://api.workers.iii.dev.",
        "&& iii registry login --registry https://registry.example`",
    ] {
        assert!(text.contains(expected), "{expected}: {text}");
    }
    assert!(registry.received_requests().await.unwrap().is_empty());

    // The default registry with the variable set: the file has no say there.
    let error = with_credentials(
        &registry,
        Credentials::from_sources(Some(ENV_TOKEN.into()), Some(&store)),
        resolve_package("ade", "@acme/ade", "1.0.0"),
    )
    .await
    .unwrap_err();
    assert_eq!(error.code(), "PACKAGE_NOT_RESOLVED", "{error}");
    let requests = registry.received_requests().await.unwrap();
    assert_eq!(
        authorization(&requests[0]),
        Some(format!("Bearer {ENV_TOKEN}"))
    );
}

#[tokio::test]
async fn an_unusable_environment_token_stops_the_default_registry_without_quoting_it() {
    let registry = empty_registry().await;
    let error = with_credentials(
        &registry,
        Credentials::from_sources(Some("wdt_half pasted".into()), None),
        resolve_package("ade", "state", "1.0.0"),
    )
    .await
    .unwrap_err();
    assert_eq!(error.code(), "REGISTRY_CREDENTIALS_INVALID", "{error}");
    let text = error.to_string();
    assert_eq!(
        text,
        "container 'ade': III_REGISTRY_TOKEN, as Compose reads it, holds a character that is not \
         visible ASCII. Compose sends no request to https://api.workers.iii.dev while it is so, \
         public workers included, rather than fall back on another token. Set it to the whole \
         token, with nothing before or after it, or unset it where Compose runs (on the engine \
         process, then restarted, or in the shell that runs `iii compose build`)."
    );
    assert!(registry.received_requests().await.unwrap().is_empty());
}

// --- Presigned URLs in errors ---

const SIGNED_QUERY: &str = "X-Amz-Signature=secret&X-Amz-Credential=cred";

fn assert_no_signature(error: &ComposeError) {
    for text in [error.to_string(), format!("{error:?}")] {
        assert!(!text.contains("secret"), "{text}");
        assert!(!text.contains("X-Amz"), "{text}");
        assert!(!text.contains("cred"), "{text}");
    }
}

/// The storage host refuses the presigned URL: the error says so without quoting it.
#[tokio::test]
async fn a_refused_presigned_download_never_quotes_its_signature() {
    let storage = MockServer::start().await;
    Mock::given(matchers::method("GET"))
        .and(matchers::path("/presigned"))
        .respond_with(ResponseTemplate::new(403).set_body_string("SignatureDoesNotMatch"))
        .mount(&storage)
        .await;
    let (registry, _) = private_registry(
        "@acme/ade",
        ResponseTemplate::new(302).insert_header(
            "location",
            format!("{}/presigned?{SIGNED_QUERY}", storage.uri()),
        ),
    )
    .await;
    let cache = tempfile::tempdir().unwrap();
    let error = with_credentials(
        &registry,
        environment_token(),
        install("ade", "@acme/ade", "1.0.0", cache.path()),
    )
    .await
    .unwrap_err();
    assert_eq!(error.code(), "PACKAGE_DOWNLOAD_FAILED");
    assert!(error.to_string().contains("403"), "{error}");
    assert!(
        error
            .to_string()
            .contains("from the location it was redirected to"),
        "{error}"
    );
    assert_no_signature(&error);
}

/// A storage host that announces more than it sends, then hangs up, on every connection.
async fn storage_cutting_bodies_short() -> String {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    tokio::spawn(async move {
        while let Ok((mut stream, _)) = listener.accept().await {
            tokio::spawn(async move {
                let mut request = [0_u8; 4096];
                let _ = stream.read(&mut request).await;
                let _ = stream
                    .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 100000\r\n\r\nshort")
                    .await;
                let _ = stream.shutdown().await;
            });
        }
    });
    format!("http://{address}")
}

#[tokio::test]
async fn a_presigned_download_cut_short_never_quotes_its_signature() {
    let storage = storage_cutting_bodies_short().await;
    let (registry, _) = private_registry(
        "@acme/ade",
        ResponseTemplate::new(302)
            .insert_header("location", format!("{storage}/presigned?{SIGNED_QUERY}")),
    )
    .await;
    let cache = tempfile::tempdir().unwrap();
    let error = with_credentials(
        &registry,
        environment_token(),
        install("ade", "@acme/ade", "1.0.0", cache.path()),
    )
    .await
    .unwrap_err();
    assert_eq!(error.code(), "PACKAGE_DOWNLOAD_FAILED", "{error}");
    assert_no_signature(&error);
}

/// A lock may hold a presigned URL itself: a digest mismatch names its origin and path only.
#[tokio::test]
async fn a_digest_mismatch_never_quotes_a_signed_lock_url() {
    let registry = MockServer::start().await;
    let storage = storage().await;
    let mut resolved = package("@acme/ade", "1.0.0");
    resolved.artifacts.insert(
        host_target().to_string(),
        RegistryArtifact {
            sha256: "b".repeat(64),
            url: format!("{}/presigned?{SIGNED_QUERY}", storage.uri()),
        },
    );
    let cache = tempfile::tempdir().unwrap();
    let error = with_credentials(
        &registry,
        environment_token(),
        install_resolved("ade", &resolved, cache.path()),
    )
    .await
    .unwrap_err();
    assert_eq!(error.code(), "PACKAGE_DIGEST_MISMATCH");
    assert!(
        error
            .to_string()
            .contains(&format!("{}/presigned", storage.uri())),
        "{error}"
    );
    assert_no_signature(&error);
}

// --- Compose end to end ---

/// `iii compose build` of a scoped package: the lock and the project name the full scoped name,
/// the container is keyed by its last part, and no token is written anywhere.
#[tokio::test]
async fn compose_build_locks_a_scoped_package_and_writes_no_token() {
    let storage = storage().await;
    let (registry, _) = private_registry("@acme/ade", redirect_to(&storage)).await;
    let project = tempfile::tempdir().unwrap();
    let file = project.path().join("worker-compose.yaml");
    std::fs::write(
        &file,
        "containers:\n  ade:\n    worker: package://@acme/ade\n    version: 1.0.0\n",
    )
    .unwrap();
    let cache = tempfile::tempdir().unwrap();
    crate::state::TEST_PACKAGE_CACHE
        .scope(
            cache.path().to_path_buf(),
            with_credentials(&registry, environment_token(), crate::build::build(&file)),
        )
        .await
        .unwrap();

    let lock = std::fs::read_to_string(project.path().join("worker-compose.lock")).unwrap();
    let parsed: serde_yaml::Value = serde_yaml::from_str(&lock).unwrap();
    let locked = serde_yaml::to_string(&parsed).unwrap();
    assert!(locked.contains("package://@acme/ade"), "{lock}");
    assert!(locked.contains("@acme/ade"), "{lock}");
    assert!(crate::build_record::package_dir(project.path(), "ade").is_dir());
    assert!(cache.path().join("@acme").is_dir());
    for path in walk(project.path()) {
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        assert!(
            !text.contains(ENV_TOKEN),
            "{} holds the token",
            path.display()
        );
    }
}

fn walk(root: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let mut pending = vec![root.to_path_buf()];
    while let Some(dir) = pending.pop() {
        for entry in std::fs::read_dir(&dir).unwrap().flatten() {
            let path = entry.path();
            if path.is_dir() {
                pending.push(path);
            } else {
                found.push(path);
            }
        }
    }
    found
}
