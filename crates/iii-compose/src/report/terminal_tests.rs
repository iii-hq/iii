use super::*;

fn progress(workers: usize) -> Console {
    let mut startup = StartupRows::new(true);
    startup.engine.state = RowState::Ready {
        what: "Ready".to_string(),
        elapsed: Duration::from_secs(2),
    };
    startup.downloads.state = RowState::Skipped("No downloads".to_string());
    startup.containers.state = RowState::Starting {
        what: "Starting".to_string(),
        began: Instant::now(),
    };
    Console {
        startup: Some(startup),
        rows: (0..workers)
            .map(|index| Row {
                key: format!("worker-{index:02}"),
                depth: index % 3,
                state: RowState::Starting {
                    what: "waiting for engine registration".to_string(),
                    began: Instant::now(),
                },
            })
            .collect(),
        ..Console::default()
    }
}

#[tokio::test]
#[ignore = "subprocess fixture for real update producer and owner isolation"]
async fn update_producer_fixture() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("worker-compose.yaml");
    std::fs::write(
        &path,
        "containers:\n  api:\n    worker: package://fixture.invalid/api\n    version: '1.0.0'\n",
    )
    .unwrap();
    let lock = serde_json::json!({"version": 1, "containers": {"api": {
        "worker": "package://fixture.invalid/api", "requested": "1.0.0", "resolved": {
            "name": "api", "registry": "https://fixture.invalid", "version": "1.0.0", "type": "binary",
            "artifacts": {(crate::registry::host_target()): {"url": "https://fixture.invalid/api.tar.gz", "sha256": "a".repeat(64)}}
        }
    }}});
    std::fs::write(
        crate::lockfile::lock_path(&path),
        serde_yaml::to_string(&lock).unwrap(),
    )
    .unwrap();
    let mut startup = StartupProgress::start(true, &path);
    startup.engine_ready();
    in_project_scope(&path, async {
        plan(&[("api".into(), 0), ("unaffected".into(), 1)]);
        ready("api", Duration::ZERO);
        ready("unaffected", Duration::ZERO);
    })
    .await;
    startup.finish(true, "Ready");
    let daemon = crate::daemon::Daemon::start(
        "ws://127.0.0.1:1/ws".into(),
        "update-fixture".into(),
        None,
        crate::daemon::EnginePolicy::External,
    );
    std::fs::write(crate::lockfile::lock_path(&path), "invalid: lock").unwrap();
    let no_op = daemon
        .operations
        .create_with_id("producer-one".into(), 1)
        .await
        .unwrap();
    let result = daemon
        .update(Some(&path), &["api@1.0.0".into()], "producer-one".into())
        .await
        .unwrap();
    assert_eq!(serde_json::to_value(result).unwrap()["changed"], false);
    let event = no_op.snapshot().await.last_event.unwrap();
    assert_eq!(event.phase, "unchanged");
    assert_eq!(event.detail, "Everything already up to date.");
    assert_eq!(event.container.as_deref(), Some("api"));
    assert_eq!(
        std::fs::read_to_string(crate::lockfile::lock_path(&path)).unwrap(),
        "invalid: lock"
    );
    std::fs::write(
        crate::lockfile::lock_path(&path),
        serde_yaml::to_string(&lock).unwrap(),
    )
    .unwrap();
    {
        let state = console().lock().unwrap();
        assert_eq!(state.rows.len(), 2);
        let owner = UpdateOwner {
            project: canonical_project(&path),
            operation: "producer-one".into(),
        };
        assert!(matches!(
            state.updates[&owner]["api"].state,
            UpdateState::Unchanged(_)
        ));
    }
    {
        use wiremock::{Mock, MockServer, ResponseTemplate, matchers};
        let server = MockServer::start().await;
        let arrived = std::sync::Arc::new(tokio::sync::Notify::new());
        let notify = arrived.clone();
        Mock::given(matchers::method("POST"))
            .and(matchers::path("/resolve"))
            .respond_with(move |_: &wiremock::Request| {
                notify.notify_one();
                ResponseTemplate::new(400).set_delay(Duration::from_secs(30))
            })
            .mount(&server)
            .await;
        let operation = daemon
            .operations
            .create_with_id("slow-update".into(), 1)
            .await
            .unwrap();
        let selected = ["api@latest".into()];
        let future = crate::registry::TEST_REGISTRY.scope(
            server.uri(),
            daemon.update(Some(&path), &selected, "slow-update".into()),
        );
        tokio::pin!(future);
        tokio::select! {
            result = &mut future => panic!("update ended before registry delay: {result:?}"),
            _ = tokio::time::timeout(Duration::from_secs(5), arrived.notified()) => {}
        }
        {
            let state = console().lock().unwrap();
            let owner = UpdateOwner {
                project: canonical_project(&path),
                operation: "slow-update".into(),
            };
            assert!(
                matches!(&state.updates[&owner]["api"].state, UpdateState::Active(text) if text == "Resolving dependencies")
            );
            assert_eq!(state.rows.len(), 2);
        }
        let snapshot = operation.snapshot().await;
        let event = snapshot.last_event.unwrap();
        assert_eq!(event.operation_id, "slow-update");
        assert_eq!(event.container.as_deref(), Some("api"));
        assert_eq!(event.phase, "resolving");
        operation.cancel();
        let result = tokio::time::timeout(Duration::from_secs(2), &mut future)
            .await
            .unwrap();
        assert!(result.is_err());
        let state = console().lock().unwrap();
        let owner = UpdateOwner {
            project: canonical_project(&path),
            operation: "slow-update".into(),
        };
        assert!(matches!(
            state.updates[&owner]["api"].state,
            UpdateState::Error(_)
        ));
    }
    update_begin(&path, "producer-two", &["api".into()]);
    update_status(&path, "producer-one", "api", "stale event", true);
    update_finish(&path, "producer-one");
    {
        let state = console().lock().unwrap();
        let owner = UpdateOwner {
            project: canonical_project(&path),
            operation: "producer-two".into(),
        };
        assert!(matches!(
            state.updates[&owner]["api"].state,
            UpdateState::Active(_)
        ));
    }
    let other = dir.path().join("other.yaml");
    std::fs::write(&other, "containers: {}\n").unwrap();
    update_begin(&other, "other-op", &["api".into()]);
    in_project_scope(&other, async {
        plan(&[("api".into(), 0)]);
        failed("api", "FIXTURE", "other project failure");
    })
    .await;
    assert!(matches!(
        console().lock().unwrap().rows[0].state,
        RowState::Ready { .. }
    ));
    update_finish(&other, "other-op");
    update_finish(&path, "producer-two");
    update_begin(&path, "producer-three", &["api".into()]);
    update_status(
        &path,
        "producer-three",
        "api",
        "Updating 1.0.0 → 2.0.0",
        false,
    );
    update_status(
        &path,
        "producer-three",
        "api",
        "Updated 1.0.0 → 2.0.0",
        true,
    );
    update_finish(&path, "producer-three");
    let owner = UpdateOwner {
        project: canonical_project(&path),
        operation: "producer-three".into(),
    };
    assert!(matches!(
        console().lock().unwrap().updates[&owner]["api"].state,
        UpdateState::Updated(_)
    ));
    update_begin(
        &path,
        "partial-result",
        &["api".into(), "dependency".into()],
    );
    let operation = daemon
        .operations
        .create_with_id("partial-result".into(), 2)
        .await
        .unwrap();
    let versions = BTreeMap::from([
        ("api".into(), (Some("1.0.0".into()), "2.0.0".into())),
        ("dependency".into(), (None, "3.0.0".into())),
    ]);
    let up = crate::lifecycle::OpResult {
        operation_id: "partial-result-up".into(),
        status: crate::lifecycle::OpStatus::Failed,
        changed: true,
        containers: vec![crate::lifecycle::ContainerResult {
            container: "api".into(),
            state: crate::state::ChildStatus::Ready,
            changed: true,
            error: None,
        }],
        primary_error: None,
    };
    crate::daemon::finish_updated_workers(&path, "partial-result", &versions, &up).await;
    update_finish(&path, "partial-result");
    let event = operation.snapshot().await.last_event.unwrap();
    assert_eq!(event.operation_id, "partial-result");
    assert_eq!(event.container.as_deref(), Some("dependency"));
    assert_eq!(event.phase, "failed");
    assert_eq!(event.detail, "Update failed to 3.0.0");
    {
        let state = console().lock().unwrap();
        let owner = UpdateOwner {
            project: canonical_project(&path),
            operation: "partial-result".into(),
        };
        assert!(
            matches!(&state.updates[&owner]["api"].state, UpdateState::Updated(text) if text == "Updated 1.0.0 → 2.0.0")
        );
        assert!(
            matches!(&state.updates[&owner]["dependency"].state, UpdateState::Error(text) if text == "Update failed to 3.0.0")
        );
    }
    daemon.shutdown().await;
}

