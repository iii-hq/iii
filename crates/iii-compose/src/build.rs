// Copyright 2025 Motia LLC. All rights reserved.
// SPDX-License-Identifier: Apache-2.0

//! Prepare every registry package in a compose file without starting anything.

use std::{future::Future, path::Path, time::Instant};

use futures::StreamExt;

use crate::{
    ComposeFile,
    config::WorkerSource,
    error::Result,
    registry::{self, InstallStatus, InstalledPackage},
    report,
    state::StateStore,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuildReport {
    pub packages: usize,
    pub downloaded: usize,
    pub cached: usize,
}

#[derive(Debug, Clone)]
struct PackageRequest {
    index: usize,
    container: String,
    reference: String,
    version: String,
}

/// Download every `package://` container declared by `file` into the cache
/// shared with `compose::up`.
pub async fn build(file: &Path) -> Result<BuildReport> {
    let file = ComposeFile::load(file)?;
    let cache = StateStore::package_cache()?;
    build_file_with(&file, &cache, |request, cache| async move {
        registry::install(
            &request.container,
            &request.reference,
            &request.version,
            &cache,
        )
        .await
    })
    .await
}

async fn build_file_with<F, Fut>(
    file: &ComposeFile,
    cache: &Path,
    installer: F,
) -> Result<BuildReport>
where
    F: Fn(PackageRequest, std::path::PathBuf) -> Fut + Sync,
    Fut: Future<Output = Result<InstalledPackage>>,
{
    let began = Instant::now();
    let requests: Vec<_> = file
        .containers
        .iter()
        .enumerate()
        .filter_map(|(index, (container, spec))| match &spec.worker {
            WorkerSource::Package { reference } => Some(PackageRequest {
                index,
                container: container.clone(),
                reference: reference.clone(),
                version: spec.version.as_deref().unwrap_or("*").to_string(),
            }),
            WorkerSource::Path { .. } => None,
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
            (index, container, began.elapsed(), result)
        }
    }))
    .buffer_unordered(crate::parallelism::max_parallel_workers());

    let mut downloaded = 0;
    let mut cached = 0;
    let mut failures = Vec::new();
    while let Some((index, container, elapsed, result)) = work.next().await {
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
    use crate::registry::Payload;

    fn package(status: InstallStatus) -> InstalledPackage {
        InstalledPackage {
            name: "worker".to_string(),
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
}
