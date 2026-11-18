// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0.

//! Source-only regressions for the optional v1 registry root binding.
//! These tests use a local mock HTTP server, never a running worker or VM.

use super::*;
use serde_json::{Value, json};
use wiremock::{Mock, MockServer, ResponseTemplate, matchers};

const OPAQUE: &str = "${SCENARIO_LITERAL}/tool";
const REGISTRY: &str = "registry.example.test";

fn node(name: &str) -> Value {
    json!({"name": name, "version": "1.2.3", "type": "bundle"})
}

fn binding(request: &str, selector: &str, name: &str) -> Value {
    json!({"schema": 1, "requested": {"worker": request, "version": selector},
        "name": name, "version": "1.2.3"})
}

fn bound_graph() -> Value {
    json!({
        "root": binding(OPAQUE, "next", "tool"),
        // Deliberately not root-first: order must never select the root.
        "graph": [node("queue"), node("tool")],
        "edges": [{"from": "tool", "to": "queue"}]
    })
}

#[test]
fn root_binding_selects_one_safe_node_without_changing_request_or_alias_identity() {
    for request in ["tool", OPAQUE] {
        let mut body = bound_graph();
        body["root"]["requested"]["worker"] = request.into();
        body["graph"][1]["alias_of"] = "canonical-tool".into();
        let response = validate_resolution(
            "renamed",
            DEFAULT_REGISTRY,
            request,
            "next",
            serde_json::from_value(body).unwrap(),
        )
        .unwrap();
        assert_eq!(response.root, 1);
        assert_eq!(response.response.graph[response.root].name, "tool");
        assert_eq!(
            response.response.graph[response.root].alias_of.as_deref(),
            Some("canonical-tool")
        );
        assert_eq!(response.response.root.unwrap().requested.worker, request);
    }
}

#[test]
fn legacy_same_name_and_alias_responses_need_no_binding() {
    for alias in [Value::Null, json!("canonical-tool")] {
        let response = validate_resolution(
            "renamed",
            DEFAULT_REGISTRY,
            "tool",
            "1.2.3",
            serde_json::from_value(json!({"graph": [{
                "name": "tool", "alias_of": alias, "version": "1.2.3", "type": "bundle"
            }], "future_metadata": true}))
            .unwrap(),
        )
        .unwrap();
        assert_eq!(response.response.graph[response.root].name, "tool");
    }
}

/// Both public lookup paths must reject the same invalid response before artifacts exist.
#[tokio::test]
async fn root_binding_rejects_invalid_identity_graph_and_paths_in_both_lookup_paths() {
    let good = bound_graph();
    let mut cases = Vec::new();
    let mut changed = |label: &str, pointer: &str, value: Value| {
        let mut body = good.clone();
        *body.pointer_mut(pointer).unwrap() = value;
        cases.push((label.to_string(), body));
    };
    changed("wrong request", "/root/requested/worker", json!("tool"));
    changed(
        "foreign authority",
        "/root/requested/worker",
        json!("other.example/${SCENARIO_LITERAL}/tool"),
    );
    changed("wrong selector", "/root/requested/version", json!("*"));
    changed("unsupported schema", "/root/schema", json!(2));
    changed("missing node", "/root/name", json!("absent"));
    changed("root traversal", "/root/name", json!("../tool"));
    changed("root version mismatch", "/root/version", json!("9.0.0"));
    changed("unsafe node", "/graph/1/name", json!(OPAQUE));
    changed("unsafe dependency", "/graph/0/name", json!("../queue"));
    changed("unsafe version", "/graph/0/version", json!("../1.2.3"));
    changed("opaque edge", "/edges/0/from", json!(OPAQUE));
    changed("foreign edge", "/edges/0/to", json!("other.example/queue"));
    changed("missing edge node", "/edges/0/to", json!("absent"));
    changed("unrelated graph", "/edges", json!([]));
    changed(
        "cyclic edges",
        "/edges",
        json!([
            {"from":"tool", "to":"queue"}, {"from":"queue", "to":"tool"}
        ]),
    );
    let mut absent = good.clone();
    absent.as_object_mut().unwrap().remove("root");
    cases.push((
        "no binding (container and basename cannot prove identity)".into(),
        absent,
    ));
    let mut ambiguous = good.clone();
    ambiguous["graph"]
        .as_array_mut()
        .unwrap()
        .push(node("tool"));
    cases.push(("duplicate name".into(), ambiguous));
    for alias in ["../tool", "a/b", "${LITERAL}", "..", "C:\\tool"] {
        let mut body = good.clone();
        body["graph"][1]["alias_of"] = alias.into();
        cases.push((format!("unsafe alias {alias}"), body));
    }
    for (label, body) in cases {
        let server = MockServer::start().await;
        Mock::given(matchers::method("POST"))
            .and(matchers::path("/resolve"))
            .and(matchers::body_partial_json(
                json!({"worker": OPAQUE, "version": "next"}),
            ))
            .respond_with(ResponseTemplate::new(200).set_body_json(body))
            .expect(2)
            .mount(&server)
            .await;
        TEST_REGISTRY
            .scope(server.uri(), async {
                let reference = format!("{REGISTRY}/{OPAQUE}");
                let graph = resolve_graph_on("tool", &reference, "next", Platform::Vm)
                    .await
                    .unwrap_err();
                let exact = resolve_package_on("tool", &reference, "next", Platform::Vm)
                    .await
                    .unwrap_err();
                assert_eq!(graph.code(), exact.code(), "{label}");
                assert!(
                    matches!(
                        graph.code(),
                        "PACKAGE_NOT_RESOLVED" | "REGISTRY_NAME_REFUSED"
                    ),
                    "{label}: {graph}"
                );
            })
            .await;
        server.verify().await;
        assert_eq!(
            server.received_requests().await.unwrap().len(),
            2,
            "{label}"
        );
    }
}

