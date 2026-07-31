// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

//! OCI registry resolution for worker images.

use serde::Deserialize;
use std::collections::{BTreeSet, HashMap, HashSet, VecDeque};
use std::sync::LazyLock;

pub const MANIFEST_PATH: &str = "/iii/worker.yaml";

const DEFAULT_API_URL: &str = "https://api.workers.iii.dev";
/// Maximum registry JSON response accepted by the client.
///
/// Dependency safety is primarily enforced structurally (cycle detection)
/// and at artifact boundaries (download/extraction byte caps). Bounding the
/// resolver response itself prevents an untrusted or buggy registry from
/// allocating unbounded memory before those checks can run.
pub const MAX_REGISTRY_RESPONSE_BYTES: u64 = 1024 * 1024;

/// Shared HTTP client for registry and download operations.
/// Reuses connections and TLS sessions across requests.
pub(crate) static HTTP_CLIENT: LazyLock<reqwest::Client> = LazyLock::new(|| {
    reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(120))
        .build()
        .expect("Failed to create HTTP client")
});

/// Returns true when any standard CI environment variable is present.
/// Matches the list used by engine telemetry and scaffolder-core.
pub(crate) fn is_ci_environment() -> bool {
    const CI_ENV_VARS: &[&str] = &[
        "CI",
        "GITHUB_ACTIONS",
        "GITLAB_CI",
        "CIRCLECI",
        "JENKINS_URL",
        "TRAVIS",
        "BUILDKITE",
        "TF_BUILD",
        "CODEBUILD_BUILD_ID",
        "BITBUCKET_BUILD_NUMBER",
        "DRONE",
        "TEAMCITY_VERSION",
    ];

    CI_ENV_VARS.iter().any(|var| std::env::var(var).is_ok())
}

/// Append `version` and, when in CI, `ci=true` to a `GET /download/{slug}` request.
pub(crate) fn with_download_query(
    request: reqwest::RequestBuilder,
    version: &str,
) -> reqwest::RequestBuilder {
    let mut request = request.query(&[("version", version)]);
    if is_ci_environment() {
        request = request.query(&[("ci", "true")]);
    }
    request
}

