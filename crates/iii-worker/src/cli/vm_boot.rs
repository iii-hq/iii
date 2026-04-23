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

    /// Unix socket path where this `__vm-boot` process will listen for
    /// control requests from the host (source watcher, stop handler).
    ///
    /// When set, `__vm-boot` creates an internal `socketpair(AF_UNIX)`,
    /// wires one end into the VM as a virtio-console port named
    /// `iii.control` (guest device: `/dev/vport0p1`), and spawns a
    /// proxy thread that serves the listed socket: incoming bytes are
    /// forwarded to the VM's port, replies are forwarded back. The
    /// in-VM `iii-supervisor` reads/writes the other end.
    ///
    /// When absent, the VM boots without a control port and all
    /// restarts fall back to the full `iii-worker start` path.
    #[arg(long)]
    pub control_sock: Option<String>,

    /// Enable SMT / hyperthreading in the guest. Default off — matches
    /// Firecracker's conservative default and avoids noisy-neighbour
    /// effects between co-tenant workers. Flip on to squeeze more
    /// throughput on Intel hosts where the perf delta is measurable.
    #[arg(long, default_value = "false")]
    pub hyperthreading: bool,

    /// Enable nested virtualization. Default off — a microworker VM has
    /// no need to run its own hypervisor, and enabling nested-KVM is a
    /// measurable perf hit. Flip on only for CI/test scenarios that
    /// explicitly boot a VM inside the worker.
    #[arg(long, default_value = "false")]
    pub nested_virt: bool,

    /// DAX window size (MiB) for every virtiofs mount. 0 means
    /// "take the msb_krun default". Larger windows cut syscall
    /// roundtrips on read-heavy mounts (node_modules, .venv,
    /// site-packages); cost is guest virtual address space and a
    /// small amount of host RAM per mount. Safe starting point:
    /// 256 for dev workflows with large dep trees.
    #[arg(long, default_value = "0")]
    pub virtiofs_shm_size_mib: u32,

    /// Soft / hard limit for `RLIMIT_NOFILE` applied to the guest
    /// worker process via libkrun's `KRUN_RLIMITS`. Complements the
    /// guest-side raise in `iii-init::rlimit::raise_nofile` — setting
    /// it here means every fd opened by `init.krun` BEFORE iii-init
    /// runs (dynamic loader, early allocations) also sees the raised
    /// limit. 0 means "don't set" (fall back to iii-init's raise).
    #[arg(long, default_value = "65536")]
    pub nofile_limit: u64,

    /// Unix socket path where this `__vm-boot` process will listen for
    /// `iii worker exec` clients. Separate from `--control-sock`
    /// because exec-style sessions are multiplexed (many concurrent
    /// requests on one virtio-console port), which the single-request
    /// control proxy doesn't support.
    ///
    /// When set, `__vm-boot` creates a second `socketpair(AF_UNIX)`,
    /// wires one end into the VM as a named virtio-console port
    /// (`iii.exec`), and spawns the async [`crate::cli::shell_relay`]
    /// task on the existing tokio runtime to route frames between
    /// connecting clients and the VM. The in-VM `iii-init` shell
    /// dispatcher reads/writes the other end.
    ///
    /// When absent, the VM boots without an exec port and
    /// `iii worker exec <name>` refuses with a clear "unavailable"
    /// error rather than silently hanging on a missing socket.
    #[arg(long)]
    pub shell_sock: Option<String>,
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

/// Identity of a bound control socket: path plus (dev, ino) of the
/// filesystem entry that `UnixListener::bind` produced. Captured at
/// bind time and consulted by the VM-exit cleanup hook so we only
/// unlink the socket we created — never a replacement someone else
/// bound at the same path in the meantime.
#[cfg(unix)]
#[derive(Debug, Clone)]
struct SocketFingerprint {
    path: String,
    dev: u64,
    ino: u64,
}

#[cfg(unix)]
impl SocketFingerprint {
    /// Remove the socket file iff its (dev, ino) still match what we
    /// captured at bind time. Silently does nothing if the path has
    /// been replaced (another VM rebinding), is gone, or can't be
    /// stat'd — all three are benign at VM-exit time.
    fn remove_if_unchanged(&self) {
        use std::os::unix::fs::MetadataExt;
        match std::fs::metadata(&self.path) {
            Ok(m) if m.dev() == self.dev && m.ino() == self.ino => {
                let _ = std::fs::remove_file(&self.path);
            }
            _ => {
                // Replaced, gone, or unstatable — leave it alone.
            }
        }
    }
}

