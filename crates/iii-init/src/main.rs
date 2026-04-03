#[cfg(target_os = "linux")]
mod error;
#[cfg(target_os = "linux")]
mod mount;
#[cfg(target_os = "linux")]
mod network;
#[cfg(target_os = "linux")]
mod rlimit;
#[cfg(target_os = "linux")]
mod supervisor;

#[cfg(target_os = "linux")]
use error::InitError;

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
fn run() -> Result<(), InitError> {
    mount::mount_filesystems()?;
    rlimit::raise_nofile()?;
    network::configure_network()?;
    if let Err(e) = network::write_resolv_conf() {
        eprintln!("iii-init: warning: {e} (DNS may use existing resolv.conf)");
    }
    supervisor::exec_worker()?;
    Ok(())
}