#[derive(Debug, Clone, Deserialize)]
pub struct BinaryInfo {
    pub url: String,
    pub sha256: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct BinaryWorkerResponse {
    pub name: String,
    pub version: String,
    pub binaries: HashMap<String, BinaryInfo>,
    pub config: serde_json::Value,
}

#[derive(Debug, Clone, Deserialize)]
pub struct OciWorkerResponse {
    pub name: String,
    pub version: String,
    pub image_url: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct EngineWorkerResponse {
    pub name: String,
    pub version: String,
}

/// Bundle workers ship a tar.gz archive of a packaged local-worker
/// directory. The archive root must contain `iii.worker.yaml`. The
/// engine downloads, verifies sha256, extracts atomically, and runs
/// the worker through the existing local-worker rails inside libkrun.
///
/// See `cli/bundle_download.rs` for the install pipeline and
/// `docs/creating-workers/workers-registry.mdx` for publish shape.
#[derive(Debug, Clone, Deserialize)]
pub struct BundleWorkerResponse {
    pub name: String,
    pub version: String,
    pub archive_url: String,
    pub sha256: String,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
pub enum WorkerInfoResponse {
    #[serde(rename = "binary")]
    Binary(BinaryWorkerResponse),
    #[serde(rename = "image")]
    Oci(OciWorkerResponse),
    #[serde(rename = "engine")]
    Engine(EngineWorkerResponse),
    #[serde(rename = "bundle")]
    Bundle(BundleWorkerResponse),
}

#[derive(Debug, Clone, Deserialize)]
pub struct ResolvedEdge {
    pub from: String,
    pub to: String,
    pub range: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ResolvedWorker {
    pub name: String,
    #[serde(rename = "type")]
    pub worker_type: String,
    pub version: String,
    pub repo: String,
    #[serde(default)]
    pub config: serde_json::Value,
    #[serde(default)]
    pub binaries: Option<HashMap<String, BinaryInfo>>,
    #[serde(default)]
    pub image: Option<String>,
    /// Bundle workers ship a tar.gz archive identified by a URL + sha256.
    /// `None` for non-bundle worker types. The lockfile path
    /// (`lockfile_from_graph` in `managed.rs`) requires both to be
    /// present for `worker_type == "bundle"` and rejects bundle nodes
    /// missing them so the install path can't silently degrade to an
    /// unverifiable fetch.
    #[serde(default)]
    pub archive_url: Option<String>,
    #[serde(default)]
    pub sha256: Option<String>,
    #[serde(default)]
    pub dependencies: HashMap<String, String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ResolvedWorkerGraph {
    pub root: ResolvedRoot,
    #[serde(default)]
    pub target: Option<String>,
    pub graph: Vec<ResolvedWorker>,
    pub edges: Vec<ResolvedEdge>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ResolvedRoot {
    pub name: String,
    pub version: String,
}

/// Validates that a worker name is safe for use in filesystem paths and YAML content.
/// Allowed characters: alphanumeric, dash, underscore, dot. Must not be empty or contain `..`.
///
/// Worker names also cannot START with `.`: the bundle install root reserves
/// `.locks` and `.staging` (under `~/.iii/workers-bundle/`) as internal control
/// directories. A worker name like `.locks` would shadow them on disk and let
/// `iii worker remove .locks` -> delete_worker_artifacts wipe every per-worker
/// fslock system-wide. We blanket-reject leading-dot names so the rule is
/// stable even as new internal directories are added.
pub use crate::core::types::validate_worker_name;

/// Parse "name@version" into (name, Some(version)) or just (name, None).
pub fn parse_worker_input(input: &str) -> (String, Option<String>) {
    if let Some((name, version)) = input.split_once('@') {
        (name.to_string(), Some(version.to_string()))
    } else {
        (input.to_string(), None)
    }
}

pub async fn fetch_worker_info(
    name: &str,
    version: Option<&str>,
) -> Result<WorkerInfoResponse, String> {
    validate_worker_name(name)?;

    let base_or_file = std::env::var("III_API_URL").unwrap_or_else(|_| DEFAULT_API_URL.to_string());

    let body = if base_or_file.starts_with("file://") {
        #[cfg(not(debug_assertions))]
        {
            return Err("file:// API URLs are only supported in debug/test builds. \
                 Set III_API_URL to an HTTPS URL."
                .to_string());
        }
        #[cfg(debug_assertions)]
        {
            let path = base_or_file.strip_prefix("file://").unwrap();
            read_local_registry_fixture(path)?
        }
    } else {
        let url = format!("{}/download/{}", base_or_file, name);

        let mut request = HTTP_CLIENT.get(&url);
        if let Some(v) = version {
            request = with_download_query(request, v);
        } else if is_ci_environment() {
            request = request.query(&[("ci", "true")]);
        }

        let resp = request
            .send()
            .await
            .map_err(|e| format!("Failed to resolve worker: {}", e))?;

        if resp.status() == reqwest::StatusCode::NOT_FOUND {
            return Err(format!("Worker '{}' not found", name));
        }
        // Engine workers return 204 — no artifact body, just metadata.
        if resp.status() == reqwest::StatusCode::NO_CONTENT {
            return Ok(WorkerInfoResponse::Engine(EngineWorkerResponse {
                name: name.to_string(),
                version: version.unwrap_or("latest").to_string(),
            }));
        }
        if !resp.status().is_success() {
            return Err(format!("Failed to resolve worker: HTTP {}", resp.status()));
        }

        read_registry_response(resp).await?
    };

    serde_json::from_str(&body).map_err(|e| format!("Failed to parse worker info: {}", e))
}

pub async fn fetch_resolved_worker_graph(
    name: &str,
    version: Option<&str>,
    target: Option<&str>,
) -> Result<ResolvedWorkerGraph, String> {
    validate_worker_name(name)?;

    let base_or_file = std::env::var("III_API_URL").unwrap_or_else(|_| DEFAULT_API_URL.to_string());

    let body = if base_or_file.starts_with("file://") {
        #[cfg(not(debug_assertions))]
        {
            return Err("file:// API URLs are only supported in debug/test builds. \
                 Set III_API_URL to an HTTPS URL."
                .to_string());
        }
        #[cfg(debug_assertions)]
        {
            let path = base_or_file.strip_prefix("file://").unwrap();
            read_local_registry_fixture(path)?
        }
    } else {
        let url = format!("{}/resolve", base_or_file);
        let mut body = serde_json::json!({
            "worker": name,
            "version": version.unwrap_or("latest"),
        });
        if let Some(target) = target {
            body["target"] = serde_json::Value::String(target.to_string());
        }

        let resp = HTTP_CLIENT
            .post(&url)
            .json(&body)
            .send()
            .await
            .map_err(|e| format!("Failed to resolve worker graph: {}", e))?;

        if resp.status() == reqwest::StatusCode::NOT_FOUND {
            return Err(format!("Worker '{}' not found", name));
        }
        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            return Err(format!(
                "Failed to resolve worker graph: HTTP {} {}",
                status, text
            ));
        }

        read_registry_response(resp).await?
    };

    serde_json::from_str(&body).map_err(|e| format!("Failed to parse worker graph: {}", e))
}

/// A graph larger than this is valid, but requires explicit operator consent.
///
/// This replaces the old hard `MAX_TRANSITIVE_DEPS = 32` rejection. A node
/// count is useful for UX ("this command installs a lot"), but it is not a
/// meaningful resource boundary by itself. Actual bytes are bounded by the
/// binary and bundle download/extraction caps.
pub const LARGE_DEPENDENCY_GRAPH_THRESHOLD: u32 = 32;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DependencyGraphStats {
    pub node_count: u32,
    pub edge_count: u32,
}

impl DependencyGraphStats {
    pub fn requires_confirmation(self) -> bool {
        self.node_count > LARGE_DEPENDENCY_GRAPH_THRESHOLD
    }
}

/// Validate a registry-resolved graph without imposing an arbitrary depth cap.
///
/// The old depth-5 and node-count-32 hard failures rejected legitimate
/// compositions such as `eval@0.1.0`. This validator instead enforces the
/// structural properties the installer actually relies on:
///
/// - worker names are unique,
/// - every edge references a declared node,
/// - every declared node is reachable from the requested root,
/// - the graph is acyclic.
///
/// Traversal is iterative and O(nodes + unique edges), so a long dependency
/// chain does not consume call stack. Oversized registry responses and
/// artifacts are bounded separately by byte limits.
pub fn validate_dependency_graph(
    graph: &ResolvedWorkerGraph,
) -> Result<DependencyGraphStats, crate::core::error::WorkerOpError> {
    validate_dependency_graph_roots(graph, std::slice::from_ref(&graph.root.name))
}

/// Validate a merged dependency forest from every explicitly requested root.
///
/// Manifest dependencies are resolved independently, so their merged graph
/// can have multiple legitimate roots. Keeping the roots separate avoids
/// inventing dependency edges while preserving the same reachability and
/// cycle guarantees as [`validate_dependency_graph`].
pub(crate) fn validate_dependency_graph_roots(
    graph: &ResolvedWorkerGraph,
    roots: &[String],
) -> Result<DependencyGraphStats, crate::core::error::WorkerOpError> {
    use crate::core::error::WorkerOpError;

    if roots.is_empty() {
        return Err(WorkerOpError::DependencyGraphInvalid {
            reason: "dependency graph has no requested roots".to_string(),
        });
    }

    let mut graph_names = HashSet::with_capacity(graph.graph.len());
    for worker in &graph.graph {
        if !graph_names.insert(worker.name.as_str()) {
            return Err(WorkerOpError::DependencyGraphInvalid {
                reason: format!("duplicate worker node {:?}", worker.name),
            });
        }
    }

    let mut nodes = graph_names;
    let roots = roots.iter().map(String::as_str).collect::<BTreeSet<_>>();
    nodes.extend(roots.iter().copied());

    let mut adjacency: HashMap<&str, HashSet<&str>> = HashMap::new();
    let mut indegree: HashMap<&str, u32> = nodes.iter().map(|&name| (name, 0)).collect();
    let mut unique_edge_count = 0u32;

    for edge in &graph.edges {
        let from = edge.from.as_str();
        let to = edge.to.as_str();
        if !nodes.contains(from) || !nodes.contains(to) {
            return Err(WorkerOpError::DependencyGraphInvalid {
                reason: format!(
                    "edge {:?} -> {:?} references an undeclared worker",
                    edge.from, edge.to
                ),
            });
        }

        if adjacency.entry(from).or_default().insert(to) {
            unique_edge_count = unique_edge_count.saturating_add(1);
            *indegree.get_mut(to).expect("validated dependency node") += 1;
        }
    }

    // Reject disconnected payload nodes. They are not dependencies of any
    // requested root and must not be installed merely because a resolver
    // included them in its response.
    let mut reachable = HashSet::new();
    let mut frontier = roots.iter().copied().collect::<Vec<_>>();
    while let Some(node) = frontier.pop() {
        if !reachable.insert(node) {
            continue;
        }
        if let Some(children) = adjacency.get(node) {
            frontier.extend(children.iter().copied());
        }
    }
    if reachable.len() != nodes.len() {
        let unreachable = nodes
            .difference(&reachable)
            .copied()
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect::<Vec<_>>()
            .join(", ");
        let root_description = if roots.len() == 1 {
            format!("{:?}", roots.first().expect("non-empty roots"))
        } else {
            format!(
                "declared roots [{}]",
                roots.iter().copied().collect::<Vec<_>>().join(", ")
            )
        };
        return Err(WorkerOpError::DependencyGraphInvalid {
            reason: format!(
                "worker node(s) are unreachable from {root_description}: {unreachable}"
            ),
        });
    }

    // Kahn's algorithm provides explicit cycle detection without recursion.
    let mut queue: VecDeque<&str> = indegree
        .iter()
        .filter_map(|(&name, &degree)| (degree == 0).then_some(name))
        .collect();
    let mut visited = 0usize;
    while let Some(node) = queue.pop_front() {
        visited += 1;
        if let Some(children) = adjacency.get(node) {
            for &child in children {
                let degree = indegree.get_mut(child).expect("validated dependency node");
                *degree -= 1;
                if *degree == 0 {
                    queue.push_back(child);
                }
            }
        }
    }
    if visited != nodes.len() {
        let cyclic = indegree
            .iter()
            .filter_map(|(&name, &degree)| (degree > 0).then_some(name))
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect::<Vec<_>>()
            .join(", ");
        return Err(WorkerOpError::DependencyGraphInvalid {
            reason: format!("dependency cycle detected involving: {cyclic}"),
        });
    }

    Ok(DependencyGraphStats {
        node_count: nodes.len().try_into().unwrap_or(u32::MAX),
        edge_count: unique_edge_count,
    })
}

#[cfg(debug_assertions)]
fn read_local_registry_fixture(path: &str) -> Result<String, String> {
    let metadata = std::fs::metadata(path)
        .map_err(|e| format!("Failed to read local API fixture at {path}: {e}"))?;
    if metadata.len() > MAX_REGISTRY_RESPONSE_BYTES {
        return Err(format!(
            "Registry response too large: {} bytes, limit {}",
            metadata.len(),
            MAX_REGISTRY_RESPONSE_BYTES
        ));
    }
    std::fs::read_to_string(path)
        .map_err(|e| format!("Failed to read local API fixture at {path}: {e}"))
}

async fn read_registry_response(response: reqwest::Response) -> Result<String, String> {
    use futures::StreamExt as _;

    if let Some(length) = response.content_length()
        && length > MAX_REGISTRY_RESPONSE_BYTES
    {
        return Err(format!(
            "Registry response too large: {length} bytes, limit {MAX_REGISTRY_RESPONSE_BYTES}"
        ));
    }

    let mut body = Vec::new();
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| format!("Failed to read API response: {e}"))?;
        let next_len = body.len().saturating_add(chunk.len());
        if next_len as u64 > MAX_REGISTRY_RESPONSE_BYTES {
            return Err(format!(
                "Registry response too large: more than {MAX_REGISTRY_RESPONSE_BYTES} bytes"
            ));
        }
        body.extend_from_slice(&chunk);
    }

