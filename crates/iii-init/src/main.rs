mod error;
mod mount;
mod network;
mod rlimit;
mod supervisor;

use error::InitError;

fn main() {
    if let Err(e) = run() {
        eprintln!("iii-init: {e}");
        std::process::exit(1);
    }
}

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