#[test]
fn real_update_producer_preserves_panel_and_isolates_old_events() {
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--ignored",
            "--exact",
            "report::terminal_tests::update_producer_fixture",
            "--nocapture",
        ])
        .env_remove("CLICOLOR_FORCE")
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stdout)
    );
    let text = String::from_utf8(output.stderr).unwrap();
    assert!(text.contains("Starting update"), "{text}");
    assert!(text.contains("Everything already up to date."), "{text}");
    assert!(text.contains("Updated 1.0.0 → 2.0.0"), "{text}");
    assert!(!text.contains("stale event"), "{text}");
    assert!(!text.contains('\u{1b}'), "{text}");
}

#[tokio::test]
#[ignore = "subprocess fixture for metadata-before-download and wire delivery"]
async fn artifact_update_fixture() {
    let state_root = tempfile::tempdir().unwrap();
    // SAFETY: this fixture runs alone in its subprocess, before daemon tasks.
    unsafe {
        std::env::set_var("III_COMPOSE_STATE_DIR", state_root.path());
    }
    use futures::{SinkExt, StreamExt};
    use wiremock::{Mock, MockServer, ResponseTemplate, matchers};
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let messages = std::sync::Arc::new(std::sync::Mutex::new(Vec::<serde_json::Value>::new()));
    let received = messages.clone();
    let ready_marker = state_root.path().join("ready");
    let marker_for_engine = ready_marker.clone();
    let transport = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        let mut socket = tokio_tungstenite::accept_async(stream).await.unwrap();
        let mut subscribed = false;
        while let Some(Ok(frame)) = socket.next().await {
            if let Ok(text) = frame.to_text() {
                let Ok(value) = serde_json::from_str::<serde_json::Value>(text) else {
                    continue;
                };
                received.lock().unwrap().push(value.clone());
                if value["type"] == "invokefunction" && value["invocation_id"].is_string() {
                    let function = value["function_id"].as_str().unwrap();
                    let result = match function {
                        "engine::workers::list" => {
                            if marker_for_engine.exists() {
                                serde_json::json!({"workers": [{"name": "api", "namespace": "default"}]})
                            } else {
                                serde_json::json!({"workers": []})
                            }
                        }
                        "engine::functions::list" => serde_json::json!({"functions": []}),
                        "configuration::migration-capabilities" => {
                            serde_json::json!({"source_priority_archive_revision": 1})
                        }
                        "configuration::migrate" => serde_json::json!({"action": "missing"}),
                        _ => serde_json::json!({"value": {}}),
                    };
                    socket.send(tokio_tungstenite::tungstenite::Message::Text(serde_json::json!({
                        "type": "invocationresult", "invocation_id": value["invocation_id"], "function_id": function, "result": result
                    }).to_string().into())).await.unwrap();
                }
                if value["type"] == "invokefunction"
                    && value["function_id"] == "compose::operation-progress"
                {
                    // Route through the registered SDK handler, the normal span path.
                    socket.send(tokio_tungstenite::tungstenite::Message::Text(serde_json::json!({
                        "type": "invokefunction", "invocation_id": uuid::Uuid::new_v4(), "function_id": "compose::operation-progress", "data": value["data"]
                    }).to_string().into())).await.unwrap();
                }
                if !subscribed && value["function_id"] == "compose::operation-progress" {
                    subscribed = true;
                    socket.send(tokio_tungstenite::tungstenite::Message::Text(serde_json::json!({
                        "type": "registertrigger", "id": "fixture-progress", "trigger_type": "compose-operation", "function_id": "fixture::progress", "config": {}, "namespace": "default"
                    }).to_string().into())).await.unwrap();
                }
            }
        }
    });
    let server = MockServer::start().await;
    let downloading = std::sync::Arc::new(tokio::sync::Notify::new());
    let signal = downloading.clone();
    Mock::given(matchers::method("GET"))
        .and(matchers::path("/artifact"))
        .respond_with(move |_: &wiremock::Request| {
            signal.notify_one();
            ResponseTemplate::new(200).set_delay(Duration::from_secs(30))
        })
        .mount(&server)
        .await;
    let url = format!("{}/artifact", server.uri());
    Mock::given(matchers::method("POST")).and(matchers::path("/resolve"))
        .respond_with(move |request: &wiremock::Request| {
            let input: serde_json::Value = serde_json::from_slice(&request.body).unwrap();
            let worker = input["worker"].as_str().unwrap();
            let names = if worker == "api" { vec!["api", "dependency"] } else { vec![worker] };
            let graph = names.into_iter().map(|name| serde_json::json!({"name": name, "version": "2.0.0", "type": "binary", "binaries": {(crate::registry::host_target()): {"url": url, "sha256": "a".repeat(64)}}})).collect::<Vec<_>>();
            ResponseTemplate::new(200).set_body_json(serde_json::json!({"graph": graph, "edges": if worker == "api" { serde_json::json!([{"from": "api", "to": "dependency"}]) } else { serde_json::json!([]) }}))
        }).mount(&server).await;
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("worker-compose.yaml");
    std::fs::write(
        &path,
        "containers:\n  api:\n    worker: package://fixture.invalid/api\n    version: next\n",
    )
    .unwrap();
    let lock = serde_json::json!({"version": 1, "containers": {"api": {"worker": "package://fixture.invalid/api", "requested": "next", "resolved": {"name": "api", "registry": "https://fixture.invalid", "version": "1.0.0", "type": "binary", "artifacts": {(crate::registry::host_target()): {"url": "https://fixture.invalid/old", "sha256": "b".repeat(64)}}}}}});
    std::fs::write(
        crate::lockfile::lock_path(&path),
        serde_yaml::to_string(&lock).unwrap(),
    )
    .unwrap();
    let mut startup = StartupProgress::start(true, &path);
    in_project_scope(&path, async {
        plan(&[("api".into(), 0)]);
        ready("api", Duration::ZERO);
    })
    .await;
    startup.finish(true, "Ready");
    let daemon = crate::daemon::Daemon::start(
        format!("ws://{address}"),
        "artifact-fixture".into(),
        None,
        crate::daemon::EnginePolicy::External,
    );
    tokio::time::sleep(Duration::from_millis(150)).await;
    let other_dir = tempfile::tempdir().unwrap();
    let other = other_dir.path().join("worker-compose.yaml");
    std::fs::create_dir_all(other_dir.path().join("api")).unwrap();
    std::fs::write(
        &other,
        "containers:\n  api:\n    worker: path://./api\n    scripts:\n      run: exit 1\n",
    )
    .unwrap();
    let foreground_before = {
        let state = console().lock().unwrap();
        (
            state.rows.clone(),
            state.downloads.clone(),
            state.startup.as_ref().unwrap().downloads.state.clone(),
            state.startup.as_ref().unwrap().containers.state.clone(),
        )
    };
    daemon
        .up(Some(&other), None, "foreign-up".into())
        .await
        .unwrap();
    let up_isolated = {
        let state = console().lock().unwrap();
        foreground_before
            == (
                state.rows.clone(),
                state.downloads.clone(),
                state.startup.as_ref().unwrap().downloads.state.clone(),
                state.startup.as_ref().unwrap().containers.state.clone(),
            )
    };
    std::fs::write(
        &other,
        "containers:\n  api:\n    worker: package://fixture.invalid/api\n    version: next\n",
    )
    .unwrap();
    let add_operation = daemon
        .operations
        .create_with_id("foreign-add".into(), 1)
        .await
        .unwrap();
    let foreign_selected = ["package://fixture.invalid/api@latest".into()];
    let add = crate::registry::TEST_REGISTRY.scope(
        server.uri(),
        daemon.add(Some(&other), &foreign_selected, "foreign-add".into()),
    );
    tokio::pin!(add);
    tokio::select! {
        result = &mut add => panic!("add ended before artifact: {result:?}"),
        result = tokio::time::timeout(Duration::from_secs(5), downloading.notified()) => { result.unwrap(); }
    }
    let add_isolated = {
        let state = console().lock().unwrap();
        foreground_before
            == (
                state.rows.clone(),
                state.downloads.clone(),
                state.startup.as_ref().unwrap().downloads.state.clone(),
                state.startup.as_ref().unwrap().containers.state.clone(),
            )
    };
    add_operation.cancel();
    assert!(
        tokio::time::timeout(Duration::from_secs(2), &mut add)
            .await
            .unwrap()
            .is_err()
    );
    assert!(
        up_isolated && add_isolated,
        "real entrypoint isolation: up={up_isolated}; add={add_isolated}"
    );
    while tokio::time::timeout(Duration::from_millis(20), downloading.notified())
        .await
        .is_ok()
    {}
    let operation = daemon
        .operations
        .create_with_id("artifact-update".into(), 1)
        .await
        .unwrap();
    let selected = ["api@latest".into()];
    let future = crate::registry::TEST_REGISTRY.scope(
        server.uri(),
        daemon.update(Some(&path), &selected, "artifact-update".into()),
    );
    tokio::pin!(future);
    tokio::select! {
        result = &mut future => panic!("ended before artifact: {result:?}"),
        result = tokio::time::timeout(Duration::from_secs(5), downloading.notified()) => { result.unwrap(); }
    }
    {
        let state = console().lock().unwrap();
        let owner = UpdateOwner {
            project: canonical_project(&path),
            operation: "artifact-update".into(),
        };
        assert!(
            matches!(&state.updates[&owner]["api"].state, UpdateState::Active(text) if text == "Updating 1.0.0 → 2.0.0")
        );
        assert!(
            matches!(&state.updates[&owner]["dependency"].state, UpdateState::Active(text) if text == "Updating to 2.0.0")
        );
    }
    tokio::time::sleep(Duration::from_millis(100)).await;
    {
        let frames = messages.lock().unwrap();
        for function in ["compose::operation-progress", "fixture::progress"] {
            assert!(
                frames.iter().any(|frame| frame["function_id"] == function
                    && frame["data"]["operation_id"] == "artifact-update"
                    && frame["data"]["container"] == "api"
                    && frame["data"]["detail"] == "Updating 1.0.0 → 2.0.0"),
                "missing delivery to {function}: {frames:?}"
            );
            assert!(frames.iter().any(|frame| frame["function_id"] == function
                && frame["data"]["container"] == "dependency"
                && frame["data"]["detail"] == "Updating to 2.0.0"));
        }
    }
    operation.cancel();
    assert!(
        tokio::time::timeout(Duration::from_secs(2), &mut future)
            .await
            .unwrap()
            .is_err()
    );
    {
        let state = console().lock().unwrap();
        let owner = UpdateOwner {
            project: canonical_project(&path),
            operation: "artifact-update".into(),
        };
        assert!(
            state.updates[&owner]
                .values()
                .all(|row| matches!(row.state, UpdateState::Error(_) | UpdateState::Cancelled(_)))
        );
    }
    Mock::given(matchers::method("GET"))
        .and(matchers::path("/artifact"))
        .respond_with(ResponseTemplate::new(400))
        .with_priority(1)
        .mount(&server)
        .await;
    let _failure = daemon
        .operations
        .create_with_id("artifact-failure".into(), 1)
        .await
        .unwrap();
    let result = crate::registry::TEST_REGISTRY
        .scope(
            server.uri(),
            daemon.update(Some(&path), &selected, "artifact-failure".into()),
        )
        .await;
    assert!(result.is_err());
    {
        let state = console().lock().unwrap();
        let owner = UpdateOwner {
            project: canonical_project(&path),
            operation: "artifact-failure".into(),
        };
        assert!(
            state.updates[&owner].values().all(
                |row| matches!(&row.state, UpdateState::Error(text) if text.contains("2.0.0"))
            )
        );
    }
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert!(messages.lock().unwrap().iter().any(|frame| {
        frame["function_id"] == "fixture::progress"
            && frame["data"]["operation_id"] == "artifact-failure"
            && frame["data"]["container"] == "api"
            && frame["data"]["phase"] == "failed"
            && frame["data"]["detail"]
                .as_str()
                .is_some_and(|text| text.contains("1.0.0 → 2.0.0"))
    }));
    // POSIX shell archives exercise real process restart and readiness on Unix;
    // metadata, cancellation, download failures, and wire delivery above run everywhere.
    #[cfg(unix)]
    {
        // A verified archive gets past acquisition and through the real restart;
        // the executable then fails readiness, not a synthetic OpResult.
        let mut archive = tar::Builder::new(Vec::new());
        let script = b"#!/bin/sh\nexit 1\n";
        let mut header = tar::Header::new_gnu();
        header.set_size(script.len() as u64);
        header.set_mode(0o755);
        header.set_cksum();
        archive
            .append_data(&mut header, "worker", &script[..])
            .unwrap();
        let archive = archive.into_inner().unwrap();
        let mut gzip = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        std::io::Write::write_all(&mut gzip, &archive).unwrap();
        let archive = gzip.finish().unwrap();
        use sha2::{Digest, Sha256};
        let digest = format!("{:x}", Sha256::digest(&archive));
        server.reset().await;
        Mock::given(matchers::method("GET"))
            .and(matchers::path("/artifact"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(archive))
            .mount(&server)
            .await;
        Mock::given(matchers::method("POST")).and(matchers::path("/resolve"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"graph": [{"name": "api", "version": "2.0.0", "type": "binary", "binaries": {(crate::registry::host_target()): {"url": format!("{}/artifact", server.uri()), "sha256": digest}}}], "edges": []}))).mount(&server).await;
        let _restart = daemon
            .operations
            .create_with_id("real-restart-failure".into(), 1)
            .await
            .unwrap();
        let result = tokio::time::timeout(
            Duration::from_secs(10),
            crate::registry::TEST_REGISTRY.scope(
                server.uri(),
                daemon.update(Some(&path), &selected, "real-restart-failure".into()),
            ),
        )
        .await
        .unwrap()
        .unwrap();
        let outcome = serde_json::to_value(result).unwrap();
        assert_eq!(
            outcome["not_required_failures"],
            serde_json::json!(["api"]),
            "{outcome}"
        );
        assert_eq!(outcome["changed"], true);
        assert_eq!(outcome["version"], "2.0.0");
        {
            let state = console().lock().unwrap();
            let owner = UpdateOwner {
                project: canonical_project(&path),
                operation: "real-restart-failure".into(),
            };
            assert!(
                matches!(&state.updates[&owner]["api"].state, UpdateState::Error(text) if text.contains("1.0.0 → 2.0.0"))
            );
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
        assert!(messages.lock().unwrap().iter().any(|frame| {
            frame["function_id"] == "fixture::progress"
                && frame["data"]["operation_id"] == "real-restart-failure"
                && frame["data"]["phase"] == "failed"
                && frame["data"]["detail"]
                    .as_str()
                    .is_some_and(|text| text.contains("1.0.0 → 2.0.0"))
        }));
        server.reset().await;
        let script = format!(
            "#!/bin/sh\ntouch '{}'\nexec sleep 30\n",
            ready_marker.display()
        );
        let mut archive = tar::Builder::new(Vec::new());
        let mut header = tar::Header::new_gnu();
        header.set_size(script.len() as u64);
        header.set_mode(0o755);
        header.set_cksum();
        archive
            .append_data(&mut header, "worker", script.as_bytes())
            .unwrap();
        let mut gzip = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        std::io::Write::write_all(&mut gzip, &archive.into_inner().unwrap()).unwrap();
        let archive = gzip.finish().unwrap();
        let digest = format!("{:x}", Sha256::digest(&archive));
        Mock::given(matchers::method("GET"))
            .and(matchers::path("/artifact"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(archive))
            .mount(&server)
            .await;
        Mock::given(matchers::method("POST")).and(matchers::path("/resolve"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"graph": [{"name": "api", "version": "3.0.0", "type": "binary", "binaries": {(crate::registry::host_target()): {"url": format!("{}/artifact", server.uri()), "sha256": digest}}}], "edges": []}))).mount(&server).await;
        let _success = daemon
            .operations
            .create_with_id("real-restart-success".into(), 1)
            .await
            .unwrap();
        let result = tokio::time::timeout(
            Duration::from_secs(10),
            crate::registry::TEST_REGISTRY.scope(
                server.uri(),
                daemon.update(Some(&path), &selected, "real-restart-success".into()),
            ),
        )
        .await
        .unwrap()
        .unwrap();
        let outcome = serde_json::to_value(result).unwrap();
        assert_eq!(outcome["changed"], true);
        assert_eq!(outcome["version"], "3.0.0");
        assert!(outcome.get("not_required_failures").is_none(), "{outcome}");
        {
            let state = console().lock().unwrap();
            let owner = UpdateOwner {
                project: canonical_project(&path),
                operation: "real-restart-success".into(),
            };
            assert!(
                matches!(&state.updates[&owner]["api"].state, UpdateState::Updated(text) if text == "Updated 2.0.0 → 3.0.0")
            );
            assert!(
                state
                    .rows
                    .iter()
                    .any(|row| row.key == "api" && matches!(row.state, RowState::Ready { .. }))
            );
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
        assert!(
            messages
                .lock()
                .unwrap()
                .iter()
                .any(|frame| frame["type"] == "invocationresult"
                    && frame["function_id"] == "compose::operation-progress"
                    && frame["result"]["detail"] == "Updated 2.0.0 → 3.0.0")
        );
        for function in ["compose::operation-progress", "fixture::progress"] {
            assert!(
                messages
                    .lock()
                    .unwrap()
                    .iter()
                    .any(|frame| frame["function_id"] == function
                        && frame["data"]["operation_id"] == "real-restart-success"
                        && frame["data"]["container"] == "api"
                        && frame["data"]["phase"] == "updated"
                        && frame["data"]["detail"] == "Updated 2.0.0 → 3.0.0")
            );
        }
        daemon
            .up(Some(&path), None, "after-update-up".into())
            .await
            .unwrap();
        {
            let mut state = console().lock().unwrap();
            assert!(
                state
                    .updates
                    .keys()
                    .all(|owner| owner.project != canonical_project(&path)),
                "old update badge survived independent up"
            );
            assert!(
                !state
                    .render(Some((40, 160)))
                    .contains("Updated 2.0.0 → 3.0.0")
            );
        }
    }
    daemon.shutdown().await;
    transport.abort();
}

