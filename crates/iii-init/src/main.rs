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
    iii_init::mount::mount_filesystems()?;
    iii_init::mount::mount_virtiofs_shares();
    // Must run AFTER mount_filesystems (needs /sys/fs/cgroup for the
    // memory.swap.max side of setup_cgroup) and AFTER virtiofs (no
    // dependency but fine either way). Must run BEFORE exec_worker so
    // the cgroup has swap headroom before the worker child gets moved
    // into it.
    iii_init::mount::setup_swap();
    iii_init::rlimit::raise_nofile()?;
    iii_init::network::configure_network()?;
    if let Err(e) = iii_init::network::write_resolv_conf() {
        eprintln!("iii-init: warning: {e} (DNS may use existing resolv.conf)");
    }
    iii_init::supervisor::exec_worker()?;
    Ok(())
}
