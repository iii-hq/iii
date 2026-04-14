// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at support@motia.dev
// See LICENSE and PATENTS files for details.

//! Hidden `__vm-boot` subcommand -- boots a libkrun microVM.
//!
//! Runs in a separate process (spawned via `current_exe() __vm-boot`)
//! for crash isolation. If libkrun segfaults, only this child dies.
//!
//! Uses msb_krun VmBuilder for type-safe VM configuration.

/// Arguments for the `__vm-boot` hidden subcommand.
#[derive(clap::Args, Debug)]
pub struct VmBootArgs {
    /// Path to the guest rootfs directory
    #[arg(long)]
    pub rootfs: String,

    /// Executable path inside the guest
    #[arg(long)]
    pub exec: String,

    /// Arguments to pass to the guest executable
    #[arg(long, allow_hyphen_values = true)]
    pub arg: Vec<String>,

    /// Working directory inside the guest
    #[arg(long, default_value = "/")]
    pub workdir: String,

    /// Number of vCPUs
    #[arg(long, default_value = "2")]
    pub vcpus: u32,

    /// RAM in MiB
    #[arg(long, default_value = "2048")]
    pub ram: u32,

    /// Volume mounts (host_path:guest_path)
    #[arg(long)]
    pub mount: Vec<String>,

    /// Environment variables (KEY=VALUE)
    #[arg(long)]
    pub env: Vec<String>,

    /// PID file to clean up on VM exit (managed workers only)
    #[arg(long)]
    pub pid_file: Option<String>,

    /// Redirect VM console output to this file (managed workers only).
    #[arg(long)]
    pub console_output: Option<String>,

    /// Network slot for IP/MAC address derivation (0-65535)
    #[arg(long, default_value = "0")]
    pub slot: u64,
}

/// One `--mount host:guest` CLI arg, expanded into the virtiofs attach plan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VirtiofsMountEntry {
    pub tag: String,
    pub host_path: String,
    pub guest_path: String,
}

/// Output of [`build_virtiofs_mount_plan`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VirtiofsMountPlan {
    /// Virtio-fs attach entries, in CLI arg order. Each gets a `virtiofs_N` tag.
    pub entries: Vec<VirtiofsMountEntry>,
    /// Value of `III_VIRTIOFS_MOUNTS` to pass into the guest env:
    /// `tag1=/guest/path1;tag2=/guest/path2`.
    pub env_var: String,
}

/// Parse `--mount host:guest` CLI args into a virtiofs attach plan and the
/// matching `III_VIRTIOFS_MOUNTS` env string the guest (iii-init) will consume.
///
/// Returns `Err` on the first malformed entry so a bad CLI arg fails the VM
/// boot instead of producing a partial attach plan.
pub fn build_virtiofs_mount_plan(mounts: &[String]) -> Result<VirtiofsMountPlan, String> {
    let mut entries = Vec::with_capacity(mounts.len());
    let mut env_var = String::new();
    for (i, mount_str) in mounts.iter().enumerate() {
        let (host_path, guest_path) = match mount_str.split_once(':') {
            Some((h, g)) if !h.is_empty() && !g.is_empty() => (h.to_string(), g.to_string()),
            _ => {
                return Err(format!(
                    "Invalid mount format '{}'. Expected host:guest",
                    mount_str
                ));
            }
        };
        let tag = format!("virtiofs_{}", i);
        if !env_var.is_empty() {
            env_var.push(';');
        }
        env_var.push_str(&tag);
        env_var.push('=');
        env_var.push_str(&guest_path);
        entries.push(VirtiofsMountEntry {
            tag,
            host_path,
            guest_path,
        });
    }
    Ok(VirtiofsMountPlan { entries, env_var })
}

/// Compose the full libkrunfw file path from the resolved directory and platform filename.
pub fn resolve_krunfw_file_path() -> Option<std::path::PathBuf> {
    let dir = crate::cli::firmware::resolve::resolve_libkrunfw_dir()?;
    let filename = crate::cli::firmware::constants::libkrunfw_filename();
    let file_path = dir.join(&filename);
    if file_path.exists() {
        Some(file_path)
    } else {
        None
    }
}