    String::from_utf8(body).map_err(|e| format!("Registry response is not valid UTF-8: {e}"))
}

#[cfg(test)]
pub(crate) fn clear_ci_env_vars_for_test() {
    const CI_ENV_VARS: &[&str] = &[
        "CI",
        "GITHUB_ACTIONS",
        "GITLAB_CI",
        "CIRCLECI",
        "JENKINS_URL",
        "TRAVIS",
        "BUILDKITE",
        "TF_BUILD",
        "CODEBUILD_BUILD_ID",
        "BITBUCKET_BUILD_NUMBER",
        "DRONE",
        "TEAMCITY_VERSION",
    ];
    for var in CI_ENV_VARS {
        // SAFETY: test-only; serialized by TEST_ENV_LOCK in callers.
        unsafe { std::env::remove_var(var) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_ci_environment_false_when_no_ci_vars() {
        let _guard = crate::TEST_ENV_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        clear_ci_env_vars_for_test();
        assert!(!is_ci_environment());
    }

    #[test]
    fn is_ci_environment_detects_ci_var() {
        let _guard = crate::TEST_ENV_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        clear_ci_env_vars_for_test();
        // SAFETY: test-only; serialized by TEST_ENV_LOCK.
        unsafe { std::env::set_var("CI", "true") };
        assert!(is_ci_environment());
        unsafe { std::env::remove_var("CI") };
    }

    #[test]
    fn is_ci_environment_detects_github_actions() {
        let _guard = crate::TEST_ENV_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        clear_ci_env_vars_for_test();
        // SAFETY: test-only; serialized by TEST_ENV_LOCK.
        unsafe { std::env::set_var("GITHUB_ACTIONS", "true") };
        assert!(is_ci_environment());
        unsafe { std::env::remove_var("GITHUB_ACTIONS") };
    }

    #[tokio::test]
    async fn fetch_worker_info_appends_ci_true_in_ci_environment() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        let _guard = crate::TEST_ENV_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        clear_ci_env_vars_for_test();

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind test server");
        let base_url = format!("http://{}", listener.local_addr().unwrap());
        let (tx, rx) = tokio::sync::oneshot::channel();
        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.expect("accept request");
            let mut buf = [0_u8; 4096];
            let n = stream.read(&mut buf).await.expect("read request");
            let request = String::from_utf8_lossy(&buf[..n]);
            let path = request
                .lines()
                .next()
                .and_then(|line| line.split_whitespace().nth(1))
                .unwrap_or_default()
                .to_string();
            let _ = tx.send(path);
            let _ = stream
                .write_all(b"HTTP/1.1 204 No Content\r\ncontent-length: 0\r\n\r\n")
                .await;
        });

        // SAFETY: test-only; serialized by TEST_ENV_LOCK.
        unsafe { std::env::set_var("III_API_URL", &base_url) };
        unsafe { std::env::set_var("CI", "true") };

        let result = fetch_worker_info("iii-exec", Some("latest")).await;

        unsafe { std::env::remove_var("III_API_URL") };
        unsafe { std::env::remove_var("CI") };

        assert!(
            result.is_ok(),
            "fetch_worker_info should succeed: {result:?}"
        );
        let path = rx.await.expect("request captured");
        assert!(
            path.contains("ci=true"),
            "expected ci=true in download request, got: {path}"
        );
        assert!(
            path.contains("version=latest"),
            "expected version=latest in download request, got: {path}"
        );
        server.abort();
    }

