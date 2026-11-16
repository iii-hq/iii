// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

//! Binary packages in a VM (spec 4.2, 6.7, 7.5).
//!
//! A `binary` package runs its host build on the host, as before. It runs in a VM when its
//! container says so, as every VM container does: `scripts.run: iii sandbox run`, with the image
//! in `runtime.base_image`, since a binary package names none. Which of the two it is decides
//! which build the lock resolves and installs, so it has to be known before the lock is, from
//! the compose declaration alone: [`declares_vm`] is that decision, and the rest of this module
//! derives from it.

use std::path::Path;

use iii_sandbox_contract as contract;

use crate::{
    config::{Container, WorkerSource},
    error::{ComposeError, Result},
    registry::Platform,
};

/// Whether `container` runs in a VM, from its declaration alone: its `scripts.run` is `iii
/// sandbox run`, or it has none and sets `runtime.base_image` (the explicit VM mode is still to
/// be written into it, as for a `path://` whose manifest sets one). Known before the lock, so it
/// holds for a package whose kind is not known yet.
pub fn declares_vm(container: &Container) -> bool {
    match container.scripts.run.as_deref() {
        Some(run) => crate::sandbox::is_sandbox_run(run),
        None => container.base_image.is_some(),
    }
}

/// The platform whose build `container`'s package installs: [`Platform::Vm`] when it
/// [`declares_vm`], else [`Platform::Host`]. Only a binary package has a build per platform; a
/// bundle's one archive is the same on both.
pub fn platform(container: &Container) -> Platform {
    if declares_vm(container) {
        Platform::Vm
    } else {
        Platform::Host
    }
}

/// Whether `container` is a binary package. Only a lock-attached package is known to be one.
pub fn is_binary(container: &Container) -> bool {
    matches!(container.worker, WorkerSource::Package { .. })
        && container
            .resolved_package
            .as_ref()
            .is_some_and(|package| package.kind == "binary")
}

/// Whether `container` is a binary package that runs in a VM. Only a lock-attached package is
/// known to be one.
pub fn is_vm_binary(container: &Container) -> bool {
    is_binary(container) && declares_vm(container)
}

/// `WorkerBuildRequest.base_image` for `container`: its `runtime.base_image`, over the
/// manifest's. `None` leaves the adapter to the manifest's; contract 1.4 requires one with a
/// binary source.
pub fn base_image(container: &Container) -> Option<&str> {
    container.base_image.as_deref()
}

/// The rule of spec 4.2 for container `key` when it is a binary package, which
/// `sandbox::validate_scripts` applies in place of `RUN_NOT_ALLOWED_FOR_PACKAGE`: its scripts run
/// on the host like any other's, and a VM needs `runtime.base_image`
/// (`SANDBOX_BINARY_WITHOUT_BASE_IMAGE`). The checks every VM container gets (`scripts.build`,
/// the platform) stay with `validate_scripts`. `container` must be lock-attached.
pub fn check(key: &str, container: &Container) -> Result<()> {
    if is_vm_binary(container) && container.base_image.is_none() {
        let package = container
            .resolved_package
            .as_ref()
            .map(|package| {
                package
                    .alias_of
                    .clone()
                    .unwrap_or_else(|| package.name.clone())
            })
            .unwrap_or_default();
        return Err(ComposeError::SandboxBinaryWithoutBaseImage {
            container: key.to_string(),
            package,
        });
    }
    Ok(())
}

/// The `WorkerSource::Binary` that `iii sandbox build` and `iii sandbox run` hand the adapter for
/// binary package `package` of container `key`, copied into `package_dir`
/// (`.iii/workers/<container>/`): the directory and the [`executable`] in it.
/// `PACKAGE_ARTIFACT_EMPTY` when it holds none.
pub fn worker_source(
    key: &str,
    package: &str,
    package_dir: &Path,
) -> Result<contract::WorkerSource> {
    let executable = executable(package_dir).ok_or_else(|| ComposeError::PackageArtifactEmpty {
        container: key.to_string(),
        name: package.to_string(),
        path: package_dir.to_path_buf(),
    })?;
    Ok(contract::WorkerSource::Binary {
        package: package.to_string(),
        host_path: package_dir.to_string_lossy().into_owned(),
        executable,
    })
}

