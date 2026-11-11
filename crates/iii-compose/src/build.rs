// Copyright 2025 Motia LLC. All rights reserved.
// SPDX-License-Identifier: Apache-2.0

//! Prepare every registry package in a compose file without starting anything: lock it,
//! download it into the shared cache, and copy it into the project's `.iii/workers/` (spec 7.5).

use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
    time::Instant,
};

#[cfg(test)]
use futures::StreamExt;

use crate::{
    ComposeFile,
    error::{ComposeError, Result},
    registry::{self, InstallStatus},
    report,
    state::StateStore,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuildReport {
    pub packages: usize,
    pub downloaded: usize,
    pub cached: usize,
}

#[cfg(test)]
#[derive(Debug, Clone)]
struct PackageRequest {
    index: usize,
    container: String,
    reference: String,
    version: String,
}

/// Lock and download every `package://` container declared by `file` into the
/// cache shared with `compose::up`, then copy each into `.iii/workers/<container>/`
/// beside the file. Not frozen, it first migrates the containers 0.26 ran in a
/// VM (spec 7.3).
pub async fn build(file: &Path) -> Result<BuildReport> {
    build_with_mode(file, false).await
}

/// Downloads only the exact artifacts recorded in a matching existing lock.
pub async fn build_frozen(file: &Path) -> Result<BuildReport> {
    build_with_mode(file, true).await
}

async fn build_with_mode(file: &Path, frozen: bool) -> Result<BuildReport> {
    let began = Instant::now();
    let text = std::fs::read_to_string(file).map_err(|source| ComposeError::Io {
        path: file.to_path_buf(),
        source,
    })?;
    let mut file = ComposeFile::load(file)?;
    // The lock as this build found it, before anything read it. Beside the
    // canonical file, as `prepare` reads it: a symlinked compose file has its
    // lock beside its target.
    let found_lock = read_if_present(&crate::lockfile::lock_path(&file.path))?;
    let cache = StateStore::package_cache()?;
    let packages = file
        .containers
        .iter()
        .filter(|(_, container)| {
            matches!(
                container.worker,
                crate::config::WorkerSource::Package { .. }
            )
        })
        .map(|(name, _)| (name.clone(), 0))
        .collect::<Vec<_>>();
    report::plan(&packages);

    // Not frozen, it writes the adapter a VM needs, so `build` then
    // `up --frozen` works.
    let mut edited = text.clone();
    // `build` takes no `--namespace`: the file's own.
    let namespace = crate::namespace::project_namespace(None, file.namespace.as_deref());
    let prepared = async {
        if frozen {
            let prepared = crate::lockfile::prepare_frozen(&mut file, &cache).await?;
            crate::sandbox::require_declared_adapter(&file, &namespace)?;
            return Ok::<_, ComposeError>(prepared);
        }
        let mut prepared = crate::lockfile::prepare_metadata(&mut file, &BTreeSet::new()).await?;
        let path = file.path.clone();
        // Spec 7.2 and 7.3 first, so the adapter check sees the rewritten
        // containers and re-resolves, or removes, the adapter `migrate` added.
        crate::sandbox::migrate(&path, &mut edited, &mut file, &namespace, &mut prepared).await?;
        crate::sandbox::apply_adapter_change(
            &path,
            &mut edited,
            &mut file,
            &namespace,
            &mut prepared,
            &mut BTreeSet::new(),
            &mut BTreeMap::new(),
            false,
            false,
        )
        .await?;
        // Resolved first, the adapter included: everything downloads in one batch.
        prepared.install(&cache).await?;
        Ok(prepared)
    }
    .await;
    let prepared = match prepared {
        Ok(prepared) => prepared,
        Err(error) => {
            report::plan_done();
            report::summary_failed("build", error.code(), began.elapsed());
            return Err(error);
        }
    };
    for (container, reference, canonical) in prepared.aliases() {
        registry::warn_alias(container, reference, Some(canonical), None).await;
    }
    // The daemon's mutation lock is in its own process: an edit made while
    // this prepared must not be overwritten from the snapshot. A build that
    // writes nothing (frozen, or already locked) overwrites nothing.
    if (edited != text || prepared.changed())
        && std::fs::read_to_string(&file.path).map_err(|source| ComposeError::Io {
            path: file.path.clone(),
            source,
        })? != text
    {
        let error = ComposeError::BuildPlanStale {
            path: file.path.clone(),
        };
        report::plan_done();
        report::summary_failed("build", error.code(), began.elapsed());
        return Err(error);
    }
    crate::daemon::persist_mutation(&file.path, &text, &edited, &prepared)?;
    if prepared.changed() {
        report::lock_changed(prepared.path(), !prepared.existed());
    }
    // `.iii/workers/` follows the lock just written, under `--frozen` too (spec 7.5).
    // The daemon materializes too, and may have changed the file or the lock
    // since this build read them: checked again under `.iii/workers/.lock`, every
    // time, so a stale build never undoes the daemon's newer `.iii/workers/`.
    // The lock must still be the one this build stands on: the one it found
    // when it wrote none, or, when it wrote one, a lock resolving every
    // container to the package it wrote. A build that wrote nothing compares
    // the lock alone, since `.iii/workers/` follows the lock: an edit of the file
    // made meanwhile is kept and fails nothing. One that wrote also compares
    // the file with what it wrote.
    let wrote = edited != text || prepared.changed();
    let unchanged = || {
        let file_holds = !wrote
            || std::fs::read_to_string(&file.path).map_err(|source| ComposeError::Io {
                path: file.path.clone(),
                source,
            })? == edited;
        let lock_holds = if prepared.changed() {
            locks_the_same_packages(prepared.path(), &prepared)?
        } else {
            read_if_present(prepared.path())? == found_lock
        };
        if !file_holds || !lock_holds {
            return Err(ComposeError::BuildPlanStale {
                path: file.path.clone(),
            });
        }
        Ok(())
    };
    let compose_dir = crate::build_record::compose_dir(&file.path);
    if let Err(error) =
        registry::materialize_project_checked(compose_dir, &cache, &prepared, unchanged).await
    {
        report::plan_done();
        report::summary_failed("build", error.code(), began.elapsed());
        return Err(error);
    }

    let mut downloaded = 0;
    let mut cached = 0;
    for (container, status) in prepared.install_statuses() {
        match status {
            InstallStatus::Downloaded => {
                downloaded += 1;
                report::completed(container, "downloaded", began.elapsed());
            }
            InstallStatus::Cached => {
                cached += 1;
                report::unchanged(container, "already cached");
            }
        }
    }
    report::plan_done();
    let packages = downloaded + cached;
    report::summary_ok("build", downloaded, packages, began.elapsed());
    Ok(BuildReport {
        packages,
        downloaded,
        cached,
    })
}

/// Whether the lock at `path` resolves the same containers to the same packages as
/// `prepared`: what this build wrote, in the part `.iii/workers/` is made from. Compared
/// parsed, so the check does not depend on re-reading the bytes it wrote; a lock that is
/// missing or does not parse holds nothing.
fn locks_the_same_packages(path: &Path, prepared: &crate::lockfile::PreparedLock) -> Result<bool> {
    let Some(bytes) = read_if_present(path)? else {
        return Ok(false);
    };
    let Ok(document) = serde_yaml::from_slice::<serde_yaml::Value>(&bytes) else {
        return Ok(false);
    };
    let Some(containers) = document
        .get("containers")
        .and_then(serde_yaml::Value::as_mapping)
    else {
        return Ok(false);
    };
    let mut on_disk = BTreeMap::new();
    for (key, entry) in containers {
        let (Some(key), Some(resolved)) = (key.as_str(), entry.get("resolved")) else {
            return Ok(false);
        };
        let Ok(resolved) = serde_yaml::from_value::<registry::ResolvedPackage>(resolved.clone())
        else {
            return Ok(false);
        };
        on_disk.insert(key, resolved);
    }
    let wrote: BTreeMap<_, _> = prepared
        .packages()
        .map(|(key, resolved)| (key, resolved.clone()))
        .collect();
    Ok(on_disk == wrote)
}

/// The bytes of the file at `path`; `None` when there is none.
fn read_if_present(path: &Path) -> Result<Option<Vec<u8>>> {
    match std::fs::read(path) {
        Ok(bytes) => Ok(Some(bytes)),
        Err(source) if source.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(source) => Err(ComposeError::Io {
            path: path.to_path_buf(),
            source,
        }),
    }
}

#[cfg(test)]
async fn build_file_with<F, Fut>(
    file: &ComposeFile,
    cache: &Path,
    installer: F,
) -> Result<BuildReport>
where
    F: Fn(PackageRequest, std::path::PathBuf) -> Fut + Sync,
    Fut: std::future::Future<Output = Result<crate::registry::InstalledPackage>>,
{
    let began = Instant::now();
    let requests: Vec<_> = file
        .containers
        .iter()
        .enumerate()
        .filter_map(|(index, (container, spec))| match &spec.worker {
            crate::config::WorkerSource::Package { reference } => Some(PackageRequest {
                index,
                container: container.clone(),
                reference: reference.clone(),
                version: spec.version.as_deref().unwrap_or("*").to_string(),
            }),
            crate::config::WorkerSource::Path { .. } => None,
        })
        .collect();

    report::plan(
        &requests
            .iter()
            .map(|request| (request.container.clone(), 0))
            .collect::<Vec<_>>(),
    );

    let installer = &installer;
    let mut work = futures::stream::iter(requests.into_iter().map(|request| {
        let cache = cache.to_path_buf();
        async move {
            let index = request.index;
            let container = request.container.clone();
            let reference = request.reference.clone();
            let version = request.version.clone();
            let began = Instant::now();
            report::starting(&container, &format!("preparing {reference}@{version}"));
            let result = installer(request, cache).await;
            (index, container, reference, began.elapsed(), result)
        }
    }))
    .buffer_unordered(crate::parallelism::max_parallel_workers());

    let mut downloaded = 0;
    let mut cached = 0;
    let mut failures = Vec::new();
    let mut warned = BTreeSet::new();
    while let Some((index, container, reference, elapsed, result)) = work.next().await {
        if let Ok(package) = &result
            && let Some(alias_of) = &package.alias_of
            && warned.insert((registry::split_reference(&reference), alias_of.clone()))
        {
            registry::warn_alias(&container, &reference, Some(alias_of), None).await;
        }
        match result {
            Ok(package) => match package.status {
                InstallStatus::Downloaded => {
                    downloaded += 1;
                    report::completed(&container, "downloaded", elapsed);
                }
                InstallStatus::Cached => {
                    cached += 1;
                    report::unchanged(&container, "already cached");
                }
            },
            Err(error) => {
                let message = error.to_string();
                let prefix = format!("container '{container}': ");
                let message = message.strip_prefix(&prefix).unwrap_or(&message);
                report::failed(&container, error.code(), message);
                failures.push((index, error));
            }
        }
    }
    report::plan_done();

    if !failures.is_empty() {
        failures.sort_by_key(|(index, _)| *index);
        let error = failures.remove(0).1;
        report::summary_failed("build", error.code(), began.elapsed());
        return Err(error);
    }

    let packages = downloaded + cached;
    report::summary_ok("build", downloaded, packages, began.elapsed());
    Ok(BuildReport {
        packages,
        downloaded,
        cached,
    })
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use super::*;
    use crate::error::ComposeError;
    use crate::registry::{InstalledPackage, Payload};

    fn package(status: InstallStatus) -> InstalledPackage {
        InstalledPackage {
            name: "worker".to_string(),
            alias_of: None,
            version: "1.0.0".to_string(),
            payload: Payload::Binary("/tmp/worker".into()),
            default_config: None,
            status,
        }
    }

    #[tokio::test]
    async fn build_installs_only_registry_packages() {
        let file = ComposeFile::parse(
            "containers:\n  local:\n    worker: path://./local\n    scripts: { run: ./start }\n  state:\n    worker: package://state\n    version: 1.0.0\n  queue:\n    worker: package://queue\n    version: 2.0.0\n",
            "/srv/app/worker-compose.yaml",
        )
        .unwrap();
        let installed = Arc::new(Mutex::new(Vec::new()));
        let seen = Arc::clone(&installed);

        let report = build_file_with(&file, Path::new("/cache"), move |request, _| {
            let seen = Arc::clone(&seen);
            async move {
                seen.lock().unwrap().push(request.container.clone());
                Ok(if request.container == "state" {
                    package(InstallStatus::Downloaded)
                } else {
                    package(InstallStatus::Cached)
                })
            }
        })
        .await
        .unwrap();

        let mut installed = installed.lock().unwrap().clone();
        installed.sort();
        assert_eq!(installed, ["queue", "state"]);
        assert_eq!(
            report,
            BuildReport {
                packages: 2,
                downloaded: 1,
                cached: 1,
            }
        );
    }

    #[tokio::test]
    async fn build_reports_the_first_declared_failure() {
        let file = ComposeFile::parse(
            "containers:\n  first:\n    worker: package://first\n    version: 1.0.0\n  second:\n    worker: package://second\n    version: 1.0.0\n",
            "/srv/app/worker-compose.yaml",
        )
        .unwrap();

        let error = build_file_with(&file, Path::new("/cache"), |request, _| async move {
            Err(ComposeError::PackageDownloadFailed {
                container: request.container.clone(),
                url: format!("https://example.test/{}", request.container),
                message: "failed".to_string(),
            })
        })
        .await
        .unwrap_err();

        assert!(error.to_string().contains("container 'first'"));
    }

    /// The executable of a binary package: `worker`, `worker.exe` on Windows, the only name
    /// `installed_binary` takes there.
    fn worker_file() -> String {
        format!("worker{}", std::env::consts::EXE_SUFFIX)
    }

    /// A tar.gz holding `files`: name, mode, body.
    fn archive(files: &[(&str, u32, &[u8])]) -> Vec<u8> {
        let encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        let mut archive = tar::Builder::new(encoder);
        for (name, mode, body) in files {
            let mut header = tar::Header::new_gnu();
            header.set_size(body.len() as u64);
            header.set_mode(*mode);
            header.set_cksum();
            archive.append_data(&mut header, name, *body).unwrap();
        }
        archive.into_inner().unwrap().finish().unwrap()
    }

    /// Answers `/resolve` for `worker` with `node` (the registry's own fields) and serves
    /// `archive` at `/<worker>`.
    async fn serve(
        server: &wiremock::MockServer,
        worker: &str,
        mut node: serde_json::Value,
        archive: Vec<u8>,
    ) {
        use sha2::Digest;
        use wiremock::{Mock, ResponseTemplate, matchers};

        let sha256 = hex::encode(sha2::Sha256::digest(&archive));
        let url = format!("{}/{worker}", server.uri());
        node["name"] = worker.into();
        node["version"] = "1.0.0".into();
        if node["type"] == "bundle" {
            node["archive_url"] = url.into();
            node["sha256"] = sha256.into();
        } else {
            node["binaries"] = serde_json::json!({
                (crate::registry::host_targets()[0]): { "sha256": sha256, "url": url }
            });
        }
        Mock::given(matchers::method("POST"))
            .and(matchers::path("/resolve"))
            .and(matchers::body_partial_json(
                serde_json::json!({ "worker": worker }),
            ))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(serde_json::json!({ "graph": [node] })),
            )
            .mount(server)
            .await;
        Mock::given(matchers::method("GET"))
            .and(matchers::path(format!("/{worker}")))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(archive))
            .mount(server)
            .await;
    }

    /// `iii compose build` of `file`, not frozen, against the registry at `registry`, with
    /// `cache` as the package cache.
    async fn build_against(file: &Path, registry: String, cache: &Path) -> Result<BuildReport> {
        crate::state::TEST_PACKAGE_CACHE
            .scope(
                cache.to_path_buf(),
                crate::registry::TEST_REGISTRY.scope(registry, build(file)),
            )
            .await
    }

    fn recorded_containers(project: &Path) -> Vec<String> {
        let packages: serde_json::Value = serde_json::from_slice(
            &std::fs::read(project.join(".iii/workers/.packages.json")).unwrap(),
        )
        .unwrap();
        packages["containers"]
            .as_object()
            .unwrap()
            .keys()
            .cloned()
            .collect()
    }

    #[tokio::test]
    async fn build_locks_and_copies_each_package_into_the_project() {
        let server = wiremock::MockServer::start().await;
        let state = archive(&[(&worker_file(), 0o755, b"#!/bin/sh\nexit 0\n")]);
        serve(
            &server,
            "state",
            serde_json::json!({ "type": "binary" }),
            state,
        )
        .await;
        let project = tempfile::tempdir().unwrap();
        std::fs::create_dir(project.path().join("local")).unwrap();
        let file = project.path().join("worker-compose.yaml");
        std::fs::write(
            &file,
            "containers:\n  local:\n    worker: path://./local\n    scripts: { run: ./start }\n  state:\n    worker: package://state\n    version: 1.0.0\n",
        )
        .unwrap();
        let cache = tempfile::tempdir().unwrap();

        let report = build_against(&file, server.uri(), cache.path())
            .await
            .unwrap();

        assert_eq!(report.packages, 1);
        assert!(project.path().join("worker-compose.lock").is_file());
        assert!(
            crate::build_record::package_dir(project.path(), "state")
                .join(worker_file())
                .is_file()
        );
        assert_eq!(recorded_containers(project.path()), ["state"]);
    }

    // Spec 11.2: `migrate` adds the adapter a rewritten bundle needs; the build installs it
    // and copies it beside the bundle.
    #[cfg(unix)]
    #[tokio::test]
    async fn build_migrates_a_vm_bundle_and_copies_it_with_the_adapter() {
        let adapter = archive(&[("sandbox-libkrun", 0o755, b"#!/bin/sh\nexit 0\n")]);
        let server = crate::sandbox::tests::adapter_registry(&adapter).await;
        let app = archive(&[(
            "iii.worker.yaml",
            0o644,
            b"name: app\nscripts:\n  start: node index.js\n",
        )]);
        serve(&server, "app", serde_json::json!({ "type": "bundle" }), app).await;
        let project = tempfile::tempdir().unwrap();
        let file = project.path().join("worker-compose.yaml");
        std::fs::write(
            &file,
            "containers:\n  app:\n    worker: package://app\n    version: 1.0.0\n",
        )
        .unwrap();
        let cache = tempfile::tempdir().unwrap();

        build_against(&file, server.uri(), cache.path())
            .await
            .unwrap();

        let text = std::fs::read_to_string(&file).unwrap();
        for written in [
            "sandbox-libkrun:",
            "build: iii sandbox build",
            "run: iii sandbox run",
        ] {
            assert!(text.contains(written), "{written} missing from:\n{text}");
        }
        assert_eq!(
            recorded_containers(project.path()),
            ["app", "sandbox-libkrun"]
        );
        assert!(
            project
                .path()
                .join(".iii/workers/app/iii.worker.yaml")
                .is_file()
        );
        assert!(
            project
                .path()
                .join(".iii/workers/sandbox-libkrun/sandbox-libkrun")
                .is_file()
        );
    }

    #[tokio::test]
    async fn the_late_check_compares_the_lock_parsed_with_what_the_build_wrote() {
        use sha2::Digest;
        use wiremock::{Mock, ResponseTemplate, matchers};

        let server = wiremock::MockServer::start().await;
        let state = archive(&[(&worker_file(), 0o755, b"#!/bin/sh\nexit 0\n")]);
        let resolved = serde_json::json!({
            "name": "state",
            "version": "1.0.0",
            "type": "binary",
            "artifacts": { (crate::registry::host_targets()[0]): {
                "sha256": hex::encode(sha2::Sha256::digest(&state)),
                "url": format!("{}/state", server.uri()),
            }}
        });
        Mock::given(matchers::method("GET"))
            .and(matchers::path("/state"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(state))
            .mount(&server)
            .await;
        let lock = |resolved: &serde_json::Value| {
            serde_json::json!({ "version": 1, "containers": { "state": {
                "worker": "package://state", "requested": "1.0.0", "resolved": resolved
            }}})
        };
        let project = tempfile::tempdir().unwrap();
        let file = project.path().join("worker-compose.yaml");
        let lock_path = project.path().join("worker-compose.lock");
        std::fs::write(
            &file,
            "containers:\n  state:\n    worker: package://state\n    version: 1.0.0\n",
        )
        .unwrap();
        std::fs::write(&lock_path, serde_yaml::to_string(&lock(&resolved)).unwrap()).unwrap();
        let cache = tempfile::tempdir().unwrap();
        let mut compose = ComposeFile::load(&file).unwrap();
        let prepared = crate::lockfile::prepare_frozen(&mut compose, cache.path())
            .await
            .unwrap();

        assert!(locks_the_same_packages(&lock_path, &prepared).unwrap());
        // The same lock in other bytes (JSON is YAML): compared parsed, it still holds.
        std::fs::write(&lock_path, lock(&resolved).to_string()).unwrap();
        assert!(locks_the_same_packages(&lock_path, &prepared).unwrap());

        // Another package for the container, as a concurrent writer would leave it.
        let mut other = resolved.clone();
        other["version"] = "1.1.0".into();
        std::fs::write(&lock_path, serde_yaml::to_string(&lock(&other)).unwrap()).unwrap();
        assert!(!locks_the_same_packages(&lock_path, &prepared).unwrap());
        std::fs::write(&lock_path, "containers: [").unwrap();
        assert!(!locks_the_same_packages(&lock_path, &prepared).unwrap());
        std::fs::remove_file(&lock_path).unwrap();
        assert!(!locks_the_same_packages(&lock_path, &prepared).unwrap());
    }
}