    #[tokio::test]
    async fn fetch_worker_info_binary_via_file() {
        let dir = tempfile::tempdir().unwrap();
        let json = r#"{
            "name": "image-resize",
            "type": "binary",
            "version": "0.1.2",
            "binaries": {
                "aarch64-apple-darwin": {
                    "sha256": "abc123",
                    "url": "https://example.com/image-resize-aarch64-apple-darwin.tar.gz"
                }
            },
            "config": {
                "name": "image-resize",
                "config": { "width": 200 }
            }
        }"#;
        let response_path = dir.path().join("response.json");
        std::fs::write(&response_path, json).unwrap();

        let url = format!("file://{}", response_path.display());
        let result = {
            let _guard = crate::TEST_ENV_LOCK
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            unsafe { std::env::set_var("III_API_URL", &url) };
            let r = fetch_worker_info("image-resize", None).await;
            unsafe { std::env::remove_var("III_API_URL") };
            r
        };

        let info = result.unwrap();
        match info {
            WorkerInfoResponse::Binary(b) => {
                assert_eq!(b.name, "image-resize");
                assert_eq!(b.version, "0.1.2");
            }
            _ => panic!("expected Binary variant"),
        }
    }

    #[tokio::test]
    async fn fetch_worker_info_oci_via_file() {
        let dir = tempfile::tempdir().unwrap();
        let json = r#"{
            "name": "todo-worker",
            "type": "image",
            "version": "0.1.0",
            "image_url": "docker.io/andersonofl/todo-worker:0.1.0"
        }"#;
        let response_path = dir.path().join("response.json");
        std::fs::write(&response_path, json).unwrap();