/// Create a unix stream `socketpair` for the control channel. The guest
/// fd will be handed to `ConsoleBuilder::port` and must remain open for
/// the lifetime of the VM; we deliberately `forget` the owned wrapper
/// and return the raw fd number so it isn't closed when this function
/// returns. The host end is kept as a `UnixStream` in the caller.
///
/// Rationale: libkrun takes ownership of the guest fd internally at
/// VM build time. Dropping our Rust-owned wrapper after handing the
/// raw fd to libkrun would close the fd underneath the VM; forgetting
/// keeps it alive. Closing happens on process exit, which is exactly
/// when the VM goes down — correct.
#[cfg(unix)]
fn setup_control_socketpair() -> Result<(std::os::unix::net::UnixStream, i32), String> {
    use std::os::fd::{AsRawFd, IntoRawFd};
    use std::os::unix::net::UnixStream;

    let (host_end, guest_end) =
        UnixStream::pair().map_err(|e| format!("control socketpair: {e}"))?;

    // Clear CLOEXEC on the guest fd so it remains open through any
    // libkrun internal fork/exec pathway. (On most platforms
    // UnixStream::pair sets FD_CLOEXEC; libkrun consumes the fd
    // in-process so CLOEXEC doesn't matter today, but this is cheap
    // insurance against future changes.)
    unsafe {
        let fd = guest_end.as_raw_fd();
        let flags = nix::libc::fcntl(fd, nix::libc::F_GETFD);
        if flags >= 0 {
            nix::libc::fcntl(fd, nix::libc::F_SETFD, flags & !nix::libc::FD_CLOEXEC);
        }
    }

    let guest_fd = guest_end.into_raw_fd();
    Ok((host_end, guest_fd))
}