/// Pre-flight check for KVM availability on Linux.
#[cfg(target_os = "linux")]
fn check_kvm_available() -> Result<(), String> {
    check_kvm_at_path(std::path::Path::new("/dev/kvm"))
}

#[cfg(target_os = "linux")]
fn check_kvm_at_path(kvm: &std::path::Path) -> Result<(), String> {
    if !kvm.exists() {
        return Err("KVM not available -- /dev/kvm does not exist. \
             Ensure KVM is enabled in your kernel and loaded (modprobe kvm_intel or kvm_amd)."
            .to_string());
    }
    match std::fs::File::options().read(true).write(true).open(kvm) {
        Ok(_) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => Err(
            "KVM not accessible -- /dev/kvm exists but current user lacks permission. \
             Add your user to the 'kvm' group: sudo usermod -aG kvm $USER"
                .to_string(),
        ),
        Err(e) => Err(format!("KVM check failed: {}", e)),
    }
}

/// Raise the process fd limit (RLIMIT_NOFILE) to accommodate PassthroughFs.
fn raise_fd_limit() {
    use nix::libc;
    let mut rlim: libc::rlimit = unsafe { std::mem::zeroed() };
    if unsafe { libc::getrlimit(libc::RLIMIT_NOFILE, &mut rlim) } == 0 {
        let target = rlim.rlim_max.min(1_048_576);
        if rlim.rlim_cur < target {
            rlim.rlim_cur = target;
            unsafe { libc::setrlimit(libc::RLIMIT_NOFILE, &rlim) };
        }
    }
}

pub fn shell_quote(s: &str) -> String {
    if s.chars().all(|c| {
        c.is_alphanumeric() || c == '-' || c == '_' || c == '/' || c == '.' || c == ':' || c == '='
    }) {
        s.to_string()
    } else {
        format!("'{}'", s.replace('\'', "'\\''"))
    }
}

pub fn build_worker_cmd(exec: &str, args: &[String]) -> String {
    if args.is_empty() {
        shell_quote(exec)
    } else {
        let mut parts = vec![shell_quote(exec)];
        for arg in args {
            parts.push(shell_quote(arg));
        }
        parts.join(" ")
    }
}

/// Rewrite localhost/loopback URLs to use the given gateway IP.
/// Used by the VM boot process to redirect traffic into the guest network.
pub fn rewrite_localhost(s: &str, gateway_ip: &str) -> String {
    s.replace("://localhost:", &format!("://{}:", gateway_ip))
        .replace("://127.0.0.1:", &format!("://{}:", gateway_ip))
}

/// Boot the VM. Called from `main()` when `__vm-boot` is parsed.
/// This function does NOT return -- `krun_start_enter` replaces the process.
pub fn run(args: &VmBootArgs) -> ! {
    if !std::path::Path::new(&args.rootfs).exists() {
        eprintln!("error: rootfs path does not exist: {}", args.rootfs);
        std::process::exit(1);
    }

    match boot_vm(args) {
        Ok(infallible) => match infallible {},
        Err(e) => {
            eprintln!("error: VM execution failed: {}", e);
            std::process::exit(1);
        }
    }
}