#[test]
fn metadata_versions_precede_artifact_and_are_delivered_on_wire() {
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--ignored",
            "--exact",
            "report::terminal_tests::artifact_update_fixture",
            "--nocapture",
        ])
        .env("NO_COLOR", "1")
        .env_remove("CLICOLOR_FORCE")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn write_terminal(terminal: &mut vt100::Parser, output: &str) {
    // Match the newline conversion performed by a normal PTY (ONLCR).
    terminal.process(output.replace('\n', "\r\n").as_bytes());
}

fn screen_and_history(terminal: &mut vt100::Parser) -> String {
    let screen = terminal.screen_mut();
    let (_, width) = screen.size();
    screen.set_scrollback(usize::MAX);
    let history = screen.scrollback();
    let mut text = String::new();
    for offset in (1..=history).rev() {
        screen.set_scrollback(offset);
        text.push_str(&screen.rows(0, width).next().unwrap());
        text.push('\n');
    }
    screen.set_scrollback(0);
    text.push_str(&screen.contents());
    text
}

#[test]
fn update_overlay_preserves_tree_health_and_frozen_completion() {
    let mut state = progress(2);
    state.foreground_project = Some("foreground".into());
    state.foreground_active = true;
    state.startup.as_mut().unwrap().finish(true, "Ready");
    for row in &mut state.rows {
        row.state = RowState::Ready {
            what: "Running".into(),
            elapsed: Duration::ZERO,
        };
    }
    let owner = UpdateOwner {
        project: "foreground".into(),
        operation: "update-one".into(),
    };
    state.updates.insert(
        owner.clone(),
        BTreeMap::from([(
            "worker-00".into(),
            UpdateRow {
                state: UpdateState::Active("Updating 1.0.0 → 2.0.0".into()),
                began: Instant::now(),
                finished: None,
            },
        )]),
    );
    let mut terminal = vt100::Parser::new(24, 100, 1000);
    write_terminal(&mut terminal, &state.render(Some((24, 100))));
    let text = screen_and_history(&mut terminal);
    assert!(text.contains("Updating 1.0.0 → 2.0.0"), "{text}");
    assert!(text.contains("Running (2/2)"), "{text}");
    assert!(text.contains("worker-01 Running"), "{text}");
    state.rows[0].state = RowState::Failed;
    write_terminal(&mut terminal, &state.render(Some((24, 100))));
    let text = screen_and_history(&mut terminal);
    assert!(text.contains("Running (1/2)"), "{text}");
    assert!(text.contains("Failed · Updating"), "{text}");
    state.rows[0].state = RowState::Ready {
        what: "Running".into(),
        elapsed: Duration::ZERO,
    };
    let update = state
        .updates
        .get_mut(&owner)
        .unwrap()
        .get_mut("worker-00")
        .unwrap();
    update.state = UpdateState::Updated("Updated 1.0.0 → 2.0.0".into());
    update.finished = Some(Duration::from_secs(3));
    write_terminal(&mut terminal, &state.render(Some((24, 100))));
    let first = terminal.screen().contents();
    state.frame += 1;
    write_terminal(&mut terminal, &state.render(Some((24, 100))));
    assert_eq!(terminal.screen().contents(), first);
    assert_eq!(
        screen_and_history(&mut terminal)
            .matches("worker-00")
            .count(),
        1
    );
}

