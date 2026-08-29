// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0.

//! VM preparation from an already-compiled package descriptor.
//!
//! Compose and Registry installs pass the complete runtime contract here. This
//! module never discovers worker metadata from the source or bundle directory.

use colored::Colorize;
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
};

use super::rootfs::clone_rootfs;

#[cfg(unix)]
pub fn restore_terminal_cooked_mode() {
    let stderr = std::io::stderr();
    if let Ok(mut termios) = nix::sys::termios::tcgetattr(&stderr) {
        termios
            .output_flags
            .insert(nix::sys::termios::OutputFlags::OPOST | nix::sys::termios::OutputFlags::ONLCR);
        termios
            .local_flags
            .insert(nix::sys::termios::LocalFlags::ICANON | nix::sys::termios::LocalFlags::ECHO);
        termios
            .input_flags
            .insert(nix::sys::termios::InputFlags::ICRNL);
        let _ = nix::sys::termios::tcsetattr(&stderr, nix::sys::termios::SetArg::TCSANOW, &termios);
    }
}

#[cfg(not(unix))]
pub fn restore_terminal_cooked_mode() {}

pub const GUEST_CONFIG_DIR: &str = "/run/iii/config";

pub struct VmOverride<'a> {
    pub state_dir: PathBuf,
    pub engine_url: &'a str,
    pub extra_env: HashMap<String, String>,
    pub config_dir: Option<PathBuf>,
}

#[derive(Clone)]
pub struct DescriptorVmSpec {
    pub exec: Vec<String>,
    pub base_image: String,
    pub prepare: Vec<Vec<String>>,
    pub environment: HashMap<String, String>,
    pub cpus: u32,
    pub memory_mib: u32,
}

struct PreparedVm {
    exec_path: &'static str,
    args: Vec<String>,
    env: HashMap<String, String>,
    vcpus: u32,
    ram_mib: u32,
    rootfs: PathBuf,
    mounts: Vec<(String, String)>,
}

fn shell_escape(value: &str) -> String {
    value.replace('\'', "'\\''")
}

fn shell_join(argv: &[String]) -> String {
    argv.iter()
        .map(|value| format!("'{}'", shell_escape(value)))
        .collect::<Vec<_>>()
        .join(" ")
}

fn runtime_script(spec: &DescriptorVmSpec, prepared: bool) -> String {
    let mut script = vec![
        "set -e".to_string(),
        "export HOME=${HOME:-/root}".to_string(),
        "export PATH=/usr/local/bin:/usr/bin:/bin:$PATH".to_string(),
        "cd /workspace".to_string(),
    ];
    if !prepared {
        for command in &spec.prepare {
            script.push(shell_join(command));
        }
        script.push("mkdir -p /var && touch /var/.iii-prepared".to_string());
    }
    for (name, value) in &spec.environment {
        if name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
            && !name.is_empty()
        {
            script.push(format!("export {name}='{}'", shell_escape(value)));
        }
    }
    script.push(format!(
        "exec /bin/sh -c '{}'",
        shell_escape(&shell_join(&spec.exec))
    ));
    script.join("\n")
}

fn runtime_env(
    worker_name: &str,
    engine_url: &str,
    descriptor: &DescriptorVmSpec,
    extra: &HashMap<String, String>,
) -> HashMap<String, String> {
    let mut env = descriptor.environment.clone();
    env.extend(extra.clone());
    env.insert("III_ENGINE_URL".into(), engine_url.into());
    env.insert("III_URL".into(), engine_url.into());
    env.insert("III_WORKER_NAME".into(), worker_name.into());
    env
}