fn boot_vm(args: &VmBootArgs) -> Result<std::convert::Infallible, String> {
    use iii_filesystem::PassthroughFs;
    use msb_krun::VmBuilder;

    #[cfg(target_os = "linux")]
    {
        if let Err(msg) = check_kvm_available() {
            return Err(msg);
        }
    }

    raise_fd_limit();

    // Pre-boot validation: ensure init binary is available either embedded or on-disk
    if !iii_filesystem::init::has_init() {
        let init_on_disk = std::path::Path::new(&args.rootfs).join("init.krun");
        if !init_on_disk.exists() {
            return Err(format!(
                "No init binary available. /init.krun not found in rootfs '{}' \
                 and no init binary is embedded in this build.\n\
                 Hint: Run `iii worker dev` which auto-provisions the init binary, \
                 or rebuild with --features embed-init.",
                args.rootfs
            ));
        }
    }

    if args.vcpus > u8::MAX as u32 {
        return Err(format!(
            "vcpus {} exceeds maximum {} for VmBuilder",
            args.vcpus,
            u8::MAX
        ));
    }

    let passthrough_fs = PassthroughFs::builder()
        .root_dir(&args.rootfs)
        .build()
        .map_err(|e| format!("PassthroughFs failed for '{}': {}", args.rootfs, e))?;

    let worker_cmd = build_worker_cmd(&args.exec, &args.arg);

    let mut builder = VmBuilder::new()
        .machine(|m| m.vcpus(args.vcpus as u8).memory_mib(args.ram as usize))
        .kernel(|k| {
            let k = match resolve_krunfw_file_path() {
                Some(path) => k.krunfw_path(&path),
                None => k,
            };
            k.init_path("/init.krun")
        })
        .fs(move |fs| fs.tag("/dev/root").custom(Box::new(passthrough_fs)));

    let mount_plan = build_virtiofs_mount_plan(&args.mount)?;
    let virtiofs_mount_env = mount_plan.env_var.clone();
    for entry in mount_plan.entries {
        let tag = entry.tag.clone();
        let host_path = entry.host_path.clone();
        builder = builder.fs(move |fs| fs.tag(&tag).path(&host_path));
    }

    let tokio_rt = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .map_err(|e| format!("tokio runtime failed: {}", e))?;

    let mut network =
        iii_network::SmoltcpNetwork::new(iii_network::NetworkConfig::default(), args.slot);
    network.start(tokio_rt.handle().clone());

    builder = builder.net(|net| net.mac(network.guest_mac()).custom(network.take_backend()));

    let dns_nameserver = network.gateway_ipv4().to_string();
    let guest_ip = network.guest_ipv4().to_string();
    let gateway_ip = network.gateway_ipv4().to_string();

    let rewrite_localhost = |s: &str| -> String { rewrite_localhost(s, &gateway_ip) };
    let worker_cmd = rewrite_localhost(&worker_cmd);

    let worker_heap_mib = (args.ram as u64 * 3 / 4).max(128);
    let worker_heap_bytes = worker_heap_mib * 1024 * 1024;

    builder = builder.exec(|mut e| {
        e = e.path("/init.krun").workdir(&args.workdir);
        e = e.env("III_WORKER_CMD", &worker_cmd);
        e = e.env("III_INIT_DNS", &dns_nameserver);
        e = e.env("III_INIT_IP", &guest_ip);
        e = e.env("III_INIT_GW", &gateway_ip);
        e = e.env("III_INIT_CIDR", "30");
        e = e.env("III_WORKER_MEM_BYTES", &worker_heap_bytes.to_string());
        if !virtiofs_mount_env.is_empty() {
            e = e.env("III_VIRTIOFS_MOUNTS", &virtiofs_mount_env);
        }

        for env_str in &args.env {
            if let Some((key, value)) = env_str.split_once('=') {
                let rewritten_value = rewrite_localhost(value);
                e = e.env(key, &rewritten_value);
            }
        }
        e
    });

    if let Some(ref path) = args.console_output {
        builder = builder.console(|c| c.output(path));
    }

    if let Some(ref pid_path) = args.pid_file {
        let path = pid_path.clone();
        builder = builder.on_exit(move |exit_code| {
            let _ = std::fs::remove_file(&path);
            if exit_code != 0 {
                eprintln!("  VM exited with code {}", exit_code);
            }
        });
    }

    let vm = builder
        .build()
        .map_err(|e| format!("VM build failed: {}", e))?;

    let vcpu_label = if args.vcpus == 1 { "vCPU" } else { "vCPUs" };
    eprintln!(
        "  Booting VM ({} {}, {} MiB RAM)...",
        args.vcpus, vcpu_label, args.ram
    );
    vm.enter().map_err(|e| format!("VM enter failed: {}", e))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_vm_boot_args_parse() {
        use clap::Parser;

        #[derive(Parser)]
        struct TestCli {
            #[command(flatten)]
            args: VmBootArgs,
        }

        let cli = TestCli::parse_from([
            "test",
            "--rootfs",
            "/tmp/rootfs",
            "--exec",
            "/usr/bin/python3",
            "--workdir",
            "/workspace",
            "--vcpus",
            "4",
            "--ram",
            "1024",
            "--env",
            "FOO=bar",
            "--arg",
            "script.py",
        ]);

        assert_eq!(cli.args.rootfs, "/tmp/rootfs");
        assert_eq!(cli.args.exec, "/usr/bin/python3");
        assert_eq!(cli.args.workdir, "/workspace");
        assert_eq!(cli.args.vcpus, 4);
        assert_eq!(cli.args.ram, 1024);
        assert_eq!(cli.args.env, vec!["FOO=bar"]);
        assert_eq!(cli.args.arg, vec!["script.py"]);
    }

    #[test]
    fn test_shell_quote_safe_chars() {
        assert_eq!(shell_quote("simple"), "simple");
        assert_eq!(shell_quote("/usr/bin/node"), "/usr/bin/node");
    }

    #[test]
    fn test_shell_quote_unsafe_chars() {
        assert_eq!(shell_quote("has space"), "'has space'");
    }

    #[test]
    fn test_build_worker_cmd_no_args() {
        assert_eq!(build_worker_cmd("/usr/bin/node", &[]), "/usr/bin/node");
    }

    #[test]
    fn test_build_worker_cmd_with_args() {
        let args = vec![
            "script.js".to_string(),
            "--port".to_string(),
            "3000".to_string(),
        ];
        assert_eq!(
            build_worker_cmd("/usr/bin/node", &args),
            "/usr/bin/node script.js --port 3000"
        );
    }

    #[test]
    fn build_virtiofs_mount_plan_empty() {
        let plan = build_virtiofs_mount_plan(&[]).unwrap();
        assert!(plan.entries.is_empty());
        assert!(plan.env_var.is_empty());
    }

    #[test]
    fn build_virtiofs_mount_plan_single() {
        let plan = build_virtiofs_mount_plan(&["/host/proj:/workspace".to_string()]).unwrap();
        assert_eq!(plan.entries.len(), 1);
        assert_eq!(plan.entries[0].tag, "virtiofs_0");
        assert_eq!(plan.entries[0].host_path, "/host/proj");
        assert_eq!(plan.entries[0].guest_path, "/workspace");
        assert_eq!(plan.env_var, "virtiofs_0=/workspace");
    }

    #[test]
    fn build_virtiofs_mount_plan_multiple_preserves_order_and_indexes_tags() {
        let plan = build_virtiofs_mount_plan(&["/host/a:/b".to_string(), "/host/c:/d".to_string()])
            .unwrap();
        assert_eq!(plan.entries.len(), 2);
        assert_eq!(plan.entries[0].tag, "virtiofs_0");
        assert_eq!(plan.entries[0].host_path, "/host/a");
        assert_eq!(plan.entries[0].guest_path, "/b");
        assert_eq!(plan.entries[1].tag, "virtiofs_1");
        assert_eq!(plan.entries[1].host_path, "/host/c");
        assert_eq!(plan.entries[1].guest_path, "/d");
        assert_eq!(plan.env_var, "virtiofs_0=/b;virtiofs_1=/d");
    }

    #[test]
    fn build_virtiofs_mount_plan_rejects_missing_colon() {
        let err = build_virtiofs_mount_plan(&["/host/noguest".to_string()]).unwrap_err();
        assert!(err.contains("Invalid mount format"));
    }

    #[test]
    fn build_virtiofs_mount_plan_rejects_empty_host() {
        let err = build_virtiofs_mount_plan(&[":/guest".to_string()]).unwrap_err();
        assert!(err.contains("Invalid mount format"));
    }

    #[test]
    fn build_virtiofs_mount_plan_rejects_empty_guest() {
        let err = build_virtiofs_mount_plan(&["/host:".to_string()]).unwrap_err();
        assert!(err.contains("Invalid mount format"));
    }

    // --- 6.1: check_kvm_nonexistent_path (Linux only) ---
    #[cfg(target_os = "linux")]
    #[test]
    fn test_check_kvm_nonexistent_path() {
        let result = check_kvm_at_path(std::path::Path::new("/dev/nonexistent_kvm"));
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("does not exist"));
    }

    // --- 6.2: shell_quote with embedded single quotes ---
    #[test]
    fn test_shell_quote_with_embedded_single_quotes() {
        let result = shell_quote("it's a test");
        assert_eq!(result, "'it'\\''s a test'");
    }
}
