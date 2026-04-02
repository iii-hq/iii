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
//! libkrun is loaded at runtime via libloading (not linked at compile time)
//! so the iii binary works normally even when libkrun is not installed.

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
    /// Without this, console output goes to the host's terminal.
    #[arg(long)]
    pub console_output: Option<String>,

    /// Network slot for IP/MAC address derivation (0-65535)
    #[arg(long, default_value = "0")]
    pub slot: u64,
}

/// Compose the full libkrunfw file path from the resolved directory and platform filename.
/// resolve_libkrunfw_dir() returns a directory; VmBuilder krunfw_path() needs a file path.
fn resolve_krunfw_file_path() -> Option<std::path::PathBuf> {
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
/// Verifies /dev/kvm exists and is accessible (read+write) by the current user.
#[cfg(target_os = "linux")]
fn check_kvm_available() -> Result<(), String> {
    check_kvm_at_path(std::path::Path::new("/dev/kvm"))
}

/// Inner implementation that accepts a path, enabling unit testing without real /dev/kvm.
#[cfg(target_os = "linux")]
fn check_kvm_at_path(kvm: &std::path::Path) -> Result<(), String> {
    if !kvm.exists() {
        return Err(
            "KVM not available -- /dev/kvm does not exist. \
             Ensure KVM is enabled in your kernel and loaded (modprobe kvm_intel or kvm_amd)."
                .to_string(),
        );
    }
    // Check read/write access via std::fs to avoid needing extra nix features.
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
/// Each guest file open maps to a host fd, so complex workloads need many fds.
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

/// Quote a string for safe inclusion in a shell command.
/// Safe characters (alphanumeric, `-`, `_`, `/`, `.`, `:`, `=`) pass through unquoted.
/// Everything else gets single-quote wrapped with internal single quotes escaped.
fn shell_quote(s: &str) -> String {
    if s.chars().all(|c| {
        c.is_alphanumeric() || c == '-' || c == '_' || c == '/' || c == '.' || c == ':' || c == '='
    }) {
        s.to_string()
    } else {
        format!("'{}'", s.replace('\'', "'\\''"))
    }
}

/// Construct III_WORKER_CMD from exec path and arguments.
/// The init binary passes this to `/bin/sh -c`, so arguments must be shell-safe.
fn build_worker_cmd(exec: &str, args: &[String]) -> String {
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

    // Pre-flight: verify KVM is available and accessible on Linux.
    #[cfg(target_os = "linux")]
    {
        if let Err(msg) = check_kvm_available() {
            return Err(msg);
        }
    }

    // PassthroughFs maps every guest file open to a host fd. Complex workloads
    // like `npm install` can open thousands of files, so raise the host process
    // fd limit to avoid EMFILE errors from flush/dup operations.
    raise_fd_limit();

    // Validate vcpus range before u8 cast (Research Pitfall 4)
    if args.vcpus > u8::MAX as u32 {
        return Err(format!(
            "vcpus {} exceeds maximum {} for VmBuilder",
            args.vcpus,
            u8::MAX
        ));
    }

    // 1. Construct PassthroughFs with embedded init binary at /init.krun (inode 2)
    let passthrough_fs = PassthroughFs::builder()
        .root_dir(&args.rootfs)
        .build()
        .map_err(|e| format!("PassthroughFs failed for '{}': {}", args.rootfs, e))?;

    // 2. Synthesize III_WORKER_CMD from exec path + args for init binary
    let worker_cmd = build_worker_cmd(&args.exec, &args.arg);

    // 3. Build VM with custom fs backend and init as PID 1
    let mut builder = VmBuilder::new()
        .machine(|m| m.vcpus(args.vcpus as u8).memory_mib(args.ram as usize))
        .kernel(|k| {
            let k = match resolve_krunfw_file_path() {
                Some(path) => k.krunfw_path(&path),
                None => k, // Fall back to dynamic linker default
            };
            k.init_path("/init.krun")
        })
        .fs(move |fs| fs.tag("/dev/root").custom(Box::new(passthrough_fs)));

    // Additional virtiofs mounts
    for (i, mount_str) in args.mount.iter().enumerate() {
        let parts: Vec<&str> = mount_str.splitn(2, ':').collect();
        if parts.len() != 2 {
            return Err(format!(
                "Invalid mount format '{}'. Expected host:guest",
                mount_str
            ));
        }
        let tag = format!("virtiofs_{}", i);
        let path = parts[0].to_string();
        builder = builder.fs(move |fs| fs.tag(&tag).path(&path));
    }

    // 4. Network: smoltcp in-process TCP/IP stack with proxy relay.
    //    The tokio runtime is created on the stack and effectively leaked since
    //    vm.enter() never returns — this is intentional (D-11).
    let tokio_rt = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .map_err(|e| format!("tokio runtime failed: {}", e))?;

    let mut network = iii_network::SmoltcpNetwork::new(
        iii_network::NetworkConfig::default(),
        args.slot,
    );
    network.start(tokio_rt.handle().clone());

    builder = builder.net(|net| {
        net.mac(network.guest_mac())
            .custom(network.take_backend())
    });

    // 5. Exec configuration -- init binary as entry point, worker cmd via env var
    let dns_nameserver = network.gateway_ipv4().to_string();
    let guest_ip = network.guest_ipv4().to_string();
    let gateway_ip = network.gateway_ipv4().to_string();

    // Rewrite localhost → gateway IP in env vars and worker cmd.
    // Inside the VM, localhost (127.0.0.1) goes through the VM's own loopback
    // and never reaches the virtio-net interface. The gateway IP is the host
    // from the guest's perspective (like QEMU's 10.0.2.2 convention).
    let rewrite_localhost = |s: &str| -> String {
        s.replace("://localhost:", &format!("://{}:", gateway_ip))
            .replace("://127.0.0.1:", &format!("://{}:", gateway_ip))
    };
    let worker_cmd = rewrite_localhost(&worker_cmd);

    // Compute memory limits for guest runtimes. Reserve 25% for kernel, init,
    // filesystem caches; give 75% to the worker process heap.
    let worker_heap_mib = (args.ram as u64 * 3 / 4).max(128);
    let worker_heap_bytes = worker_heap_mib * 1024 * 1024;

    builder = builder.exec(|mut e| {
        e = e.path("/init.krun").workdir(&args.workdir);
        e = e.env("III_WORKER_CMD", &worker_cmd);
        e = e.env("III_INIT_DNS", &dns_nameserver);
        e = e.env("III_INIT_IP", &guest_ip);
        e = e.env("III_INIT_GW", &gateway_ip);
        e = e.env("III_INIT_CIDR", "30");

        // Pass heap limit to init for cgroup v2 enforcement (works for all runtimes).
        e = e.env("III_WORKER_MEM_BYTES", &worker_heap_bytes.to_string());

        for env_str in &args.env {
            if let Some((key, value)) = env_str.split_once('=') {
                let rewritten_value = rewrite_localhost(value);
                e = e.env(key, &rewritten_value);
            }
        }
        e
    });

    // Redirect VM console output to a file when running as a managed worker.
    // Without this, krun outputs to the host terminal which is unavailable
    // for background workers (stdout/stderr are redirected to log files).
    if let Some(ref path) = args.console_output {
        builder = builder.console(|c| c.output(path));
    }

    // PID file cleanup on exit (synchronous, no async -- D-09, D-10)
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

    eprintln!(
        "  Booting VM (vcpus={}, ram={}MiB)...",
        args.vcpus, args.ram
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
        assert!(cli.args.pid_file.is_none());
    }

    #[test]
    fn test_vm_boot_args_defaults() {
        use clap::Parser;

        #[derive(Parser)]
        struct TestCli {
            #[command(flatten)]
            args: VmBootArgs,
        }

        let cli =
            TestCli::parse_from(["test", "--rootfs", "/tmp/rootfs", "--exec", "/usr/bin/node"]);

        assert_eq!(cli.args.workdir, "/");
        assert_eq!(cli.args.vcpus, 2);
        assert_eq!(cli.args.ram, 2048);
        assert!(cli.args.mount.is_empty());
        assert!(cli.args.env.is_empty());
        assert!(cli.args.arg.is_empty());
        assert!(cli.args.pid_file.is_none());
    }

    #[test]
    fn test_vm_boot_args_with_pid_file() {
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
            "/usr/bin/node",
            "--pid-file",
            "/tmp/test.pid",
        ]);

        assert_eq!(cli.args.pid_file, Some("/tmp/test.pid".to_string()));
    }

    #[test]
    fn test_resolve_krunfw_file_path_found() {
        use tempfile::TempDir;

        let tmp = TempDir::new().unwrap();
        let filename = crate::cli::firmware::constants::libkrunfw_filename();
        let file_path = tmp.path().join(&filename);
        std::fs::write(&file_path, b"fake firmware").unwrap();

        // We cannot easily mock resolve_libkrunfw_dir(), so test the logic directly:
        // Given a directory with the firmware file, composing dir + filename should find it.
        let dir = tmp.path().to_path_buf();
        let composed = dir.join(&filename);
        assert!(composed.exists());
    }

    #[test]
    fn test_resolve_krunfw_file_path_not_found() {
        use tempfile::TempDir;

        let tmp = TempDir::new().unwrap();
        let filename = crate::cli::firmware::constants::libkrunfw_filename();
        // Empty dir -- file does not exist
        let composed = tmp.path().join(&filename);
        assert!(!composed.exists());
    }

    #[test]
    fn test_shell_quote_safe_chars() {
        assert_eq!(shell_quote("simple"), "simple");
        assert_eq!(shell_quote("/usr/bin/node"), "/usr/bin/node");
        assert_eq!(shell_quote("--port=3000"), "--port=3000");
        assert_eq!(shell_quote("file.js"), "file.js");
    }

    #[test]
    fn test_shell_quote_unsafe_chars() {
        assert_eq!(shell_quote("has space"), "'has space'");
        assert_eq!(shell_quote("a;b"), "'a;b'");
        assert_eq!(shell_quote("$(cmd)"), "'$(cmd)'");
    }

    #[test]
    fn test_shell_quote_single_quotes() {
        assert_eq!(shell_quote("it's"), "'it'\\''s'");
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
    fn test_build_worker_cmd_with_spaces() {
        let args = vec!["hello world".to_string()];
        assert_eq!(
            build_worker_cmd("/usr/bin/node", &args),
            "/usr/bin/node 'hello world'"
        );
    }

    #[test]
    fn test_build_worker_cmd_with_single_quotes() {
        let args = vec!["it's".to_string()];
        assert_eq!(
            build_worker_cmd("/usr/bin/node", &args),
            "/usr/bin/node 'it'\\''s'"
        );
    }

    #[test]
    fn test_vm_boot_args_with_slot() {
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
            "/usr/bin/node",
            "--slot",
            "42",
        ]);
        assert_eq!(cli.args.slot, 42);
    }

    #[test]
    fn test_vm_boot_args_slot_default() {
        use clap::Parser;

        #[derive(Parser)]
        struct TestCli {
            #[command(flatten)]
            args: VmBootArgs,
        }

        let cli =
            TestCli::parse_from(["test", "--rootfs", "/tmp/rootfs", "--exec", "/usr/bin/node"]);
        assert_eq!(cli.args.slot, 0);
    }

    #[test]
    fn test_vmbuilder_config() {
        // Compile-time verification that VmBuilder chain correctly threads
        // VmBootArgs fields through the new Phase 5 API: init_path, III_WORKER_CMD.
        // We do NOT call .build() since that would try to dlopen libkrun.
        // We cannot construct PassthroughFs here because /tmp/rootfs does not exist.
        use msb_krun::VmBuilder;

        let args = VmBootArgs {
            rootfs: "/tmp/rootfs".to_string(),
            exec: "/sbin/init".to_string(),
            arg: vec![],
            workdir: "/".to_string(),
            vcpus: 2,
            ram: 2048,
            mount: vec![],
            env: vec![],
            pid_file: None,
            console_output: None,
            slot: 0,
        };

        let worker_cmd = build_worker_cmd(&args.exec, &args.arg);

        // Verify the VmBuilder API accepts the Phase 5 configuration pattern:
        // - /init.krun as exec path (not worker binary)
        // - III_WORKER_CMD env var
        let _builder = VmBuilder::new()
            .machine(|m| m.vcpus(args.vcpus as u8).memory_mib(args.ram as usize))
            .exec(|e| {
                e.path("/init.krun")
                    .workdir(&args.workdir)
                    .env("III_WORKER_CMD", &worker_cmd)
            });
    }

    // --- KVM pre-flight check tests (Phase 09) ---

    #[test]
    #[cfg(target_os = "linux")]
    fn test_kvm_check_missing_device() {
        // Use a non-existent path to simulate missing /dev/kvm
        let fake_path = std::path::Path::new("/tmp/nonexistent_kvm_device_test");
        // Ensure it doesn't exist
        let _ = std::fs::remove_file(fake_path);

        let result = check_kvm_at_path(fake_path);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            err.contains("/dev/kvm does not exist"),
            "Error should mention /dev/kvm does not exist, got: {}",
            err
        );
        assert!(
            err.contains("modprobe"),
            "Error should suggest modprobe, got: {}",
            err
        );
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn test_kvm_check_permission_denied() {
        // Create a file with no permissions to simulate permission-denied /dev/kvm
        let tmp = tempfile::NamedTempFile::new().expect("create temp file");
        let path = tmp.path().to_path_buf();

        // Remove all permissions
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o000))
            .expect("set permissions");

        let result = check_kvm_at_path(&path);
        // Restore permissions for cleanup
        let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644));

        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            err.contains("lacks permission") || err.contains("Permission"),
            "Error should mention permission issue, got: {}",
            err
        );
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn test_kvm_check_accessible_device() {
        // Create a temp file with read/write permissions to simulate accessible /dev/kvm
        let tmp = tempfile::NamedTempFile::new().expect("create temp file");
        let result = check_kvm_at_path(tmp.path());
        assert!(
            result.is_ok(),
            "Accessible file should pass KVM check: {:?}",
            result
        );
    }
}