#[test]
fn update_static_output_has_no_frame_spam_and_qualifies_other_projects() {
    let mut state = progress(1);
    state.foreground_project = Some("foreground".into());
    state.rows[0].state = RowState::Ready {
        what: "Running".into(),
        elapsed: Duration::ZERO,
    };
    state.updates.insert(
        UpdateOwner {
            project: "other".into(),
            operation: "op-other".into(),
        },
        BTreeMap::from([(
            "worker-00".into(),
            UpdateRow {
                state: UpdateState::Active("Starting update".into()),
                began: Instant::now(),
                finished: None,
            },
        )]),
    );
    let first = state.render(None);
    assert!(first.contains("worker-00 Running"), "{first}");
    assert!(
        first.contains("worker-00 (other / op-other) Starting update"),
        "{first}"
    );
    assert!(!first.contains('\u{1b}'));
    for _ in 0..10 {
        state.frame += 1;
        assert!(state.render(None).is_empty());
    }
}

const DOWNLOAD_WORKERS: [(&str, usize); 13] = [
    ("llm-router", 0),
    ("provider-openai", 1),
    ("provider-anthropic", 1),
    ("context-manager", 1),
    ("cron", 0),
    ("harness", 2),
    ("iii-directory", 0),
    ("console", 0),
    ("session-manager", 0),
    ("queue", 0),
    ("web", 0),
    ("state", 0),
    ("shell", 0),
];

