#[cfg(target_os = "linux")]
fn main() {
    if let Err(e) = run() {
        eprintln!("iii-init: {e}");
        std::process::exit(1);
    }
}

#[cfg(not(target_os = "linux"))]
fn main() {
    eprintln!(
        "iii-init: this binary is Linux guest-only; build with --target <arch>-unknown-linux-musl"
    );
    std::process::exit(1);
}

#[cfg(target_os = "linux")]
fn run() -> Result<(), iii_init::InitError> {
    // Pivot `/` off the libkrun virtiofs share onto a tmpfs before
    // any other mount work. The virtiofs root directory has a
    // readdir bug that OOM-kills `ls /` and similar listings; the
    // pivot replaces `/` with a well-behaved tmpfs and re-exposes
    // rootfs content via per-directory bind mounts. See root_pivot
    // module for the full rationale.
    // Block-image overlay mode (shared read-only base image + writable
    // upper) when the host signals it via III_BLOCK_ROOT_*; otherwise the
    // legacy per-worker virtiofs rootfs + tmpfs-pivot path.
    if iii_init::root_pivot::overlay_root_requested() {
        iii_init::root_pivot::overlay_root()?;
    } else {
        iii_init::root_pivot::pivot_to_tmpfs_root()?;
    }
    iii_init::mount::mount_filesystems()?;
    iii_init::mount::mount_virtiofs_shares();
    // Fakes `/proc/meminfo::MemTotal` to the per-worker cap so Bun's
    // Zig allocator — which reads MemTotal directly and ignores
    // cgroup v2 `memory.max` — sees the right budget. Must run AFTER
    // mount_filesystems (needs /run tmpfs for the faux file) and
    // BEFORE exec_worker (so the child reads the bind-mounted view).
    iii_init::mount::override_proc_meminfo();
    iii_init::rlimit::raise_nofile()?;
    iii_init::network::configure_network()?;
    if let Err(e) = iii_init::network::write_resolv_conf() {
        eprintln!("iii-init: warning: {e} (DNS may use existing resolv.conf)");
    }
    iii_init::supervisor::exec_worker()?;
    Ok(())
}