#[test]
fn root_binding_rejects_wrong_exact_version_ranges_and_competing_same_name_root() {
    for range in ["1.2.4", "^2.0"] {
        let mut body = bound_graph();
        body["root"]["requested"]["version"] = range.into();
        let error = validate_resolution(
            "tool",
            DEFAULT_REGISTRY,
            OPAQUE,
            range,
            serde_json::from_value(body).unwrap(),
        )
        .unwrap_err();
        assert_eq!(error.code(), "PACKAGE_NOT_RESOLVED");
    }
    let mut body = bound_graph();
    body["root"]["requested"]["worker"] = "queue".into();
    let error = validate_resolution(
        "tool",
        DEFAULT_REGISTRY,
        "queue",
        "next",
        serde_json::from_value(body).unwrap(),
    )
    .unwrap_err();
    assert!(
        error
            .to_string()
            .contains("conflicts with the requested graph node")
    );
}

#[test]
fn root_binding_is_strict_inside_the_optional_extension_only() {
    for pointer in ["/root", "/root/requested"] {
        let mut body = bound_graph();
        body.pointer_mut(pointer)
            .unwrap()
            .as_object_mut()
            .unwrap()
            .insert("unknown".into(), true.into());
        assert!(serde_json::from_value::<ResolveResponse>(body).is_err());
    }
    let mut body = bound_graph();
    body["root"].as_object_mut().unwrap().remove("requested");
    assert!(serde_json::from_value::<ResolveResponse>(body).is_err());
    let mut body = bound_graph();
    body["root"] = json!([
        binding(OPAQUE, "next", "tool"),
        binding(OPAQUE, "next", "queue")
    ]);
    assert!(serde_json::from_value::<ResolveResponse>(body).is_err());
    let mut body = bound_graph();
    body["unknown_top_level"] = true.into();
    assert!(serde_json::from_value::<ResolveResponse>(body).is_ok());
}