#[test]
fn downloaded_workers_keep_one_row_through_every_startup_phase() {
    for (height, width) in [(24, 80), (18, 120)] {
        let mut state = progress(0);
        state.startup.as_mut().unwrap().downloads.state = RowState::Starting {
            what: "Downloading (0/13)".to_string(),
            began: Instant::now(),
        };
        state.startup.as_mut().unwrap().containers.state = RowState::Waiting;
        let mut terminal = vt100::Parser::new(height, width, 1000);
        // Downloads register dynamically, and transfer amounts/rates grow wider
        // as chunks arrive. Real names must fit throughout, not only at 0%.
        for (key, _) in DOWNLOAD_WORKERS {
            state.downloads.push(Row {
                key: key.to_string(),
                depth: 1,
                state: RowState::Downloading {
                    downloaded: 0,
                    total: Some(8 * 1024 * 1024),
                    began: Instant::now() - Duration::from_millis(10),
                },
            });
            write_terminal(&mut terminal, &state.render(Some((height, width))));
        }
        for step in 1..=64 {
            for row in &mut state.downloads {
                if let RowState::Downloading { downloaded, .. } = &mut row.state {
                    *downloaded = step * 16 * 1024;
                }
            }
            write_terminal(&mut terminal, &state.render(Some((height, width))));
            assert!(!state.static_output, "{width}x{height}, chunk {step}");
        }
        for index in 0..13 {
            state.downloads[index].state = RowState::Downloaded {
                downloaded: 1024 * 1024,
                total: Some(1024 * 1024),
                elapsed: Duration::from_secs(1),
            };
            write_terminal(&mut terminal, &state.render(Some((height, width))));
        }
        state.startup.as_mut().unwrap().downloads.state = RowState::Ready {
            what: "Complete (13)".to_string(),
            elapsed: Duration::from_secs(1),
        };
        state.startup.as_mut().unwrap().containers.state = RowState::Starting {
            what: "Starting (0/13)".to_string(),
            began: Instant::now(),
        };
        state.rows = DOWNLOAD_WORKERS
            .iter()
            .map(|(key, depth)| Row {
                key: (*key).to_string(),
                depth: *depth,
                state: RowState::Waiting,
            })
            .collect();
        write_terminal(&mut terminal, &state.render(Some((height, width))));
        let text = screen_and_history(&mut terminal);
        assert!(!state.static_output, "{width}x{height}: {text}");
        assert_eq!(text.matches("100%").count(), 13, "{text}");
        for (index, (key, _)) in DOWNLOAD_WORKERS.iter().enumerate() {
            for phase in [
                "starting",
                "installing package",
                "configuring",
                "waiting for engine registration",
                "ready",
            ] {
                state.rows[index].state = if phase == "ready" {
                    RowState::Ready {
                        what: phase.to_string(),
                        elapsed: Duration::from_secs(1),
                    }
                } else {
                    RowState::Starting {
                        what: phase.to_string(),
                        began: Instant::now(),
                    }
                };
                write_terminal(&mut terminal, &state.render(Some((height, width))));
                let text = screen_and_history(&mut terminal);
                assert_eq!(text.matches(key).count(), 1, "{text}");
                assert!(text.contains(&format!("{key} {phase}")), "{text}");
            }
        }
        write_terminal(
            &mut terminal,
            &state.line("compose diagnostic", Some((height, width))),
        );
        state.startup.as_mut().unwrap().finish(true, "Ready");
        write_terminal(&mut terminal, &state.render(Some((height, width))));
        let text = screen_and_history(&mut terminal);
        for (key, _) in DOWNLOAD_WORKERS {
            assert_eq!(text.matches(key).count(), 1, "{text}");
            assert!(text.contains(&format!("✓ {key} ready")), "{text}");
        }
        assert!(!text.contains("100%"), "{text}");
        assert_eq!(text.matches("Engine Ready").count(), 1, "{text}");
        assert_eq!(text.matches("compose diagnostic").count(), 1, "{text}");
    }
}

#[test]
fn static_downloads_report_transitions_not_chunks_or_indentation_changes() {
    for size in [None, Some((3, 80)), Some((24, 20))] {
        let began = Instant::now();
        let mut state = Console {
            downloads: vec![Row {
                key: "provider-anthropic".to_string(),
                depth: 1,
                state: RowState::Downloading {
                    downloaded: 0,
                    total: None,
                    began,
                },
            }],
            ..Console::default()
        };
        let mut output = if size == Some((3, 80)) {
            // A previously animated panel becomes unsafe after a resize.
            state.render(Some((24, 80)));
            state.render(size)
        } else {
            state.render(size)
        };
        for chunk in 1..=512 {
            state.downloads[0].state = RowState::Downloading {
                downloaded: chunk * 16 * 1024,
                total: Some(8 * 1024 * 1024),
                began,
            };
            let update = state.render(size);
            assert!(update.is_empty(), "{size:?}, chunk {chunk}: {update}");
        }
        state.downloads[0].state = RowState::Downloaded {
            downloaded: 8 * 1024 * 1024,
            total: Some(8 * 1024 * 1024),
            elapsed: Duration::from_secs(1),
        };
        output.push_str(&state.render(size));
        state.rows = vec![Row {
            key: "provider-anthropic".to_string(),
            depth: 3,
            state: RowState::Waiting,
        }];
        assert!(
            state.render(size).is_empty(),
            "planning reprinted a download"
        );
        state.rows[0].state = RowState::Starting {
            what: "configuring".to_string(),
            began: Instant::now(),
        };
        output.push_str(&state.render(size));
        state.rows[0].state = RowState::Ready {
            what: "ready".to_string(),
            elapsed: Duration::from_secs(1),
        };
        output.push_str(&state.render(size));
        assert_eq!(output.matches("100%").count(), 1, "{output}");
        assert_eq!(output.matches("configuring").count(), 1, "{output}");
        assert_eq!(
            output.matches("✓ provider-anthropic ready").count(),
            1,
            "{output}"
        );
        assert!(!output.contains('\x1b'), "{output:?}");
    }
}

#[test]
fn completed_download_does_not_claim_worker_readiness() {
    let row = Row {
        key: "api".to_string(),
        depth: 1,
        state: RowState::Downloaded {
            downloaded: 1024,
            total: Some(1024),
            elapsed: Duration::from_secs(1),
        },
    };
    let text = render_row(&row, 0, false);
    assert!(text.contains("100%"), "{text}");
    assert!(!text.contains(OK), "{text}");
}

#[test]
fn startup_preserves_one_ready_line_per_worker_and_engine_at_small_terminal_sizes() {
    for (height, width) in [
        (24, 120),
        (16, 120),
        (15, 120),
        (14, 120),
        (40, 40),
        (24, 40),
    ] {
        let mut state = progress(13);
        let mut terminal = vt100::Parser::new(height, width, 1000);
        for frame in 0..20 {
            state.frame = frame;
            write_terminal(&mut terminal, &state.render(Some((height, width))));
        }
        for index in 0..state.rows.len() {
            state.rows[index].state = RowState::Ready {
                what: "ready".to_string(),
                elapsed: Duration::from_millis(612),
            };
            write_terminal(&mut terminal, &state.render(Some((height, width))));
        }
        state.startup.as_mut().unwrap().finish(true, "Ready");
        write_terminal(&mut terminal, &state.render(Some((height, width))));

        let text = screen_and_history(&mut terminal);
        for name in std::iter::once("Engine Ready".to_string())
            .chain((0..13).map(|index| format!("worker-{index:02} ready")))
        {
            assert_eq!(text.matches(&name).count(), 1, "{width}x{height}: {text}");
        }
    }
}

#[test]
fn adding_workers_beyond_the_screen_keeps_the_confirmed_engine_line_once() {
    let mut state = progress(0);
    let mut terminal = vt100::Parser::new(15, 80, 1000);
    write_terminal(&mut terminal, &state.render(Some((15, 80))));
    state.rows = progress(13).rows;
    for frame in 0..20 {
        state.frame = frame;
        write_terminal(&mut terminal, &state.render(Some((15, 80))));
    }

    let text = screen_and_history(&mut terminal);
    assert_eq!(text.matches("Engine Ready").count(), 1, "{text}");
}

#[test]
fn a_wrapping_status_switches_to_static_output_without_overwriting_prior_lines() {
    let mut state = progress(1);
    let mut terminal = vt100::Parser::new(20, 80, 1000);
    write_terminal(&mut terminal, "keep this log\n");
    write_terminal(&mut terminal, &state.render(Some((20, 80))));
    state.rows[0].state = RowState::Starting {
        what: "waiting for a worker in a very long directory ".repeat(3),
        began: Instant::now(),
    };
    for frame in 0..10 {
        state.frame = frame;
        write_terminal(&mut terminal, &state.render(Some((20, 80))));
    }
    state.rows[0].state = RowState::Ready {
        what: "ready".to_string(),
        elapsed: Duration::from_secs(1),
    };
    write_terminal(&mut terminal, &state.render(Some((20, 80))));

    let text = screen_and_history(&mut terminal);
    assert!(text.starts_with("keep this log\n"), "{text}");
    assert_eq!(text.matches("Engine Ready").count(), 1, "{text}");
    assert_eq!(text.matches("worker-00 ready").count(), 1, "{text}");
}

#[test]
fn smaller_panels_erase_removed_rows() {
    let mut state = progress(3);
    let mut terminal = vt100::Parser::new(24, 80, 1000);
    write_terminal(&mut terminal, &state.render(Some((24, 80))));
    state.rows.truncate(1);
    write_terminal(&mut terminal, &state.render(Some((24, 80))));

    let text = screen_and_history(&mut terminal);
    assert!(text.contains("worker-00"), "{text}");
    assert!(
        !text.contains("worker-01") && !text.contains("worker-02"),
        "{text}"
    );
}