/// Spawn a detached background thread that listens on `sock_path` and
/// bridges accepted connections to `host_end`. Each client gets
/// exclusive access to the channel for the duration of their
/// connection (the supervisor protocol is strictly request/response,
/// one in flight at a time).
///
/// The listener is bound to a fresh unix socket — any stale file at
/// `sock_path` is unlinked first. The `on_exit` hook in the caller
/// unlinks it again on VM shutdown so stop leaves a tidy managed dir.
/// Even without that cleanup the file becomes inert when the
/// __vm-boot process dies, and any subsequent `iii worker start`
/// overwrites it via the same unlink step below.
#[cfg(unix)]
fn spawn_control_proxy(
    sock_path: String,
    host_end: std::os::unix::net::UnixStream,
) -> Option<SocketFingerprint> {
    use std::os::unix::fs::{MetadataExt, PermissionsExt};
    use std::os::unix::net::{UnixListener, UnixStream};
    use std::path::PathBuf;
    use std::sync::{Arc, Mutex};
    use std::thread;
    use std::time::Duration;

    /// Host↔VM round-trip budget. Matches the host-side
    /// `supervisor_ctl::DEFAULT_TIMEOUT` so callers' timeouts and the
    /// proxy's timeouts converge on the same deadline.
    const VM_IO_TIMEOUT: Duration = Duration::from_millis(500);

    /// Cap on a single request or response line, matching
    /// `iii_supervisor::control::serve_with`. A client streaming bytes
    /// without a newline can't grow memory beyond this.
    const MAX_LINE: usize = 4096;

    // Ensure the parent dir exists and any stale socket is gone. The
    // parent dir should already exist (managed_dir is created earlier
    // in the worker start path), but we don't rely on that.
    if let Some(parent) = PathBuf::from(&sock_path).parent() {
        let _ = std::fs::create_dir_all(parent);
        // Lock the parent to 0o700 so no other local user can traverse
        // into it to reach the control socket. Best-effort.
        let _ = std::fs::set_permissions(parent, std::fs::Permissions::from_mode(0o700));
    }
    let _ = std::fs::remove_file(&sock_path);

    // Narrow umask around the bind so the socket inode is created with
    // 0o600 mode from the start, rather than born with the process
    // umask (typically 0o002 / 0o022) and chmod'd afterwards. The prior
    // `set_permissions` approach left a narrow TOCTOU window where a
    // same-uid attacker could `connect()` before we locked the perms;
    // SO_PEERCRED would reject cross-uid clients but not same-uid ones.
    // umask(0o077) yields 0o600 files (default mode 0o666 & !0o077 = 0o600),
    // closing the window. Restore the prior umask immediately after bind.
    //
    // Kept for defense-in-depth: SO_PEERCRED (below) remains the primary
    // authz check and the parent dir is already 0o700 from line ~316.
    //
    // Caveat: `umask` is process-global, not per-thread. If a concurrent
    // Tokio task (or any other thread) happens to create a file between
    // the `umask(0o077)` and restore call, that file also inherits 0o077.
    // The window is a few syscalls wide and `iii worker start` is
    // effectively serial at this point in boot, so the practical risk is
    // low; the `set_permissions` follow-up below catches the rare miss.
    let prev_umask = unsafe { nix::libc::umask(0o077) };
    let bind_result = UnixListener::bind(&sock_path);
    unsafe {
        nix::libc::umask(prev_umask);
    }
    let listener = match bind_result {
        Ok(l) => l,
        Err(e) => {
            eprintln!(
                "warning: control proxy bind({sock_path}) failed: {e}. \
                 Fast-path restart is disabled; full VM restarts still work."
            );
            return None;
        }
    };
    // Belt-and-suspenders: some platforms or `nix` versions of `umask`
    // might not influence Unix-socket inode creation mode. The
    // `set_permissions` call here is a no-op when bind already produced
    // 0o600 (the common case), and a corrective chmod otherwise.
    let _ = std::fs::set_permissions(&sock_path, std::fs::Permissions::from_mode(0o600));

    // Capture (dev, ino) of the just-bound socket so the on_exit hook
    // can refuse to unlink if the file at `sock_path` has since been
    // replaced — a fast `stop → start` race can let the new VM bind
    // the same path before the old VM's on_exit fires; without this
    // fingerprint check the stale hook would nuke the live socket
    // and silently downgrade fast-path restart to a full-VM cycle.
    let fingerprint = std::fs::metadata(&sock_path)
        .ok()
        .map(|m| SocketFingerprint {
            path: sock_path.clone(),
            dev: m.dev(),
            ino: m.ino(),
        });

    // Cap how long any single read from the VM end can block. A wedged
    // supervisor otherwise pins the mutex forever and every subsequent
    // client deadlocks on host.lock(). 500ms matches the host-side
    // supervisor_ctl round-trip budget.
    let _ = host_end.set_read_timeout(Some(VM_IO_TIMEOUT));
    let _ = host_end.set_write_timeout(Some(VM_IO_TIMEOUT));

    let host = Arc::new(Mutex::new(host_end));
    let our_uid = unsafe { nix::libc::geteuid() };

    thread::Builder::new()
        .name("iii-control-proxy".to_string())
        .spawn(move || {
            for conn in listener.incoming() {
                let client = match conn {
                    Ok(c) => c,
                    Err(_) => continue,
                };
                // Reject clients running as a different uid. The control
                // channel is strictly for the local owner of the worker.
                if !peer_uid_matches(&client, our_uid) {
                    continue;
                }

                let mut guard = match host.lock() {
                    Ok(g) => g,
                    Err(_) => return, // poisoned — give up
                };

                if !proxy_one_round_trip(client, &mut guard, MAX_LINE) {
                    // VM-end failure (EOF, IO error, timeout). Drop the
                    // listener so future fast-restarts fall back to the
                    // full path instead of accepting doomed connections.
                    return;
                }
                // Drop client at end of iteration → connection closes.
            }
        })
        .expect("spawn control proxy thread");

    /// One request/response exchange. Returns `true` to keep the
    /// listener alive, `false` if the host↔VM channel itself has
    /// failed (caller should abandon the proxy thread).
    #[cfg(unix)]
    fn proxy_one_round_trip(client: UnixStream, vm: &mut UnixStream, max_line: usize) -> bool {
        use std::io::{BufRead, BufReader, Read, Write};
        // Client-side reads can hang on a slow peer but the listener
        // is still useful — bound the client read with the same budget
        // as the VM side so a lazy attacker can't wedge the proxy.
        let _ = client.set_read_timeout(Some(VM_IO_TIMEOUT));
        let _ = client.set_write_timeout(Some(VM_IO_TIMEOUT));

        let mut client_reader = BufReader::new(&client);
        let mut req = Vec::with_capacity(128);
        match (&mut client_reader)
            .take(max_line as u64)
            .read_until(b'\n', &mut req)
        {
            Ok(0) => return true, // client closed without sending — next
            Ok(_) => {}
            Err(_) => return true, // slow/bad client, keep listener
        }
        // Forward to VM.
        if vm.write_all(&req).is_err() || vm.flush().is_err() {
            return false;
        }

        // Pull one line of response from VM back to client.
        let mut vm_reader = BufReader::new(&*vm);
        let mut resp = Vec::with_capacity(64);
        let read_result = (&mut vm_reader)
            .take(max_line as u64)
            .read_until(b'\n', &mut resp);
        let mut client_writer = &client;
        match read_result {
            Ok(0) => false, // VM closed — supervisor gone
            Ok(_) => {
                let _ = client_writer.write_all(&resp);
                let _ = client_writer.flush();
                true
            }
            Err(_) => false, // VM timeout or IO error — bail
        }
    }

    /// Check SO_PEERCRED on Linux / LOCAL_PEERCRED via getpeereid on
    /// macOS+BSD: does the connecting peer share our euid? If not,
    /// refuse the connection. Protects against lateral attacks on
    /// multi-tenant dev hosts where $HOME perms can't be relied on.
    #[cfg(unix)]
    fn peer_uid_matches(stream: &UnixStream, expected: u32) -> bool {
        use std::os::unix::io::AsRawFd;

        let fd = stream.as_raw_fd();

        #[cfg(target_os = "linux")]
        {
            let mut cred: nix::libc::ucred = unsafe { std::mem::zeroed() };
            let mut len: nix::libc::socklen_t =
                std::mem::size_of::<nix::libc::ucred>() as nix::libc::socklen_t;
            let rc = unsafe {
                nix::libc::getsockopt(
                    fd,
                    nix::libc::SOL_SOCKET,
                    nix::libc::SO_PEERCRED,
                    &mut cred as *mut _ as *mut nix::libc::c_void,
                    &mut len,
                )
            };
            return rc == 0 && cred.uid == expected;
        }

        #[cfg(any(
            target_os = "macos",
            target_os = "freebsd",
            target_os = "dragonfly",
            target_os = "netbsd",
            target_os = "openbsd",
        ))]
        {
            let mut uid: nix::libc::uid_t = 0;
            let mut gid: nix::libc::gid_t = 0;
            let rc = unsafe { nix::libc::getpeereid(fd, &mut uid, &mut gid) };
            return rc == 0 && uid == expected;
        }

        // Other unices: fail closed. Don't forward to the VM.
        #[allow(unreachable_code)]
        {
            let _ = (fd, expected);
            false
        }
    }

    fingerprint
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

    let hyperthreading = args.hyperthreading;
    let nested_virt = args.nested_virt;
    let mut builder = VmBuilder::new()
        .machine(|m| {
            m.vcpus(args.vcpus as u8)
                .memory_mib(args.ram as usize)
                .hyperthreading(hyperthreading)
                .nested_virt(nested_virt)
        })
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
    // Per-mount DAX window. 0 (the CLI default) skips the call so
    // msb_krun's built-in default applies; any positive value is
    // converted to bytes and set on every virtiofs attach.
    let shm_bytes_per_mount: usize =
        (args.virtiofs_shm_size_mib as usize).saturating_mul(1024 * 1024);
    for entry in mount_plan.entries {
        let tag = entry.tag.clone();
        let host_path = entry.host_path.clone();
        builder = builder.fs(move |fs| {
            let mut fs = fs.tag(&tag);
            if shm_bytes_per_mount > 0 {
                fs = fs.shm_size(shm_bytes_per_mount);
            }
            fs.path(&host_path)
        });
    }

    // 2 workers when the shell relay is active (benefits from
    // parallel read/write task scheduling); 1 otherwise (network +
    // control proxy run fine on a single worker).
    let worker_threads = if args.shell_sock.is_some() { 2 } else { 1 };
    let tokio_rt = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(worker_threads)
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

    // When --control-sock is provided the host wires an `iii.control`
    // virtio-console port into the VM; telling iii-init about it via
    // III_CONTROL_PORT flips iii-init into supervisor mode so it serves
    // Restart/Shutdown/Ping/Status RPCs over that port. Without the env
    // var, iii-init falls back to its legacy single-spawn waitpid path.
    let control_port_env = args.control_sock.is_some();
    // Set up the shell-exec channel BEFORE constructing the exec
    // closure so `shell_port_env` reflects whether the host relay
    // actually started. Otherwise a failed `shell_relay::spawn` still
    // sets III_SHELL_PORT and the guest dispatcher spins on a port
    // nobody reads, silently breaking `iii worker exec` for the VM's
    // lifetime. Fail-closed: on relay spawn failure, skip the env var
    // and drop the attached fd so the VM boots without the port.
    let mut shell_port_env = false;
    let mut guest_shell_fd: Option<i32> = None;
    let mut shell_sock_fingerprint: Option<crate::cli::shell_relay::ShellSocketFingerprint> = None;
    if let Some(sock_path) = args.shell_sock.clone() {
        let (host_end, guest_fd) = setup_control_socketpair()?;
        match crate::cli::shell_relay::spawn(
            tokio_rt.handle(),
            std::path::PathBuf::from(sock_path),
            host_end,
        ) {
            Ok(fp) => {
                shell_sock_fingerprint = Some(fp);
                guest_shell_fd = Some(guest_fd);
                shell_port_env = true;
            }
            Err(e) => {
                eprintln!(
                    "warning: shell relay failed to start ({e}). \
                     `iii worker exec` disabled; VM boots normally."
                );
                // `host_end` was moved into `shell_relay::spawn` and is
                // dropped on its Err path. Close our guest half so we
                // don't leak the fd for the __vm-boot process lifetime.
                unsafe {
                    libc::close(guest_fd);
                }
            }
        }
    }
    let control_workdir = args.workdir.clone();
    let nofile_limit = args.nofile_limit;

    builder = builder.exec(|mut e| {
        e = e.path("/init.krun").workdir(&args.workdir);
        // Bound the open-file table for the guest worker from the host
        // builder. iii-init also calls setrlimit(RLIMIT_NOFILE) as its
        // first act, but KRUN_RLIMITS is applied by init.krun before
        // iii-init's Rust code runs — belt-and-suspenders for fds opened
        // during dynamic loader setup and early allocator state.
        // nofile_limit=0 means "let the guest rlimit::raise_nofile path
        // own it" and skips the call.
        if nofile_limit > 0 {
            e = e.rlimit("RLIMIT_NOFILE", nofile_limit, nofile_limit);
        }
        e = e.env("III_WORKER_CMD", &worker_cmd);
        e = e.env("III_INIT_DNS", &dns_nameserver);
        e = e.env("III_INIT_IP", &guest_ip);
        e = e.env("III_INIT_GW", &gateway_ip);
        e = e.env("III_INIT_CIDR", "30");
        e = e.env("III_WORKER_MEM_BYTES", &worker_heap_bytes.to_string());
        if control_port_env {
            e = e.env(
                "III_CONTROL_PORT",
                iii_supervisor::protocol::CONTROL_PORT_NAME,
            );
            e = e.env("III_WORKER_WORKDIR", &control_workdir);
        }
        if shell_port_env {
            e = e.env(
                "III_SHELL_PORT",
                iii_supervisor::shell_protocol::SHELL_PORT_NAME,
            );
        }
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

    // Configure the console device.
    //
    // `output(path)` sets the main console log file; `port("iii.control",
    // fd, fd)` adds a named virtio-console port backed by the guest-side
    // end of a socketpair. msb_krun routes both independently
    // (msb_krun builder.rs:367-391).
    //
    // When --control-sock is set, we:
    //   1. Create a socketpair — one end for the VM, one for the host.
    //   2. Spawn a proxy thread that listens on the unix socket and
    //      bridges bytes between connected clients and the host end.
    //   3. Hand the guest end to ConsoleBuilder::port.
    //
    // The proxy thread outlives this function — it runs until the
    // `__vm-boot` process exits (i.e. until the VM powers off), which
    // is exactly the lifetime of the control channel.
    let console_output_path = args.console_output.clone();
    let mut guest_control_fd: Option<i32> = None;
    let mut control_sock_fingerprint: Option<SocketFingerprint> = None;
    if let Some(sock_path) = args.control_sock.clone() {
        let (host_end, guest_fd) = setup_control_socketpair()?;
        control_sock_fingerprint = spawn_control_proxy(sock_path, host_end);
        guest_control_fd = Some(guest_fd);
    }

    // Shell-exec channel was already set up above (before the exec
    // closure was built) so `shell_port_env` correctly reflects whether
    // the relay actually spawned. `guest_shell_fd` is `Some` only when
    // spawn succeeded; on Err we dropped the guest fd to fail closed.

    builder = builder.console(move |mut c| {
        if let Some(path) = console_output_path {
            c = c.output(path);
        }
        if let Some(fd) = guest_control_fd {
            c = c.port(iii_supervisor::protocol::CONTROL_PORT_NAME, fd, fd);
        }
        if let Some(fd) = guest_shell_fd {
            c = c.port(iii_supervisor::shell_protocol::SHELL_PORT_NAME, fd, fd);
        }
        c
    });

    // Register VM-exit cleanup for pidfile and control socket
    // independently — they're separate resources and may be enabled
    // independently of each other (a future caller may want a control
    // socket without a persistent pidfile, or vice versa). Coupling
    // them inside `if let Some(pid_file)` would silently regress the
    // socket-cleanup path for such callers.
    //
    // libkrun's Builder::on_exit replaces the previous hook, so we
    // fold everything into a single closure that checks each resource
    // independently.
    let pid_path_for_exit = args.pid_file.clone();
    let sock_fingerprint_for_exit = control_sock_fingerprint.clone();
    let shell_fingerprint_for_exit = shell_sock_fingerprint.clone();
    if pid_path_for_exit.is_some()
        || sock_fingerprint_for_exit.is_some()
        || shell_fingerprint_for_exit.is_some()
    {
        builder = builder.on_exit(move |exit_code| {
            if let Some(ref p) = pid_path_for_exit {
                let _ = std::fs::remove_file(p);
            }
            // Only unlink the control socket if it's still the one we
            // bound — a fast `stop → start` can let a new VM rebind
            // the same path before this hook fires. The fingerprint
            // check makes stale unlinks a no-op.
            if let Some(ref fp) = sock_fingerprint_for_exit {
                fp.remove_if_unchanged();
            }
            // Same fingerprint-guarded cleanup for the shell socket.
            if let Some(ref fp) = shell_fingerprint_for_exit {
                fp.remove_if_unchanged();
            }
            if exit_code != 0 {
                eprintln!("  VM exited with code {}", exit_code);
            }
        });
    }

    let vm = builder
        .build()
        .map_err(|e| format!("VM build failed: {}", e))?;

    // Capture an exit handle BEFORE `enter()` moves `vm`. SIGTERM or
    // SIGINT delivered to this __vm-boot process now triggers the
    // VMM's clean shutdown path (event-loop reads the exit eventfd,
    // notifies on_exit observers, `_exit`s with the current exit
    // code) instead of the process being killed mid-guest-write.
    // Without this, `stop → SIGTERM` from the host leaves the guest
    // no chance to fsync, flush stdout logs, or drop pidfiles; the
    // on_exit closure installed above is what does that cleanup.
    //
    // Register the two signal streams SYNCHRONOUSLY before spawning
    // the awaiter task. Doing registration inside the spawned task
    // leaves a race window: SIGTERM arriving between `spawn` and the
    // task's first poll hits the default action (process kill).
    // Registering via `block_on` here closes that window — once the
    // block_on returns, the runtime's signal driver is already
    // listening; the spawned task just awaits the notification.
    let exit_handle_for_signals = vm.exit_handle();
    let signals_registered: Result<
        (tokio::signal::unix::Signal, tokio::signal::unix::Signal),
        std::io::Error,
    > = tokio_rt.block_on(async {
        use tokio::signal::unix::{SignalKind, signal};
        let sigterm = signal(SignalKind::terminate())?;
        let sigint = signal(SignalKind::interrupt())?;
        Ok((sigterm, sigint))
    });
    if let Ok((mut sigterm, mut sigint)) = signals_registered {
        tokio_rt.spawn(async move {
            tokio::select! {
                _ = sigterm.recv() => {}
                _ = sigint.recv() => {}
            }
            // `trigger` is async-signal-safe and idempotent per
            // msb_krun's ExitHandle docs — repeat fires after shutdown
            // has already begun are no-ops.
            exit_handle_for_signals.trigger();
        });
    } else {
        eprintln!(
            "  warning: failed to register SIGTERM/SIGINT handlers; \
             `stop` will fall back to abrupt SIGTERM kills."
        );
    }

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