/// The executable of the binary package in `package_dir`, relative to it with `/` between
/// components: the first regular file with an execute bit, at the root before any directory, in
/// name order, at most three directories down. As on the host, so the same archive yields the
/// same file on every machine; unlike the host, a symlink is passed over, since the adapter
/// refuses one (contract 1.4). `None` when there is none.
pub fn executable(package_dir: &Path) -> Option<String> {
    fn find(dir: &Path, depth: usize) -> Option<Vec<String>> {
        if depth > 3 {
            return None;
        }
        let mut entries: Vec<_> = std::fs::read_dir(dir)
            .ok()?
            .filter_map(|entry| entry.ok())
            .collect();
        entries.sort_by_key(|entry| entry.file_name());
        let name = |entry: &std::fs::DirEntry| entry.file_name().to_str().map(str::to_string);
        for entry in &entries {
            let Ok(metadata) = std::fs::symlink_metadata(entry.path()) else {
                continue;
            };
            if metadata.is_file()
                && is_executable(&metadata)
                && let Some(name) = name(entry)
            {
                return Some(vec![name]);
            }
        }
        for entry in &entries {
            let is_dir = std::fs::symlink_metadata(entry.path()).is_ok_and(|meta| meta.is_dir());
            if is_dir
                && let Some(name) = name(entry)
                && let Some(mut rest) = find(&entry.path(), depth + 1)
            {
                rest.insert(0, name);
                return Some(rest);
            }
        }
        None
    }

    find(package_dir, 0).map(|components| components.join("/"))
}

#[cfg(unix)]
fn is_executable(metadata: &std::fs::Metadata) -> bool {
    use std::os::unix::fs::PermissionsExt;
    metadata.permissions().mode() & 0o111 != 0
}