#[test]
fn logs_remain_above_the_panel_even_when_they_wrap_or_span_lines() {
    let mut state = progress(2);
    let mut terminal = vt100::Parser::new(10, 80, 1000);
    write_terminal(&mut terminal, &state.render(Some((10, 80))));
    let message = format!("error: {}\nsecond diagnostic", "long path/".repeat(15));
    write_terminal(&mut terminal, &state.line(&message, Some((10, 80))));
    write_terminal(&mut terminal, &state.render(Some((10, 80))));

    let text = screen_and_history(&mut terminal);
    assert_eq!(text.matches("error:").count(), 1, "{text}");
    assert_eq!(text.matches("second diagnostic").count(), 1, "{text}");
    assert_eq!(text.matches("Engine Ready").count(), 1, "{text}");
}

#[test]
fn logging_during_static_fallback_restores_rows_erased_with_the_previous_panel() {
    let mut state = progress(1);
    let mut terminal = vt100::Parser::new(20, 80, 1000);
    write_terminal(&mut terminal, &state.render(Some((20, 80))));
    state.rows[0].state = RowState::Starting {
        what: "a status that now wraps onto multiple terminal lines ".repeat(3),
        began: Instant::now(),
    };
    write_terminal(&mut terminal, &state.line("new diagnostic", Some((20, 80))));

    let text = screen_and_history(&mut terminal);
    assert_eq!(text.matches("Engine Ready").count(), 1, "{text}");
}

#[test]
fn resizing_stops_cursor_updates_even_when_a_log_arrives_before_the_next_frame() {
    for size in [(3, 80), (24, 25), (40, 120)] {
        let mut state = progress(2);
        let mut terminal = vt100::Parser::new(24, 80, 1000);
        write_terminal(&mut terminal, &state.render(Some((24, 80))));
        terminal.screen_mut().set_size(size.0, size.1);

        let output = state.line("new diagnostic", Some(size));
        assert_eq!(output, "new diagnostic\n");
        write_terminal(&mut terminal, &output);
        assert!(state.render(Some(size)).is_empty());
        // Growing the terminal again must not repaint already committed rows.
        assert!(state.render(Some((40, 120))).is_empty());
    }
}

#[test]
fn missing_dimensions_use_static_state_changes_without_spinner_ticks() {
    let mut state = progress(1);
    let first = state.render(None);
    assert!(first.contains("Engine Ready"), "{first}");
    assert!(!FRAMES.iter().any(|frame| first.contains(frame)), "{first}");
    state.frame += 1;
    assert!(state.render(None).is_empty());
    state.rows[0].state = RowState::Ready {
        what: "ready".to_string(),
        elapsed: Duration::from_millis(25),
    };
    let output = state.render(None);
    assert!(output.contains("worker-00 ready"), "{output}");
    assert!(!output.contains("Engine"), "{output}");
}

#[test]
fn colored_unicode_rows_fit_by_display_width_and_not_ansi_byte_length() {
    let mut state = Console {
        rows: vec![Row {
            key: "\x1b[32m日本語\x1b[0m".to_string(),
            depth: 0,
            state: RowState::Waiting,
        }],
        ..Console::default()
    };
    let mut terminal = vt100::Parser::new(4, 20, 1000);
    write_terminal(&mut terminal, "saved log\n");
    for frame in 0..10 {
        state.frame = frame;
        write_terminal(&mut terminal, &state.render(Some((4, 20))));
    }
    let text = screen_and_history(&mut terminal);
    assert_eq!(text, "saved log\n· 日本語 Pending");
}

#[test]
fn wide_characters_that_would_wrap_do_not_move_the_cursor_into_earlier_logs() {
    let mut state = Console {
        rows: vec![Row {
            key: "a日本語日本語".to_string(),
            depth: 0,
            state: RowState::Waiting,
        }],
        ..Console::default()
    };
    let first = state.render(Some((10, 10)));
    assert!(!first.contains('\x1b'), "{first:?}");
    assert!(state.render(Some((10, 10))).is_empty());
}

fn cancelled_retry_output() -> String {
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--ignored",
            "--exact",
            "report::terminal_tests::cancelled_retry_fixture",
            "--nocapture",
        ])
        .env_remove("CLICOLOR_FORCE")
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    String::from_utf8(output.stderr).unwrap()
}

#[test]
fn cancelled_retries_report_the_final_state() {
    let text = cancelled_retry_output();
    let cancelled = text.find("Containers Cancelled").unwrap();
    assert!(text[cancelled..].contains("api Cancelled"), "{text}");
    assert!(!text[cancelled..].contains("Retrying"), "{text}");
}

#[test]
fn zero_change_summaries_do_not_claim_workers_are_already_running() {
    let text = cancelled_retry_output();
    assert!(text.contains("up: 0 of 2 changed"), "{text}");
    assert!(!text.contains("already in place"), "{text}");
}

#[tokio::test]
#[ignore = "subprocess fixture for cancellation and zero-change summaries"]
async fn cancelled_retry_fixture() {
    let mut progress = StartupProgress::start(true, Path::new("/tmp/startup-fixture"));
    progress.engine_ready();
    progress.downloads_starting();
    containers_starting();
    plan(&[("api".to_string(), 0)]);
    retry_waiting("api", 1, 3, Duration::from_secs(10));
    progress.finish(false, "Cancelled");
    summary_ok("up", 0, 2, Duration::from_millis(100));
}

#[tokio::test]
#[ignore = "subprocess fixture for independent persistent-panel regression checks"]
async fn coordinator_persistent_panel_regressions_fixture() {
    let dir = tempfile::tempdir().unwrap();
    let foreground = dir.path().join("foreground.yaml");
    let other = dir.path().join("other.yaml");
    std::fs::write(&foreground, "containers: {}\n").unwrap();
    std::fs::write(&other, "containers: {}\n").unwrap();
    let mut startup = StartupProgress::start(true, &foreground);
    startup.engine_ready();
    in_project_scope(&foreground, async {
        plan(&[("api".into(), 0)]);
        ready("api", Duration::ZERO);
    })
    .await;
    startup.finish(true, "Ready");

    let original_header = console()
        .lock()
        .unwrap()
        .startup
        .as_ref()
        .unwrap()
        .containers
        .state
        .clone();
    in_project_scope(&other, async {
        containers_starting();
    })
    .await;
    let foreign_changed_header = console()
        .lock()
        .unwrap()
        .startup
        .as_ref()
        .unwrap()
        .containers
        .state
        != original_header;

    in_project_scope(&foreground, async {
        starting("api", "stopping");
        stopped("api");
    })
    .await;
    let stopped_still_spins = matches!(
        console().lock().unwrap().rows[0].state,
        RowState::Starting { .. } | RowState::Retrying { .. }
    );
    assert!(
        !foreign_changed_header && !stopped_still_spins,
        "persistent panel regressions: foreign_project_changed_foreground_header={foreign_changed_header}; stopped_worker_still_spins={stopped_still_spins}"
    );
}

#[test]
fn coordinator_persistent_panel_regressions() {
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--ignored",
            "--exact",
            "report::terminal_tests::coordinator_persistent_panel_regressions_fixture",
            "--nocapture",
        ])
        .env_remove("CLICOLOR_FORCE")
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[tokio::test]
#[ignore = "subprocess fixture for update without package workers"]
async fn coordinator_empty_update_fixture() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("worker-compose.yaml");
    std::fs::write(&path, "containers:\n  api:\n    worker: path://.\n").unwrap();
    let mut startup = StartupProgress::start(true, &path);
    startup.engine_ready();
    startup.finish(true, "Ready");
    let daemon = crate::daemon::Daemon::start(
        "ws://127.0.0.1:1/ws".into(),
        "empty-update-fixture".into(),
        None,
        crate::daemon::EnginePolicy::External,
    );
    in_project_scope(&path, async {
        plan(&[("api".into(), 0)]);
        ready("api", Duration::ZERO);
    })
    .await;
    let rows_before = console().lock().unwrap().rows.clone();
    let operation = daemon
        .operations
        .create_with_id("empty-update".into(), 0)
        .await
        .unwrap();
    let result = daemon
        .update(Some(&path), &[], "empty-update".into())
        .await
        .unwrap();
    assert_eq!(serde_json::to_value(result).unwrap()["changed"], false);
    assert!(
        console().lock().unwrap().rows == rows_before,
        "no-op changed lifecycle rows"
    );
    let event = operation.snapshot().await.last_event.unwrap();
    assert_eq!(event.operation_id, "empty-update");
    assert_eq!(event.container, None);
    assert_eq!(event.phase, "unchanged");
    assert_eq!(event.detail, "Everything already up to date.");
    daemon.shutdown().await;
}