/// Explicit prerelease constraints use semver's whole conjunction, not a stripped version.
#[test]
fn root_binding_preserves_explicit_prerelease_constraints_and_ordinary_range_compatibility() {
    let cases = [
        ("1.2.3-rc.1", "1.2.3-rc.1", true),
        ("1.2.3-rc.2", "1.2.3-rc.1", false),
        ("1.2.3", "1.2.3-rc.1", false),
        ("=1.2.3-rc.1", "1.2.3-rc.1", true),
        ("=1.2.3-rc.2", "1.2.3-rc.1", false),
        ("=1.2.3-rc.1", "1.2.3", false),
        ("^1.2.3-rc.2", "1.2.3-rc.1", false),
        ("^1.2.3-rc.2", "1.2.3-rc.2", true),
        ("^1.2.3-rc.2", "1.2.3-rc.3", true),
        ("^1.2.3-rc.2", "1.2.3", true),
        ("^1.2.3-rc.2", "1.2.4-rc.1", false),
        (">=1.2.3-rc.2, <2.0.0", "1.2.3-rc.3", true),
        (">=1.2.3-rc.2, <2.0.0", "1.2.3-rc.1", false),
        (">=1.2.3-rc.2, <2.0.0", "1.3.0-rc.1", false),
        (">=1.2.3-rc.2, <2.0.0", "1.9.0", true),
        (">=1.2.3-rc.1, <1.2.3-rc.4", "1.2.3-rc.2", true),
        (">=1.2.3-rc.1, <1.2.3-rc.4", "1.2.3-rc.4", false),
        (">=1.2.3-rc.2, <1.2.3", "1.2.3-rc.3", true),
        (">=1.2.3, <1.3.0-rc.2", "1.3.0-rc.1", true),
        (">=1.2.3, <1.3.0-rc.2", "1.3.0-rc.2", false),
        (">=1.2.3, <1.3.0-rc.2", "1.2.4-rc.1", false),
        ("^1.0", "1.1.0-rc.1", true),
        ("^1.0", "2.0.0-rc.1", false),
    ];
    for explicit in [false, true] {
        for (selector, selected, accepted) in cases {
            let request = if explicit { OPAQUE } else { "tool" };
            let mut body =
                json!({"graph": [{"name": "tool", "version": selected, "type": "bundle"}]});
            if explicit {
                body["root"] = binding(request, selector, "tool");
                body["root"]["version"] = selected.into();
            }
            let result = validate_resolution(
                "tool",
                DEFAULT_REGISTRY,
                request,
                selector,
                serde_json::from_value(body).unwrap(),
            );
            assert_eq!(
                result.is_ok(),
                accepted,
                "explicit={explicit}, {selector} -> {selected}: {result:?}"
            );
            if let Err(error) = result {
                assert_eq!(error.code(), "PACKAGE_NOT_RESOLVED");
            }
        }
    }
}

#[test]
fn root_binding_rejects_valid_positional_arrays_in_raw_json() {
    // These are well-typed, declaration-ordered representations accepted by a derived
    // struct visitor. Only the object boundary must reject them; no Value round trip.
    let arrays = [
        r#"[1,{"worker":"${SCENARIO_LITERAL}/tool","version":"next"},"tool","1.2.3"]"#,
        r#"{"schema":1,"requested":["${SCENARIO_LITERAL}/tool","next"],"name":"tool","version":"1.2.3"}"#,
    ];
    for root in arrays {
        let raw = format!(
            r#"{{"graph":[{{"name":"tool","version":"1.2.3","type":"bundle"}}],"root":{root}}}"#
        );
        let error = serde_json::from_str::<ResolveResponse>(&raw).unwrap_err();
        assert!(
            error.to_string().contains("invalid type: sequence"),
            "{error}"
        );
    }
}