        let url = format!("file://{}", response_path.display());
        let result = {
            let _guard = crate::TEST_ENV_LOCK
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            unsafe { std::env::set_var("III_API_URL", &url) };
            let r = fetch_worker_info("todo-worker", None).await;
            unsafe { std::env::remove_var("III_API_URL") };
            r
        };

        let info = result.unwrap();
        match info {
            WorkerInfoResponse::Oci(o) => {
                assert_eq!(o.name, "todo-worker");
                assert_eq!(o.image_url, "docker.io/andersonofl/todo-worker:0.1.0");
            }
            _ => panic!("expected Oci variant"),
        }
    }

    #[tokio::test]
    async fn fetch_resolved_worker_graph_via_file() {
        let dir = tempfile::tempdir().unwrap();
        let json = r#"{
            "root": {"name": "hello-worker", "version": "1.0.0"},
            "target": "aarch64-apple-darwin",
            "graph": [
                {
                    "name": "helper",
                    "type": "binary",
                    "version": "1.0.0",
                    "repo": "https://example.com/helper",
                    "config": {},
                    "binaries": {
                        "aarch64-apple-darwin": {
                            "sha256": "abc123",
                            "url": "https://example.com/helper.tar.gz"
                        }
                    },
                    "dependencies": {}
                },
                {
                    "name": "hello-worker",
                    "type": "binary",
                    "version": "1.0.0",
                    "repo": "https://example.com/hello-worker",
                    "config": {},
                    "binaries": {
                        "aarch64-apple-darwin": {
                            "sha256": "def456",
                            "url": "https://example.com/hello-worker.tar.gz"
                        }
                    },
                    "dependencies": {"helper": "^1.0.0"}
                }
            ],
            "edges": [{"from": "hello-worker", "to": "helper", "range": "^1.0.0"}]
        }"#;
        let response_path = dir.path().join("response.json");
        std::fs::write(&response_path, json).unwrap();

        let url = format!("file://{}", response_path.display());
        let result = {
            let _guard = crate::TEST_ENV_LOCK
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            unsafe { std::env::set_var("III_API_URL", &url) };
            let r = fetch_resolved_worker_graph(
                "hello-worker",
                Some("1.0.0"),
                Some("aarch64-apple-darwin"),
            )
            .await;
            unsafe { std::env::remove_var("III_API_URL") };
            r
        };