#[test]
fn coordinator_empty_update_reports_no_changes() {
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--ignored",
            "--exact",
            "report::terminal_tests::coordinator_empty_update_fixture",
            "--nocapture",
        ])
        .env_remove("CLICOLOR_FORCE")
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(output.status.success(), "{stderr}");
    assert!(
        stderr.contains("Everything already up to date."),
        "missing no-op feedback: {stderr}"
    );
}

#[tokio::test]
#[ignore = "isolated subprocess for bare daemon lifecycle ownership"]
async fn bare_daemon_panel_fixture() {
    let root = tempfile::tempdir().unwrap();
    // SAFETY: isolated fixture, before daemon tasks.
    unsafe {
        std::env::set_var("III_COMPOSE_STATE_DIR", root.path());
    }
    let path = root.path().join("worker-compose.yaml");
    std::fs::write(
        &path,
        "containers:\n  api:\n    worker: path://.\n    scripts:\n      run: exit 1\n",
    )
    .unwrap();
    let daemon = crate::daemon::Daemon::start(
        "ws://127.0.0.1:1/ws".into(),
        "bare-fixture".into(),
        None,
        crate::daemon::EnginePolicy::External,
    );
    let up = daemon.up(Some(&path), None, "bare-up".into());
    tokio::pin!(up);
    tokio::select! {
        result = &mut up => panic!("up ended before observation: {result:?}"),
        _ = tokio::time::sleep(Duration::from_millis(150)) => {}
    }
    {
        let mut state = console().lock().unwrap();
        assert!(state.startup.is_none());
        assert!(
            state
                .rows
                .iter()
                .any(|row| row.key == "api" && matches!(row.state, RowState::Starting { .. })),
            "bare up has no active panel"
        );
        let before = state.rows.clone();
        state.static_output = false;
        state.size = None;
        state.drawn = 0;
        let frame = state.render(Some((40, 120)));
        let mut terminal = vt100::Parser::new(40, 120, 0);
        write_terminal(&mut terminal, &frame);
        assert!(terminal.screen().contents().contains("api"));
        state.rows = before;
    }
    let other = root.path().join("other.yaml");
    std::fs::write(
        &other,
        "containers:\n  api:\n    worker: path://.\n    scripts:\n      run: exit 1\n",
    )
    .unwrap();
    let before = console().lock().unwrap().rows.clone();
    in_project_scope(&other, async {
        plan(&[("api".into(), 0)]);
        ready("api", Duration::ZERO);
        plan_done();
    })
    .await;
    assert!(
        console().lock().unwrap().rows == before,
        "foreign operation replaced bare active panel"
    );
    let outcome = tokio::time::timeout(Duration::from_secs(15), &mut up)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(outcome.status, crate::lifecycle::OpStatus::Ok);
    assert!(
        console().lock().unwrap().rows.is_empty(),
        "completed ephemeral plan retained rows"
    );
    let restart = daemon.restart(Some(&path), Some("api"), "bare-restart".into());
    tokio::pin!(restart);
    tokio::select! {
        result = &mut restart => panic!("restart ended before observation: {result:?}"),
        _ = tokio::time::sleep(Duration::from_millis(150)) => {}
    }
    assert!(
        console()
            .lock()
            .unwrap()
            .rows
            .iter()
            .any(|row| row.key == "api")
    );
    tokio::time::timeout(Duration::from_secs(15), &mut restart)
        .await
        .unwrap()
        .unwrap();
    assert!(console().lock().unwrap().rows.is_empty());
    let declarations = [crate::edit::WorkerInput::Definition(
        serde_json::json!({"worker": "path://./extra", "scripts": {"run": "/bin/false"}})
            .as_object()
            .unwrap()
            .clone(),
    )];
    std::fs::create_dir(root.path().join("extra")).unwrap();
    let add = daemon.add_configured(Some(&path), &declarations, "bare-add".into());
    tokio::pin!(add);
    tokio::select! {
        result = &mut add => panic!("add ended before observation: {result:?}"),
        _ = tokio::time::sleep(Duration::from_millis(150)) => {}
    }
    assert!(
        console()
            .lock()
            .unwrap()
            .rows
            .iter()
            .any(|row| row.key == "extra")
    );
    tokio::time::timeout(Duration::from_secs(30), &mut add)
        .await
        .unwrap()
        .unwrap();
    assert!(console().lock().unwrap().rows.is_empty());
    // Path-only update uses the real no-op entrypoint and must not invent a panel.
    let result = daemon
        .update(Some(&path), &[], "bare-update".into())
        .await
        .unwrap();
    assert_eq!(serde_json::to_value(result).unwrap()["changed"], false);
    assert!(console().lock().unwrap().rows.is_empty());
    daemon.shutdown().await;
}

#[test]
fn bare_daemon_up_owns_an_active_panel() {
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--ignored",
            "--exact",
            "report::terminal_tests::bare_daemon_panel_fixture",
            "--nocapture",
        ])
        .env("NO_COLOR", "1")
        .env_remove("CLICOLOR_FORCE")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[tokio::test]
#[ignore = "isolated subprocess for foreign lifecycle output"]
async fn foreign_lifecycle_output_fixture() {
    let root = tempfile::tempdir().unwrap();
    let a = root.path().join("a.yaml");
    let b = root.path().join("b.yaml");
    std::fs::write(&a, "containers: {}\n").unwrap();
    std::fs::write(&b, "containers: {}\n").unwrap();
    let mut startup = StartupProgress::start(false, &a);
    in_project_scope(&a, async {
        plan(&[("api".into(), 0)]);
        ready("api", Duration::ZERO);
    })
    .await;
    startup.finish(true, "Ready");
    let before = console().lock().unwrap().rows.clone();
    in_project_scope(&b, async {
        plan_full(&[("api".into(), 0)]);
        starting("api", "foreign-start");
        completed("api", "foreign-complete", Duration::ZERO);
        ready("api", Duration::ZERO);
        failed("api", "FOREIGN_ERROR", "foreign-failure");
        unchanged("api", "foreign-unchanged");
        stopped("api");
        rolled_back("api");
        restarting("api");
        retry_waiting("api", 1, 2, Duration::from_secs(1));
        retry_starting("api", 1, 2);
        retry_recovered("api", 1, 2, Duration::ZERO);
        not_required_failed(&["api".into()]);
        summary_ok("foreign-up", 1, 1, Duration::ZERO);
        summary_failed("foreign-up", "FOREIGN_ERROR", Duration::ZERO);
        plan_done();
    })
    .await;
    assert!(console().lock().unwrap().rows == before);
    in_project_scope(&a, async {
        restarting("api");
        summary_ok("foreground-up", 0, 1, Duration::ZERO);
    })
    .await;
}

#[test]
fn foreign_lifecycle_lines_identify_project_without_relabeling_foreground() {
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--ignored",
            "--exact",
            "report::terminal_tests::foreign_lifecycle_output_fixture",
            "--nocapture",
        ])
        .env("NO_COLOR", "1")
        .env_remove("CLICOLOR_FORCE")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    for token in [
        "foreign-start",
        "foreign-complete",
        "ready",
        "FOREIGN_ERROR",
        "foreign-unchanged",
        "stopped",
        "rolled back",
        "restarting",
        "Retry",
        "foreign-up",
        "is not required",
    ] {
        let lines: Vec<_> = stderr
            .lines()
            .filter(|line| line.contains(token) && !line.contains("Pending"))
            .collect();
        assert!(!lines.is_empty(), "missing {token}: {stderr}");
        for line in lines {
            // The foreground ready transition precedes the foreign callbacks.
            if token == "ready" && !line.contains("b.yaml") {
                continue;
            }
            if token == "restarting" && !line.contains("b.yaml") {
                continue;
            }
            assert!(
                line.contains(&format!("{}b.yaml: ", std::path::MAIN_SEPARATOR)),
                "unqualified {token}: {line}"
            );
        }
    }
    assert!(
        stderr
            .lines()
            .any(|line| line.contains("foreground-up") && !line.contains("a.yaml"))
    );
}