/// A VM runs on Unix only (`SANDBOX_UNSUPPORTED_PLATFORM`), where a Linux build has no `.exe`.
#[cfg(not(unix))]
fn is_executable(_: &std::fs::Metadata) -> bool {
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::ComposeFile;
    use crate::registry::{RegistryArtifact, ResolvedPackage};
    use std::collections::BTreeMap;

    fn file(text: &str) -> ComposeFile {
        ComposeFile::parse(text, "/srv/app/worker-compose.yaml").unwrap()
    }

    fn attached(mut container: Container, kind: &str) -> Container {
        container.resolved_package = Some(ResolvedPackage {
            name: "tool".to_string(),
            alias_of: None,
            registry: crate::registry::DEFAULT_REGISTRY.to_string(),
            version: "1.0.0".to_string(),
            kind: kind.to_string(),
            artifacts: BTreeMap::from([(
                "x86_64-unknown-linux-musl".to_string(),
                RegistryArtifact {
                    sha256: "a".repeat(64),
                    url: "https://example.com/tool.tar.gz".to_string(),
                },
            )]),
            default_config: None,
        });
        container
    }

    const PROJECT: &str = r#"
containers:
  host:
    worker: package://tool
    version: 1.0.0
  host-scripts:
    worker: package://tool
    version: 1.0.0
    scripts:
      run: ./tool --port 3000
  vm:
    worker: package://tool
    version: 1.0.0
    runtime:
      base_image: docker.io/library/alpine:3.20
    scripts:
      build: iii sandbox build
      run: iii sandbox run
  vm-to-write:
    worker: package://tool
    version: 1.0.0
    runtime:
      base_image: docker.io/library/alpine:3.20
  vm-without-image:
    worker: package://tool
    version: 1.0.0
    scripts:
      build: iii sandbox build
      run: iii sandbox run --stop-timeout 5s
"#;

    #[test]
    fn the_declaration_alone_places_a_container_in_a_vm() {
        let file = file(PROJECT);
        let placed = |key: &str| {
            let container = &file.containers[key];
            (declares_vm(container), platform(container))
        };
        assert_eq!(placed("host"), (false, Platform::Host));
        assert_eq!(placed("host-scripts"), (false, Platform::Host));
        assert_eq!(placed("vm"), (true, Platform::Vm));
        assert_eq!(placed("vm-to-write"), (true, Platform::Vm));
        assert_eq!(placed("vm-without-image"), (true, Platform::Vm));
        // Before the lock, no package is known to be a binary.
        assert!(!is_vm_binary(&file.containers["vm"]));
    }

    #[test]
    fn a_binary_in_a_vm_needs_a_base_image_and_host_scripts_are_allowed() {
        let file = file(PROJECT);
        let checked = |key: &str, kind: &str| {
            check(key, &attached(file.containers[key].clone(), kind)).map_err(|error| error.code())
        };
        assert_eq!(checked("host", "binary"), Ok(()));
        assert_eq!(checked("host-scripts", "binary"), Ok(()));
        assert_eq!(checked("vm", "binary"), Ok(()));
        assert_eq!(checked("vm-to-write", "binary"), Ok(()));
        assert_eq!(
            checked("vm-without-image", "binary"),
            Err("SANDBOX_BINARY_WITHOUT_BASE_IMAGE")
        );
        // A bundle names its image in its manifest.
        assert_eq!(checked("vm-without-image", "bundle"), Ok(()));
        let vm = attached(file.containers["vm"].clone(), "binary");
        assert!(is_vm_binary(&vm));
        assert_eq!(base_image(&vm), Some("docker.io/library/alpine:3.20"));
        assert_eq!(base_image(&file.containers["host"]), None);
    }

    #[cfg(unix)]
    fn write(path: &Path, mode: u32) {
        use std::os::unix::fs::PermissionsExt;
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, b"\x7fELF").unwrap();
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode)).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn the_executable_is_the_first_regular_one_root_first_and_never_a_symlink() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        assert_eq!(executable(root), None);

        write(&root.join("README.md"), 0o644);
        write(&root.join("a-dir/nested-tool"), 0o755);
        assert_eq!(executable(root).as_deref(), Some("a-dir/nested-tool"));

        // A file at the root wins over any directory, and a symlink is passed over.
        std::os::unix::fs::symlink(root.join("a-dir/nested-tool"), root.join("a-link")).unwrap();
        assert_eq!(executable(root).as_deref(), Some("a-dir/nested-tool"));
        write(&root.join("z-tool"), 0o755);
        assert_eq!(executable(root).as_deref(), Some("z-tool"));

        // Four directories down is too deep.
        let deep = tempfile::tempdir().unwrap();
        write(&deep.path().join("a/b/c/d/tool"), 0o755);
        assert_eq!(executable(deep.path()), None);
        write(&deep.path().join("a/b/c/tool"), 0o755);
        assert_eq!(executable(deep.path()).as_deref(), Some("a/b/c/tool"));
    }

    #[cfg(unix)]
    #[test]
    fn the_worker_source_names_the_package_directory_and_its_executable() {
        let dir = tempfile::tempdir().unwrap();
        let package_dir = dir.path().join(".iii/workers/tool");
        std::fs::create_dir_all(&package_dir).unwrap();
        assert_eq!(
            worker_source("tool", "tool", &package_dir)
                .unwrap_err()
                .code(),
            "PACKAGE_ARTIFACT_EMPTY"
        );
        write(&package_dir.join("bin/tool"), 0o755);
        let source = worker_source("tool", "tool", &package_dir).unwrap();
        assert_eq!(
            serde_json::to_value(source).unwrap(),
            serde_json::json!({
                "kind": "binary",
                "package": "tool",
                "host_path": package_dir.to_string_lossy(),
                "executable": "bin/tool"
            })
        );
    }
}