        let graph = result.unwrap();
        assert_eq!(graph.root.name, "hello-worker");
        assert_eq!(graph.graph.len(), 2);
        assert_eq!(graph.edges[0].to, "helper");
    }

    #[test]
    fn registry_fixture_larger_than_response_budget_is_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let response_path = dir.path().join("oversized.json");
        std::fs::write(
            &response_path,
            vec![b' '; MAX_REGISTRY_RESPONSE_BYTES as usize + 1],
        )
        .unwrap();

        let result = read_local_registry_fixture(
            response_path
                .to_str()
                .expect("temporary fixture path is valid UTF-8"),
        );

        let err = result.expect_err("oversized registry response must be rejected");
        assert!(err.contains("Registry response too large"), "{err}");
    }

    #[tokio::test]
    async fn fetch_worker_info_rejects_invalid_name() {
        let result = fetch_worker_info("../evil", None).await;
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.contains("invalid characters") || err.contains("'..'"));
    }

    #[test]
    fn parse_version_override_syntax() {
        let (name, version) = parse_worker_input("image-resize@0.1.2");
        assert_eq!(name, "image-resize");
        assert_eq!(version, Some("0.1.2".to_string()));
    }

    #[test]
    fn parse_name_without_version() {
        let (name, version) = parse_worker_input("image-resize");
        assert_eq!(name, "image-resize");
        assert_eq!(version, None);
    }

    #[test]
    fn parse_worker_input_empty_version() {
        let (name, version) = parse_worker_input("pdfkit@");
        assert_eq!(name, "pdfkit");
        assert_eq!(version, Some("".to_string()));
    }

    #[test]
    fn parse_worker_input_with_multiple_at_signs() {
        let (name, version) = parse_worker_input("scope@org@1.0");
        assert_eq!(name, "scope");
        assert_eq!(version, Some("org@1.0".to_string()));
    }

    #[test]
    fn validate_worker_name_valid() {
        assert!(validate_worker_name("image-resize").is_ok());
        assert!(validate_worker_name("my_worker.v2").is_ok());
        assert!(validate_worker_name("pdfkit").is_ok());
    }

    #[test]
    fn validate_worker_name_rejects_path_traversal() {
        assert!(validate_worker_name("../../../etc/passwd").is_err());
        assert!(validate_worker_name("foo/bar").is_err());
        assert!(validate_worker_name("foo\\bar").is_err());
    }

    #[test]
    fn validate_worker_name_rejects_yaml_injection() {
        assert!(validate_worker_name("evil\n  - name: injected").is_err());
        assert!(validate_worker_name("evil\r\nimage: bad").is_err());
        assert!(validate_worker_name("name: injected").is_err());
    }

    #[test]
    fn validate_worker_name_rejects_empty() {
        assert!(validate_worker_name("").is_err());
    }

    #[test]
    fn validate_worker_name_rejects_dotdot() {
        assert!(validate_worker_name("..").is_err());
        assert!(validate_worker_name("foo..bar").is_err());
    }

    #[test]
    fn validate_worker_name_rejects_leading_dot() {
        // Reserved control-directory names. `iii worker remove .locks`
        // would otherwise wipe every per-worker fslock system-wide via
        // delete_worker_artifacts -> remove_dir_all(~/.iii/workers-bundle/.locks).
        assert!(validate_worker_name(".locks").is_err());
        assert!(validate_worker_name(".staging").is_err());
        assert!(validate_worker_name(".").is_err());
        assert!(validate_worker_name(".hidden").is_err());
        // Dots in the middle or trailing are still allowed:
        assert!(validate_worker_name("worker.v2").is_ok());
        assert!(validate_worker_name("a.b.c").is_ok());
    }

    #[test]
    fn deserialize_binary_worker_response() {
        let json = r#"{
            "name": "image-resize",
            "type": "binary",
            "version": "0.1.2",
            "binaries": {
                "aarch64-apple-darwin": {
                    "sha256": "5fdbce8e5db431ea6dddb527d3be0adf5bfac92fafac4a0c78d21e438d583f17",
                    "url": "https://github.com/iii-hq/workers/releases/download/image-resize/v0.1.2/image-resize-aarch64-apple-darwin.tar.gz"
                },
                "x86_64-unknown-linux-gnu": {
                    "sha256": "37c9b004c61cc76d8041cd3645ac7e7004cacd9eccbdd6bda1d847922fa98eb4",
                    "url": "https://github.com/iii-hq/workers/releases/download/image-resize/v0.1.2/image-resize-x86_64-unknown-linux-gnu.tar.gz"
                }
            },
            "config": {
                "name": "image-resize",
                "config": {
                    "width": 200,
                    "height": 200,
                    "quality": { "jpeg": 85, "webp": 80 },
                    "strategy": "scale-to-fit"
                }
            }
        }"#;
        let response: WorkerInfoResponse = serde_json::from_str(json).unwrap();
        match response {
            WorkerInfoResponse::Binary(b) => {
                assert_eq!(b.name, "image-resize");
                assert_eq!(b.version, "0.1.2");
                assert_eq!(b.binaries.len(), 2);
                let darwin = b.binaries.get("aarch64-apple-darwin").unwrap();
                assert_eq!(
                    darwin.sha256,
                    "5fdbce8e5db431ea6dddb527d3be0adf5bfac92fafac4a0c78d21e438d583f17"
                );
                assert!(darwin.url.ends_with("aarch64-apple-darwin.tar.gz"));
                assert_eq!(b.config["name"], "image-resize");
            }
            _ => panic!("expected Binary variant"),
        }
    }

    #[test]
    fn deserialize_binary_worker_response_with_empty_registry_config() {
        let json = r#"{
            "name": "image-resize",
            "type": "binary",
            "version": "0.1.2",
            "binaries": {
                "aarch64-apple-darwin": {
                    "sha256": "abc123",
                    "url": "https://example.com/image-resize-aarch64-apple-darwin.tar.gz"
                }
            },
            "config": {}
        }"#;
        let response: WorkerInfoResponse = serde_json::from_str(json).unwrap();
        match response {
            WorkerInfoResponse::Binary(b) => {
                assert_eq!(b.name, "image-resize");
            }
            _ => panic!("expected Binary variant"),
        }
    }

    #[test]
    fn deserialize_oci_worker_response() {
        let json = r#"{
            "name": "todo-worker",
            "type": "image",
            "version": "0.1.0",
            "image_url": "docker.io/andersonofl/todo-worker:0.1.0"
        }"#;
        let response: WorkerInfoResponse = serde_json::from_str(json).unwrap();
        match response {
            WorkerInfoResponse::Oci(o) => {
                assert_eq!(o.name, "todo-worker");
                assert_eq!(o.version, "0.1.0");
                assert_eq!(o.image_url, "docker.io/andersonofl/todo-worker:0.1.0");
            }
            _ => panic!("expected Oci variant"),
        }
    }

    #[test]
    fn deserialize_unknown_type_fails() {
        let json = r#"{"name": "x", "type": "wasm", "version": "1.0"}"#;
        let result: Result<WorkerInfoResponse, _> = serde_json::from_str(json);
        assert!(result.is_err());
    }

    // -- Deserialization edge cases --

    #[test]
    fn deserialize_binary_missing_type_fails() {
        let json = r#"{"name": "x", "version": "1.0", "binaries": {}, "config": {"name": "x", "config": {}}}"#;
        let result: Result<WorkerInfoResponse, _> = serde_json::from_str(json);
        assert!(result.is_err(), "missing 'type' field should fail");
    }

    #[test]
    fn deserialize_binary_missing_name_fails() {
        let json = r#"{"type": "binary", "version": "1.0", "binaries": {}, "config": {"name": "x", "config": {}}}"#;
        let result: Result<WorkerInfoResponse, _> = serde_json::from_str(json);
        assert!(result.is_err(), "missing 'name' field should fail");
    }

    #[test]
    fn deserialize_binary_missing_binaries_fails() {
        let json = r#"{"name": "x", "type": "binary", "version": "1.0", "config": {"name": "x", "config": {}}}"#;
        let result: Result<WorkerInfoResponse, _> = serde_json::from_str(json);
        assert!(result.is_err(), "missing 'binaries' field should fail");
    }

    #[test]
    fn deserialize_binary_missing_config_fails() {
        let json = r#"{"name": "x", "type": "binary", "version": "1.0", "binaries": {}}"#;
        let result: Result<WorkerInfoResponse, _> = serde_json::from_str(json);
        assert!(result.is_err(), "missing 'config' field should fail");
    }

    #[test]
    fn deserialize_oci_missing_image_url_fails() {
        let json = r#"{"name": "x", "type": "image", "version": "1.0"}"#;
        let result: Result<WorkerInfoResponse, _> = serde_json::from_str(json);
        assert!(result.is_err(), "missing 'image_url' field should fail");
    }

    #[test]
    fn deserialize_binary_empty_binaries_map_ok() {
        let json = r#"{
            "name": "empty-worker",
            "type": "binary",
            "version": "0.1.0",
            "binaries": {},
            "config": {"name": "empty-worker", "config": {}}
        }"#;
        let response: WorkerInfoResponse = serde_json::from_str(json).unwrap();
        match response {
            WorkerInfoResponse::Binary(b) => {
                assert_eq!(b.name, "empty-worker");
                assert!(b.binaries.is_empty());
            }
            _ => panic!("expected Binary variant"),
        }
    }

    #[test]
    fn deserialize_binary_info_missing_sha256_fails() {
        let json = r#"{
            "name": "x",
            "type": "binary",
            "version": "1.0",
            "binaries": {
                "aarch64-apple-darwin": {"url": "https://example.com/file.tar.gz"}
            },
            "config": {"name": "x", "config": {}}
        }"#;
        let result: Result<WorkerInfoResponse, _> = serde_json::from_str(json);
        assert!(result.is_err(), "BinaryInfo missing sha256 should fail");
    }

    #[test]
    fn deserialize_binary_info_missing_url_fails() {
        let json = r#"{
            "name": "x",
            "type": "binary",
            "version": "1.0",
            "binaries": {
                "aarch64-apple-darwin": {"sha256": "abc123"}
            },
            "config": {"name": "x", "config": {}}
        }"#;
        let result: Result<WorkerInfoResponse, _> = serde_json::from_str(json);
        assert!(result.is_err(), "BinaryInfo missing url should fail");
    }

    #[test]
    fn deserialize_extra_fields_tolerated() {
        let json = r#"{
            "name": "x",
            "type": "image",
            "version": "1.0",
            "image_url": "docker.io/x:1.0",
            "description": "this field is not in the struct",
            "author": "someone"
        }"#;
        let response: WorkerInfoResponse = serde_json::from_str(json).unwrap();
        match response {
            WorkerInfoResponse::Oci(o) => assert_eq!(o.name, "x"),
            _ => panic!("expected Oci variant"),
        }
    }

    #[test]
    fn deserialize_completely_invalid_json_fails() {
        let json = "not json at all";
        let result: Result<WorkerInfoResponse, _> = serde_json::from_str(json);
        assert!(result.is_err());
    }

    #[test]
    fn deserialize_empty_json_object_fails() {
        let json = "{}";
        let result: Result<WorkerInfoResponse, _> = serde_json::from_str(json);
        assert!(result.is_err(), "empty object should fail (no type tag)");
    }

    #[test]
    fn deserialize_config_with_null_value_ok() {
        let json = r#"{
            "name": "x",
            "type": "binary",
            "version": "1.0",
            "binaries": {},
            "config": {"name": "x", "config": null}
        }"#;
        let response: WorkerInfoResponse = serde_json::from_str(json).unwrap();
        match response {
            WorkerInfoResponse::Binary(b) => {
                assert!(b.config["config"].is_null());
            }
            _ => panic!("expected Binary variant"),
        }
    }

    // -- fetch_worker_info error paths --

    #[tokio::test]
    async fn fetch_worker_info_empty_name_rejected() {
        let result = fetch_worker_info("", None).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("cannot be empty"));
    }

    #[tokio::test]
    async fn fetch_worker_info_dotdot_name_rejected() {
        let result = fetch_worker_info("..", None).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("'..'"));
    }

    #[tokio::test]
    async fn fetch_worker_info_file_not_found() {
        let url = "file:///tmp/nonexistent-iii-test-fixture-12345.json";
        let result = {
            let _guard = crate::TEST_ENV_LOCK
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            unsafe { std::env::set_var("III_API_URL", url) };
            let r = fetch_worker_info("some-worker", None).await;
            unsafe { std::env::remove_var("III_API_URL") };
            r
        };
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .contains("Failed to read local API fixture")
        );
    }

    #[tokio::test]
    async fn fetch_worker_info_malformed_json() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("bad.json");
        std::fs::write(&path, "this is not json").unwrap();

        let url = format!("file://{}", path.display());
        let result = {
            let _guard = crate::TEST_ENV_LOCK
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            unsafe { std::env::set_var("III_API_URL", &url) };
            let r = fetch_worker_info("some-worker", None).await;
            unsafe { std::env::remove_var("III_API_URL") };
            r
        };
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Failed to parse worker info"));
    }

    #[tokio::test]
    async fn fetch_worker_info_empty_json_object() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("empty.json");
        std::fs::write(&path, "{}").unwrap();

        let url = format!("file://{}", path.display());
        let result = {
            let _guard = crate::TEST_ENV_LOCK
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            unsafe { std::env::set_var("III_API_URL", &url) };
            let r = fetch_worker_info("some-worker", None).await;
            unsafe { std::env::remove_var("III_API_URL") };
            r
        };
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Failed to parse worker info"));
    }

    #[tokio::test]
    async fn fetch_worker_info_wrong_type_in_fixture() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("wasm.json");
        std::fs::write(&path, r#"{"name": "x", "type": "wasm", "version": "1.0"}"#).unwrap();

        let url = format!("file://{}", path.display());
        let result = {
            let _guard = crate::TEST_ENV_LOCK
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            unsafe { std::env::set_var("III_API_URL", &url) };
            let r = fetch_worker_info("some-worker", None).await;
            unsafe { std::env::remove_var("III_API_URL") };
            r
        };
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Failed to parse worker info"));
    }
}