#[tokio::test]
#[ignore = "isolated subprocess for retry-only ownership"]
async fn retry_only_ownership_fixture() {
    let root = tempfile::tempdir().unwrap();
    let a = root.path().join("a.yaml");
    let b = root.path().join("b.yaml");
    std::fs::write(&a, "containers: {}\n").unwrap();
    std::fs::write(&b, "containers: {}\n").unwrap();
    in_project_scope(&b, async {
        retry_waiting("api", 2, 3, Duration::from_secs(6));
    })
    .await;
    in_project_scope(&a, async {
        plan(&[("web".into(), 0)]);
    })
    .await;
    assert_eq!(
        console().lock().unwrap().panel_project,
        Some(canonical_project(&a)),
        "retry-only feedback blocked new plan"
    );
    in_project_scope(&a, async {
        starting("web", "starting");
    })
    .await;
    let before = console().lock().unwrap().rows.clone();
    in_project_scope(&b, async {
        plan(&[("api".into(), 0)]);
        plan_done();
    })
    .await;
    assert!(
        console().lock().unwrap().rows == before,
        "active planned owner was stolen"
    );
    in_project_scope(&a, async {
        plan_done();
    })
    .await;
    in_project_scope(&b, async {
        retry_waiting("api", 2, 3, Duration::from_secs(6));
        unchanged("api", "not running");
    })
    .await;
    in_project_scope(&a, async {
        plan_full(&[("web".into(), 0)]);
    })
    .await;
    assert_eq!(
        console().lock().unwrap().panel_project,
        Some(canonical_project(&a)),
        "settled retry retained exclusive ownership"
    );
    in_project_scope(&a, async {
        plan_done();
    })
    .await;
    in_project_scope(&b, async {
        retry_waiting("api", 2, 3, Duration::from_secs(6));
        retries_cancelled(&["api".into()]);
    })
    .await;
    {
        let state = console().lock().unwrap();
        assert!(
            state.rows.is_empty(),
            "cancelled retry persists without a next operation"
        );
        assert!(state.panel_project.is_none());
    }
}

#[test]
fn retry_only_feedback_yields_to_a_new_plan() {
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--ignored",
            "--exact",
            "report::terminal_tests::retry_only_ownership_fixture",
            "--nocapture",
        ])
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

// This fixture launches POSIX shell commands to drive the real supervisor.
#[cfg(unix)]
#[tokio::test]
#[ignore = "isolated subprocess for real supervisor cancellation"]
async fn supervisor_down_releases_retry_fixture() {
    use crate::daemon::{Daemon, EnginePolicy};
    use futures::{SinkExt, StreamExt};
    let state = tempfile::tempdir().unwrap();
    // SAFETY: setup before any daemon task exists.
    unsafe { std::env::set_var("III_COMPOSE_STATE_DIR", state.path()) };
    let marker = state.path().join("b-ready");
    let marker_engine = marker.clone();

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    tokio::spawn(async move {
        loop {
            let Ok((stream, _)) = listener.accept().await else {
                return;
            };
            let marker = marker_engine.clone();
            tokio::spawn(async move {
                let Ok(mut socket) = tokio_tungstenite::accept_async(stream).await else {
                    return;
                };
                while let Some(Ok(frame)) = socket.next().await {
                    let Ok(text) = frame.to_text() else { continue };
                    let Ok(value) = serde_json::from_str::<serde_json::Value>(text) else {
                        continue;
                    };
                    if value["type"] == "invokefunction" && value["invocation_id"].is_string() {
                        let function = value["function_id"]
                            .as_str()
                            .unwrap_or_default()
                            .to_string();
                        let result = match function.as_str() {
                            "engine::workers::list" => {
                                if marker.exists() {
                                    serde_json::json!({"workers": [{"name": "api", "namespace": "default"}]})
                                } else {
                                    serde_json::json!({"workers": []})
                                }
                            }
                            "engine::functions::list" => serde_json::json!({"functions": []}),
                            "configuration::migration-capabilities" => {
                                serde_json::json!({"source_priority_archive_revision": 1})
                            }
                            "configuration::migrate" => serde_json::json!({"action": "missing"}),
                            _ => serde_json::json!({"value": {}}),
                        };
                        let reply = serde_json::json!({"type": "invocationresult", "invocation_id": value["invocation_id"], "function_id": function, "result": result});
                        if socket
                            .send(tokio_tungstenite::tungstenite::Message::Text(
                                reply.to_string().into(),
                            ))
                            .await
                            .is_err()
                        {
                            return;
                        }
                    }
                }
            });
        }
    });

    let root = tempfile::tempdir().unwrap();
    let b_dir = root.path().join("b");
    let a_dir = root.path().join("a");
    std::fs::create_dir_all(b_dir.join("api")).unwrap();
    std::fs::create_dir_all(a_dir.join("web")).unwrap();
    let b = b_dir.join("worker-compose.yaml");
    let a = a_dir.join("worker-compose.yaml");
    std::fs::write(
        &b,
        format!(
            "startup_timeout: 5s\nstop_timeout: 100ms\ncontainers:\n  api:\n    worker: path://./api\n    scripts:\n      run: \"touch '{}'; sleep 2; exit 1\"\n    restart:\n      condition: on-failure\n      delay: 6s\n      max_delay: 6s\n      max_attempts: 3\n      window: 2m\n",
            marker.display()
        ),
    )
    .unwrap();
    std::fs::write(
        &a,
        "startup_timeout: 2s\nstop_timeout: 100ms\ncontainers:\n  web:\n    worker: path://./web\n    scripts:\n      run: \"sleep 30\"\n",
    )
    .unwrap();

    let daemon = Daemon::start(
        format!("ws://{address}"),
        format!("repro-stuck-{}", std::process::id()),
        None,
        EnginePolicy::External,
    );
    tokio::time::sleep(Duration::from_millis(300)).await;

    {
        eprintln!("=== B up ===");
        let up = daemon.up(Some(&b), None, "b-up".into()).await;
        eprintln!(
            "=== B up ok={} ===",
            matches!(up, Ok(ref r) if r.containers.iter().all(|c| c.error.is_none()))
        );
        // Child exits at ~2s; supervisor schedules a retry with a 6s backoff.
        tokio::time::timeout(Duration::from_secs(30), async {
            loop {
                if console().lock().unwrap().rows.iter().any(|row| {
                    matches!(
                        row.state,
                        RowState::Retrying {
                            phase: RetryPhase::Waiting(_),
                            ..
                        }
                    )
                }) {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(25)).await;
            }
        })
        .await
        .unwrap();
        eprintln!("=== B down during backoff ===");
        let _ = daemon.down(Some(&b), None, "b-down".into()).await;
        assert!(
            console()
                .lock()
                .unwrap()
                .rows
                .iter()
                .all(|row| !matches!(row.state, RowState::Retrying { .. })),
            "down left active retry feedback"
        );
        tokio::time::sleep(Duration::from_millis(7000)).await;
        assert!(
            console()
                .lock()
                .unwrap()
                .rows
                .iter()
                .all(|row| !matches!(row.state, RowState::Retrying { .. })),
            "late supervisor announcement revived cancelled retry"
        );
    }
    eprintln!("=== A up ===");
    let up = daemon.up(Some(&a), None, "a-up".into());
    tokio::pin!(up);
    tokio::select! {
        result = &mut up => panic!("A finished before panel observation: {result:?}"),
        _ = tokio::time::sleep(Duration::from_millis(150)) => {}
    }
    {
        let state = console().lock().unwrap();
        assert_eq!(state.panel_project, Some(canonical_project(&a)));
        assert!(state.rows.iter().any(|row| row.key == "web"));
        assert!(state.rows.iter().all(|row| row.key != "api"));
    }
    tokio::time::timeout(Duration::from_secs(20), &mut up)
        .await
        .unwrap()
        .unwrap();
    daemon.shutdown().await;
}

#[cfg(unix)]
#[test]
fn supervisor_down_clears_retry_and_releases_bare_panel() {
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--ignored",
            "--exact",
            "report::terminal_tests::supervisor_down_releases_retry_fixture",
            "--nocapture",
        ])
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