#[test]
fn root_binding_rejects_duplicate_fields_without_losing_raw_json_entries() {
    let request = r#"{"worker":"${SCENARIO_LITERAL}/tool","version":"next"}"#;
    let root = format!(r#"{{"schema":1,"requested":{request},"name":"tool","version":"1.2.3"}}"#);
    let graph = r#""graph":[{"name":"tool","version":"1.2.3","type":"bundle"}]"#;
    let mut cases = vec![
        (
            "root",
            format!(r#"{{{graph},"root":{root},"root":{root}}}"#),
        ),
        ("root", format!(r#"{{{graph},"root":null,"root":{root}}}"#)),
        ("root", format!(r#"{{{graph},"root":{root},"root":null}}"#)),
        ("root", format!(r#"{{{graph},"root":null,"root":null}}"#)),
    ];
    for (field, value) in [
        ("schema", "1"),
        ("requested", request),
        ("name", r#""tool""#),
        ("version", r#""1.2.3""#),
    ] {
        let duplicate = format!(r#"{},"{field}":{value}}}"#, root.strip_suffix('}').unwrap());
        cases.push((field, format!(r#"{{{graph},"root":{duplicate}}}"#)));
    }
    for (field, value) in [
        ("worker", r#""${SCENARIO_LITERAL}/tool""#),
        ("version", r#""next""#),
    ] {
        let duplicate = format!(
            r#"{},"{field}":{value}}}"#,
            request.strip_suffix('}').unwrap()
        );
        let duplicate_root = root.replace(request, &duplicate);
        cases.push((field, format!(r#"{{{graph},"root":{duplicate_root}}}"#)));
    }
    for (field, raw) in cases {
        let error = serde_json::from_str::<ResolveResponse>(&raw).unwrap_err();
        assert!(
            error
                .to_string()
                .contains(&format!("duplicate field `{field}`")),
            "{error}: {raw}"
        );
    }
}

#[test]
fn root_binding_null_and_unknown_field_boundaries_remain_explicit() {
    for alias in ["", r#", "alias_of":"canonical-tool""#] {
        for root in ["", r#", "root":null"#] {
            let raw = format!(
                r#"{{"graph":[{{"name":"tool","version":"1.2.3","type":"bundle"{alias}}}],"future_metadata":true{root}}}"#
            );
            let response: ResolveResponse = serde_json::from_str(&raw).unwrap();
            assert!(response.root.is_none());
            assert!(
                validate_resolution("renamed", DEFAULT_REGISTRY, "tool", "1.2.3", response).is_ok()
            );
            let error = validate_resolution(
                "tool",
                DEFAULT_REGISTRY,
                OPAQUE,
                "1.2.3",
                serde_json::from_str(&raw).unwrap(),
            )
            .unwrap_err();
            assert_eq!(error.code(), "PACKAGE_NOT_RESOLVED");
        }
    }
    let root = r#"{"schema":1,"requested":{"worker":"${SCENARIO_LITERAL}/tool","version":"next"},"name":"tool","version":"1.2.3"}"#;
    for invalid in [
        root.replace(
            r#"{"worker":"${SCENARIO_LITERAL}/tool","version":"next"}"#,
            "null",
        ),
        root.replace(r#""schema":1"#, r#""schema":null"#),
        root.replace(r#""worker":"${SCENARIO_LITERAL}/tool""#, r#""worker":null"#),
        root.replace(r#""name":"tool""#, r#""name":null"#),
        root.replace(r#""version":"1.2.3""#, r#""version":null"#),
        root.replace(r#""version":"next""#, r#""version":null"#),
        root.replace(r#""schema":1"#, r#""schema":1,"unknown":true"#),
        root.replace(r#""version":"next""#, r#""version":"next","unknown":true"#),
    ] {
        let raw = format!(r#"{{"graph":[],"root":{invalid}}}"#);
        assert!(
            serde_json::from_str::<ResolveResponse>(&raw).is_err(),
            "{raw}"
        );
    }
    // Unknown schema is well-typed but must still fail semantic validation.
    let raw = format!(
        r#"{{"graph":[{{"name":"tool","version":"1.2.3","type":"bundle"}}],"root":{}}}"#,
        root.replace(r#""schema":1"#, r#""schema":2"#)
    );
    let error = validate_resolution(
        "tool",
        DEFAULT_REGISTRY,
        OPAQUE,
        "next",
        serde_json::from_str(&raw).unwrap(),
    )
    .unwrap_err();
    assert!(
        error
            .to_string()
            .contains("unsupported registry root binding schema")
    );
}

#[cfg(unix)]
fn bundle(name: &str) -> Vec<u8> {
    let encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    let mut archive = tar::Builder::new(encoder);
    let body =
        format!("name: {name}\nruntime:\n  type: javascript\nscripts:\n  start: node main.js\n");
    let mut header = tar::Header::new_gnu();
    header.set_size(body.len() as u64);
    header.set_mode(0o644);
    header.set_cksum();
    archive
        .append_data(&mut header, BUNDLE_MANIFEST, body.as_bytes())
        .unwrap();
    archive.into_inner().unwrap().finish().unwrap()
}

#[cfg(unix)]
async fn bundle_registry() -> MockServer {
    let server = MockServer::start().await;
    let mut artifacts = BTreeMap::new();
    for name in ["tool", "queue"] {
        let archive = bundle(name);
        artifacts.insert(
            name,
            (
                format!("{}/{name}.tar.gz", server.uri()),
                hex::encode(Sha256::digest(&archive)),
            ),
        );
        Mock::given(matchers::method("GET"))
            .and(matchers::path(format!("/{name}.tar.gz")))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(archive))
            .expect(1)
            .mount(&server)
            .await;
    }
    Mock::given(matchers::method("POST"))
        .and(matchers::path("/resolve"))
        .respond_with(move |request: &wiremock::Request| {
            let request: Value = serde_json::from_slice(&request.body).unwrap();
            let wanted = request["worker"].as_str().unwrap();
            let selector = request["version"].as_str().unwrap();
            assert!(
                ["tool", "queue", "console", OPAQUE].contains(&wanted),
                "{request}"
            );
            assert!(
                ["next", "^1.0", "*", "1.2.3"].contains(&selector),
                "{request}"
            );
            let targets = if wanted == "queue" {
                host_targets()
            } else {
                Platform::Vm.targets()
            };
            assert!(
                targets.contains(&request["target"].as_str().unwrap()),
                "{request}"
            );
            let root_name = if wanted == OPAQUE { "tool" } else { wanted };
            let worker = |name: &str| {
                let canonical = if name == "console" { "tool" } else { name };
                let (url, digest) = &artifacts[canonical];
                let mut node = json!({"name": name, "version": "1.2.3", "type": "bundle",
                    "archive_url": url, "sha256": digest});
                if canonical != name {
                    node["alias_of"] = canonical.into();
                }
                node
            };
            let mut response = if root_name == "queue" {
                json!({"graph": [worker("queue")], "edges": []})
            } else {
                json!({"graph": [worker("queue"), worker(root_name)],
                    "edges": [{"from": root_name, "to": "queue"}]})
            };
            if wanted == OPAQUE {
                response["root"] = binding(wanted, selector, root_name);
            }
            ResponseTemplate::new(200).set_body_json(response)
        })
        .mount(&server)
        .await;
    server
}

/// Plan -> exact HTTP resolution -> lock -> verified archive/cache -> raw text persistence.
/// No injected planner resolver, no preseeded lock/cache, no executable is started.
#[cfg(unix)]
#[tokio::test]
async fn root_binding_survives_planning_exact_lock_install_cache_and_raw_editing() {
    use crate::{ComposeFile, WorkerSource, dependencies, edit, lockfile};
    for (raw_name, wanted) in [
        ("tool", "tool"),
        ("$${SCENARIO_LITERAL}/tool", OPAQUE),
        ("console", "console"),
    ] {
        for selector in [Some("next"), Some("^1.0"), None] {
            let dir = tempfile::tempdir().unwrap();
            let cache = tempfile::tempdir().unwrap();
            let path = dir.path().join("worker-compose.yaml");
            let initial = "containers:\n  local: {worker: path://.}\n";
            let file = ComposeFile::parse(initial, &path).unwrap();
            let raw_reference =
                format!("${{III_ROOT_BINDING_TEST_REGISTRY:-{REGISTRY}}}/{raw_name}");
            let suffix = selector.map(|s| format!("@{s}")).unwrap_or_default();
            let input: edit::WorkerInput = serde_json::from_value(json!({
                "worker": format!("package://{raw_reference}{suffix}"),
                "runtime": {"base_image": "alpine:3.20"}
            }))
            .unwrap();
            let mut asked = input.parse().unwrap();
            // The declared container is not proof of a package/root identity.
            asked.key = "renamed".into();
            let server = bundle_registry().await;
            TEST_REGISTRY
                .scope(server.uri(), async {
                    let plan = dependencies::plan(&file, &[asked], &dependencies::Policy::Add)
                        .await
                        .unwrap();
                    assert_eq!(
                        plan.containers
                            .iter()
                            .map(|c| c.key.as_str())
                            .collect::<Vec<_>>(),
                        ["queue", "renamed"]
                    );
                    let root = plan.containers.iter().find(|c| c.key == "renamed").unwrap();
                    assert_eq!(root.start_after, ["queue"]);
                    assert_eq!(
                        root.source,
                        edit::Source::Package {
                            reference: raw_reference.clone(),
                            version: Some(selector.unwrap_or("1.2.3").into())
                        }
                    );
                    assert_eq!(plan.selected_versions["renamed"], "1.2.3");
                    let mut text = initial.to_string();
                    for declaration in &plan.containers {
                        text = match edit::upsert_container(&text, declaration).unwrap() {
                            edit::Outcome::Added(text) | edit::Outcome::Replaced { text, .. } => {
                                text
                            }
                            edit::Outcome::Unchanged => text,
                        };
                    }
                    assert!(
                        text.contains(&format!("package://{raw_reference}")),
                        "{text}"
                    );
                    assert_eq!(
                        edit::upsert_container(&text, root).unwrap(),
                        edit::Outcome::Unchanged
                    );
                    let mut parsed = ComposeFile::parse(&text, &path).unwrap();
                    assert_eq!(
                        parsed.containers["renamed"].worker,
                        WorkerSource::Package {
                            reference: format!("{REGISTRY}/{wanted}")
                        }
                    );
                    assert_eq!(
                        crate::vm_binary::platform(&parsed.containers["renamed"]),
                        Platform::Vm
                    );
                    let force = plan.selected_versions.keys().cloned().collect();
                    let mut prepared = lockfile::prepare_metadata_with_versions(
                        &mut parsed,
                        &force,
                        &plan.selected_versions,
                    )
                    .await
                    .unwrap();
                    let resolved = parsed.containers["renamed"]
                        .resolved_package
                        .as_ref()
                        .unwrap();
                    assert_eq!(
                        resolved.name,
                        if wanted == "console" {
                            "console"
                        } else {
                            "tool"
                        }
                    );
                    assert_eq!(package_name(resolved), "tool");
                    assert_eq!(resolved.registry, format!("https://{REGISTRY}"));
                    prepared.install(cache.path()).await.unwrap();
                    assert!(
                        prepared
                            .install_statuses()
                            .values()
                            .all(|s| *s == InstallStatus::Downloaded)
                    );
                    prepared.write_if_changed().unwrap();
                    let lock: Value = serde_yaml::from_str(
                        &std::fs::read_to_string(lockfile::lock_path(&path)).unwrap(),
                    )
                    .unwrap();
                    assert_eq!(
                        lock["containers"]["renamed"]["worker"],
                        format!("package://{REGISTRY}/{wanted}")
                    );
                    assert_eq!(
                        lock["containers"]["renamed"]["requested"],
                        selector.unwrap_or("1.2.3")
                    );
                    assert_eq!(
                        lock["containers"]["renamed"]["resolved"]["name"],
                        resolved.name
                    );
                    let installed =
                        install_resolved_on("renamed", resolved, Platform::Vm, cache.path())
                            .await
                            .unwrap();
                    assert_eq!(installed.status, InstallStatus::Cached);
                    let Payload::Bundle(install_dir) = &installed.payload else {
                        panic!("expected bundle")
                    };
                    assert!(install_dir.starts_with(cache.path()));
                    assert_eq!(
                        crate::manifest::read_manifest(install_dir)
                            .unwrap()
                            .unwrap()
                            .name
                            .as_deref(),
                        Some("tool")
                    );
                    let artifact = &resolved.artifacts[host_target()];
                    assert_eq!(artifact.sha256, hex::encode(Sha256::digest(bundle("tool"))));
                    assert!(
                        cache_matches(
                            install_dir,
                            &artifact.sha256,
                            &crate::shutdown::BlockingCancellation::default()
                        )
                        .unwrap()
                    );
                    // Canonical and existing safe alias references must reuse the identical entry.
                    for name in ["tool", "console"] {
                        let canonical = resolve_package_on(
                            name,
                            &format!("{REGISTRY}/{name}"),
                            "1.2.3",
                            Platform::Vm,
                        )
                        .await
                        .unwrap();
                        let reused =
                            install_resolved_on(name, &canonical, Platform::Vm, cache.path())
                                .await
                                .unwrap();
                        assert_eq!(reused.status, InstallStatus::Cached);
                        assert_eq!(reused.payload, installed.payload);
                    }
                    let requests = server.received_requests().await.unwrap();
                    let posts: Vec<Value> = requests
                        .iter()
                        .filter(|r| r.method == "POST")
                        .map(|r| serde_json::from_slice(&r.body).unwrap())
                        .collect();
                    assert!(
                        posts
                            .iter()
                            .any(|r| r["worker"] == wanted
                                && r["version"] == selector.unwrap_or("*"))
                    );
                    assert!(
                        posts
                            .iter()
                            .any(|r| r["worker"] == wanted && r["version"] == "1.2.3")
                    );
                    assert_eq!(requests.iter().filter(|r| r.method == "GET").count(), 2);
                })
                .await;
            server.verify().await;
        }
    }
}

#[tokio::test]
async fn root_binding_cannot_merge_a_different_package_at_the_declared_container_key() {
    let server = MockServer::start().await;
    Mock::given(matchers::method("POST"))
        .and(matchers::path("/resolve"))
        .respond_with(ResponseTemplate::new(200).set_body_json(bound_graph()))
        .expect(1)
        .mount(&server)
        .await;
    let dir = tempfile::tempdir().unwrap();
    let file = crate::ComposeFile::parse(
        "containers:\n  local: {worker: path://.}\n",
        dir.path().join("worker-compose.yaml"),
    )
    .unwrap();
    let asked = crate::edit::NewContainer {
        // The graph's queue is a dependency, not the requested tool.
        key: "queue".into(),
        source: crate::edit::Source::Package {
            reference: format!("{REGISTRY}/$${{SCENARIO_LITERAL}}/tool"),
            version: Some("next".into()),
        },
        start_after: vec![],
        fields: Default::default(),
    };
    let error = TEST_REGISTRY
        .scope(
            server.uri(),
            crate::dependencies::plan(&file, &[asked], &crate::dependencies::Policy::Add),
        )
        .await
        .unwrap_err();
    assert!(
        error
            .to_string()
            .contains("container key conflicts with another registry graph node")
    );
    server.verify().await;
}

#[tokio::test]
async fn root_binding_keeps_dependency_edges_registry_qualified() {
    let server = MockServer::start().await;
    Mock::given(matchers::method("POST"))
        .and(matchers::path("/resolve"))
        .respond_with(ResponseTemplate::new(200).set_body_json(bound_graph()))
        .expect(2)
        .mount(&server)
        .await;
    let dir = tempfile::tempdir().unwrap();
    let file = crate::ComposeFile::parse(
        "containers:\n  local: {worker: path://.}\n",
        dir.path().join("worker-compose.yaml"),
    )
    .unwrap();
    let asked: Vec<_> = [("one.example", "one"), ("two.example", "two")]
        .into_iter()
        .map(|(registry, key)| crate::edit::NewContainer {
            key: key.into(),
            source: crate::edit::Source::Package {
                reference: format!("{registry}/$${{SCENARIO_LITERAL}}/tool"),
                version: Some("next".into()),
            },
            start_after: vec![],
            fields: Default::default(),
        })
        .collect();
    let error = TEST_REGISTRY
        .scope(
            server.uri(),
            crate::dependencies::plan(&file, &asked, &crate::dependencies::Policy::Add),
        )
        .await
        .unwrap_err();
    // Same safe dependency name at two authorities cannot become one declaration.
    assert_eq!(error.code(), "INVALID_WORKER_SPEC");
    server.verify().await;
}

#[cfg(unix)]
#[tokio::test]
async fn root_binding_never_publishes_an_archive_with_a_different_digest() {
    let server = MockServer::start().await;
    let mut body = bound_graph();
    body["graph"][1]["archive_url"] = format!("{}/wrong.tar.gz", server.uri()).into();
    let expected = hex::encode(Sha256::digest(bundle("tool")));
    body["graph"][1]["sha256"] = expected.clone().into();
    Mock::given(matchers::method("POST"))
        .and(matchers::path("/resolve"))
        .respond_with(ResponseTemplate::new(200).set_body_json(body))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(matchers::method("GET"))
        .and(matchers::path("/wrong.tar.gz"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(bundle("other-package")))
        .expect(1)
        .mount(&server)
        .await;
    let cache = tempfile::tempdir().unwrap();
    TEST_REGISTRY
        .scope(server.uri(), async {
            let resolved = resolve_package_on(
                "renamed",
                &format!("{REGISTRY}/{OPAQUE}"),
                "next",
                Platform::Vm,
            )
            .await
            .unwrap();
            let install_dir = cache_install_dir(
                &registry_cache_root(cache.path(), &resolved),
                &resolved,
                host_target(),
                &expected,
            );
            let error = install_resolved_on("renamed", &resolved, Platform::Vm, cache.path())
                .await
                .unwrap_err();
            assert_eq!(error.code(), "PACKAGE_DIGEST_MISMATCH");
            assert!(!install_dir.exists());
        })
        .await;
    server.verify().await;
}
