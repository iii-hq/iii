// Copyright 2025 Motia LLC. All rights reserved.
// SPDX-License-Identifier: Apache-2.0

//! Prepare every registry package in a compose file without starting anything.

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
/// cache shared with `compose::up`.
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
        let mut prepared = crate::lockfile::prepare(&mut file, &cache, &BTreeSet::new()).await?;
        let path = file.path.clone();
        if crate::sandbox::apply_adapter_change(
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
        .await?
        .is_some()
        {
            prepared.install(&cache).await?;
        }
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
    // this prepared must not be overwritten from the snapshot.
    let current = std::fs::read_to_string(&file.path).map_err(|source| ComposeError::Io {
        path: file.path.clone(),
        source,
    })?;
    if current != text {
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
}