async fn prepare_descriptor_vm(
    worker_name: &str,
    worker_dir: &Path,
    descriptor: &DescriptorVmSpec,
    over: &VmOverride<'_>,
) -> Result<PreparedVm, String> {
    if !worker_dir.is_dir() {
        return Err(format!(
            "worker directory '{}' does not exist",
            worker_dir.display()
        ));
    }
    if descriptor.exec.is_empty() {
        return Err(format!("package {worker_name} has no runtime.exec"));
    }
    if descriptor.base_image.trim().is_empty() {
        return Err(format!("package {worker_name} has no runtime.base_image"));
    }

    if let Err(error) = super::firmware::download::ensure_libkrunfw().await {
        tracing::warn!(%error, "failed to ensure libkrunfw");
    }
    if !super::worker_manager::libkrun::libkrun_available() {
        return Err("no libkrun runtime is available".into());
    }

    let state_dir = over.state_dir.clone();
    let needs_rootfs = !state_dir.join("bin").exists();
    if needs_rootfs {
        if state_dir.exists() {
            std::fs::remove_dir_all(&state_dir)
                .map_err(|error| format!("cannot clear {}: {error}", state_dir.display()))?;
        }
        let base = super::worker_manager::oci::prepare_rootfs(
            "descriptor",
            Some(descriptor.base_image.as_str()),
        )
        .await
        .map_err(|error| error.to_string())?;
        clone_rootfs(&base, &state_dir)
            .map_err(|error| format!("cannot clone package rootfs: {error}"))?;
    }

    let base = super::worker_manager::oci::prepare_rootfs(
        "descriptor",
        Some(descriptor.base_image.as_str()),
    )
    .await
    .map_err(|error| error.to_string())?;
    let mut env = runtime_env(worker_name, over.engine_url, descriptor, &over.extra_env);
    for (name, value) in super::worker_manager::oci::read_oci_env(&base) {
        env.entry(name).or_insert(value);
    }

    let script_dir = state_dir.join("opt/iii");
    std::fs::create_dir_all(&script_dir)
        .map_err(|error| format!("cannot create runtime script directory: {error}"))?;
    let script_path = script_dir.join("descriptor-run.sh");
    let prepared = state_dir.join("var/.iii-prepared").exists();
    std::fs::write(&script_path, runtime_script(descriptor, prepared))
        .map_err(|error| format!("cannot write runtime script: {error}"))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&script_path, std::fs::Permissions::from_mode(0o755))
            .map_err(|error| format!("cannot make runtime script executable: {error}"))?;
    }

    let init = super::firmware::download::ensure_init_binary()
        .await
        .map_err(|error| format!("cannot provision iii-init: {error}"))?;
    if !iii_filesystem::init::has_init() {
        let destination = state_dir.join("init.krun");
        std::fs::copy(init, &destination)
            .map_err(|error| format!("cannot copy iii-init: {error}"))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&destination, std::fs::Permissions::from_mode(0o755))
                .map_err(|error| format!("cannot make iii-init executable: {error}"))?;
        }
    }

    let mut mounts = vec![(
        worker_dir.to_string_lossy().into_owned(),
        "/workspace".to_string(),
    )];
    if let Some(config_dir) = &over.config_dir {
        mounts.push((
            config_dir.to_string_lossy().into_owned(),
            GUEST_CONFIG_DIR.to_string(),
        ));
    }
    Ok(PreparedVm {
        exec_path: "/bin/sh",
        args: vec!["-c".into(), "exec bash /opt/iii/descriptor-run.sh".into()],
        env,
        vcpus: descriptor.cpus.max(1),
        ram_mib: descriptor.memory_mib,
        rootfs: state_dir,
        mounts,
    })
}

pub async fn descriptor_vm_command(
    worker_name: &str,
    worker_dir: &Path,
    _is_bundle: bool,
    descriptor: DescriptorVmSpec,
    over: VmOverride<'_>,
) -> Result<super::worker_manager::libkrun::VmCommand, String> {
    let prepared = prepare_descriptor_vm(worker_name, worker_dir, &descriptor, &over).await?;
    super::worker_manager::libkrun::build_vm_command(
        prepared.exec_path,
        &prepared.args,
        prepared.env,
        prepared.vcpus,
        prepared.ram_mib,
        prepared.rootfs,
        None,
        String::new(),
        None,
        &prepared.mounts,
    )
}

fn descriptor_vm_spec_from_install(
    install_dir: &Path,
    worker_name: &str,
) -> Result<DescriptorVmSpec, String> {
    let descriptor_path = install_dir.join(".iii-package-descriptor.json");
    let digest_path = install_dir.join(".iii-package-descriptor.sha256");
    let descriptor: iii_compose::descriptor::PackageDescriptor = serde_json::from_slice(
        &std::fs::read(&descriptor_path)
            .map_err(|error| format!("cannot read {}: {error}", descriptor_path.display()))?,
    )
    .map_err(|error| format!("invalid {}: {error}", descriptor_path.display()))?;
    let expected = std::fs::read_to_string(&digest_path)
        .map_err(|error| format!("cannot read {}: {error}", digest_path.display()))?;
    let actual = descriptor.sha256();
    if expected.trim() != actual {
        return Err(format!(
            "package descriptor digest mismatch for {worker_name}"
        ));
    }
    if descriptor.name != worker_name {
        return Err(format!(
            "package descriptor identity mismatch for {worker_name}"
        ));
    }
    if !matches!(
        descriptor.artifact,
        iii_compose::descriptor::Artifact::JavascriptBundle { .. }
            | iii_compose::descriptor::Artifact::PythonBundle { .. }
    ) {
        return Err(format!("package {worker_name} is not a bundle artifact"));
    }
    let runtime = descriptor.runtime;
    let exec = runtime
        .exec
        .ok_or_else(|| format!("package {worker_name} has no runtime.exec"))?;
    let base_image = runtime
        .base_image
        .ok_or_else(|| format!("package {worker_name} has no runtime.base_image"))?;
    Ok(DescriptorVmSpec {
        exec,
        base_image,
        prepare: runtime.prepare,
        environment: runtime.environment.into_iter().collect(),
        cpus: runtime
            .resources
            .as_ref()
            .and_then(|value| value.cpu)
            .map(|value| value.ceil() as u32)
            .unwrap_or(2)
            .max(1),
        memory_mib: runtime
            .resources
            .as_ref()
            .and_then(|value| value.memory_mib)
            .and_then(|value| u32::try_from(value).ok())
            .unwrap_or(2048),
    })
}

pub async fn start_bundle_worker(worker_name: &str, worker_path: &str, port: u16) -> i32 {
    let worker_dir = Path::new(worker_path);
    let descriptor = match descriptor_vm_spec_from_install(worker_dir, worker_name) {
        Ok(descriptor) => descriptor,
        Err(error) => {
            eprintln!("{} {error}", "error:".red());
            return 1;
        }
    };
    let Some(home) = dirs::home_dir() else {
        eprintln!("{} cannot determine home directory", "error:".red());
        return 1;
    };
    let state_dir = home.join(".iii/managed").join(worker_name);
    let engine_url = format!("ws://localhost:{port}");
    let extra_env = super::config_file::get_worker_config_as_env(worker_name);
    let over = VmOverride {
        state_dir: state_dir.clone(),
        engine_url: &engine_url,
        extra_env,
        config_dir: None,
    };
    let prepared = match prepare_descriptor_vm(worker_name, worker_dir, &descriptor, &over).await {
        Ok(prepared) => prepared,
        Err(error) => {
            eprintln!("{} {error}", "error:".red());
            return 1;
        }
    };
    super::worker_manager::libkrun::run_dev(
        "descriptor",
        worker_path,
        prepared.exec_path,
        &prepared.args,
        prepared.env,
        prepared.vcpus,
        prepared.ram_mib,
        prepared.rootfs,
        None,
        String::new(),
        None,
        true,
        worker_name,
        &prepared.mounts,
    )
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use iii_compose::descriptor::{
        Artifact, FrontendTool, PackageDescriptor, PackageSource, RegistryMetadata, Runtime,
        Validation, ValidationMode,
    };

    #[test]
    fn bundle_install_reads_only_descriptor_sidecars() {
        let temp = tempfile::tempdir().unwrap();
        let descriptor = PackageDescriptor {
            name: "probe".into(),
            version: "1.0.0".into(),
            source: PackageSource {
                path: ".".into(),
                package_manifest: "package.json".into(),
            },
            artifact: Artifact::JavascriptBundle {
                workspace_root: ".".into(),
                runtime: FrontendTool {
                    name: "node".into(),
                    version: "22".into(),
                },
                package_manager: FrontendTool {
                    name: "pnpm".into(),
                    version: "11".into(),
                },
                lockfile: "pnpm-lock.yaml".into(),
                install_command: vec!["pnpm".into(), "install".into()],
                build_command: vec!["pnpm".into(), "build".into()],
                include: vec!["dist/index.mjs".into()],
            },
            runtime: Runtime {
                exec: Some(vec!["node".into(), "dist/index.mjs".into()]),
                base_image: Some(format!("node@sha256:{}", "a".repeat(64))),
                ..Runtime::default()
            },
            registry: RegistryMetadata {
                description: "probe".into(),
                license: "Apache-2.0".into(),
                tags: vec![],
                dependencies: Default::default(),
                config: None,
                publish: true,
            },
            validation: Validation {
                interface: ValidationMode::Required,
            },
        };
        let digest = descriptor.sha256();
        std::fs::write(
            temp.path().join(".iii-package-descriptor.json"),
            serde_json::to_vec(&descriptor).unwrap(),
        )
        .unwrap();
        std::fs::write(temp.path().join(".iii-package-descriptor.sha256"), digest).unwrap();

        let spec = descriptor_vm_spec_from_install(temp.path(), "probe").unwrap();
        assert_eq!(spec.exec, ["node", "dist/index.mjs"]);
    }
}
