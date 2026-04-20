use std::os::unix::fs::symlink;
use std::path::Path;

use nix::mount::{MsFlags, mount};
use nix::sys::stat::Mode;
use nix::unistd::mkdir;

use crate::error::InitError;

/// Creates a directory, ignoring `EEXIST` errors (directory already exists).
fn mkdir_ignore_exists(path: &str) -> Result<(), InitError> {
    match mkdir(path, Mode::from_bits_truncate(0o755)) {
        Ok(()) | Err(nix::Error::EEXIST) => Ok(()),
        Err(e) => Err(InitError::Mkdir {
            path: path.into(),
            source: e,
        }),
    }
}

/// Mounts a filesystem, ignoring `EBUSY` errors (already mounted).
fn mount_ignore_busy(
    source: Option<&str>,
    target: &str,
    fstype: Option<&str>,
    flags: MsFlags,
    data: Option<&str>,
) -> Result<(), InitError> {
    match mount(source, target, fstype, flags, data) {
        Ok(()) | Err(nix::Error::EBUSY) => Ok(()),
        Err(e) => Err(InitError::Mount {
            target: target.into(),
            source: e,
        }),
    }
}

/// Mounts essential Linux filesystems in the correct order.
///
/// Mount sequence:
/// 1. `/dev` as devtmpfs (MS_RELATIME)
/// 2. `/proc` as proc (MS_NODEV | MS_NOEXEC | MS_NOSUID | MS_RELATIME)
/// 3. `/sys` as sysfs (MS_NODEV | MS_NOEXEC | MS_NOSUID | MS_RELATIME)
/// 4. `/dev/pts` as devpts (MS_NOEXEC | MS_NOSUID | MS_RELATIME)
/// 5. `/dev/shm` as tmpfs (MS_NOEXEC | MS_NOSUID | MS_RELATIME)
/// 6. `/dev/fd` symlink to `/proc/self/fd` (if not already present)
/// 7. `/tmp` as tmpfs (MS_NOSUID | MS_NODEV | MS_RELATIME, mode=1777)
/// 8. `/run` as tmpfs (MS_NOSUID | MS_NODEV | MS_RELATIME, mode=755)
pub fn mount_filesystems() -> Result<(), InitError> {
    let nodev_noexec_nosuid =
        MsFlags::MS_NODEV | MsFlags::MS_NOEXEC | MsFlags::MS_NOSUID | MsFlags::MS_RELATIME;
    let noexec_nosuid = MsFlags::MS_NOEXEC | MsFlags::MS_NOSUID | MsFlags::MS_RELATIME;

    // 1. /dev -- devtmpfs
    mkdir_ignore_exists("/dev")?;
    mount_ignore_busy(
        Some("devtmpfs"),
        "/dev",
        Some("devtmpfs"),
        MsFlags::MS_RELATIME,
        None::<&str>,
    )?;

    // 2. /proc -- proc
    mkdir_ignore_exists("/proc")?;
    mount_ignore_busy(
        Some("proc"),
        "/proc",
        Some("proc"),
        nodev_noexec_nosuid,
        None::<&str>,
    )?;

    // 3. /sys -- sysfs
    mkdir_ignore_exists("/sys")?;
    mount_ignore_busy(
        Some("sysfs"),
        "/sys",
        Some("sysfs"),
        nodev_noexec_nosuid,
        None::<&str>,
    )?;

    // 4. /dev/pts -- devpts
    mkdir_ignore_exists("/dev/pts")?;
    mount_ignore_busy(
        Some("devpts"),
        "/dev/pts",
        Some("devpts"),
        noexec_nosuid,
        None::<&str>,
    )?;

    // 5. /dev/shm -- tmpfs
    mkdir_ignore_exists("/dev/shm")?;
    mount_ignore_busy(
        Some("tmpfs"),
        "/dev/shm",
        Some("tmpfs"),
        noexec_nosuid,
        None::<&str>,
    )?;

    // 6. /dev/fd -> /proc/self/fd (INIT-05: must come after /proc mount)
    if !Path::new("/dev/fd").exists() {
        symlink("/proc/self/fd", "/dev/fd").map_err(|e| InitError::Symlink {
            path: "/dev/fd".into(),
            source: e,
        })?;
    }

    // 7. /tmp -- tmpfs (real kernel tmpfs so Unix domain sockets work;
    //    the rootfs passthrough filesystem does not implement mknod)
    mkdir_ignore_exists("/tmp")?;
    mount_ignore_busy(
        Some("tmpfs"),
        "/tmp",
        Some("tmpfs"),
        MsFlags::MS_NOSUID | MsFlags::MS_NODEV | MsFlags::MS_RELATIME,
        Some("mode=1777"),
    )?;

    // 8. /run -- tmpfs (runtime scratch space)
    mkdir_ignore_exists("/run")?;
    mount_ignore_busy(
        Some("tmpfs"),
        "/run",
        Some("tmpfs"),
        MsFlags::MS_NOSUID | MsFlags::MS_NODEV | MsFlags::MS_RELATIME,
        Some("mode=755"),
    )?;

    // 9. /sys/fs/cgroup -- cgroup2 (best-effort, used for worker memory limits)
    mount_cgroup2().ok();

    Ok(())
}

/// Recursively `mkdir -p` a guest path, ignoring `EEXIST` at each level.
fn mkdir_p(path: &str) -> Result<(), InitError> {
    let mut acc = String::new();
    for segment in path.trim_start_matches('/').split('/') {
        if segment.is_empty() {
            continue;
        }
        acc.push('/');
        acc.push_str(segment);
        mkdir_ignore_exists(&acc)?;
    }
    Ok(())
}

/// Mount virtiofs shares passed via the `III_VIRTIOFS_MOUNTS` env var.
///
/// Format: `tag1=/guest/path1;tag2=/guest/path2`. The tag matches the virtiofs
/// source tag attached in vm_boot. Each guest path is created with `mkdir -p`
/// before mounting. Failures on individual mounts log a warning and continue
/// so a bad share cannot wedge worker startup.
pub fn mount_virtiofs_shares() {
    let spec = match std::env::var("III_VIRTIOFS_MOUNTS") {
        Ok(s) if !s.is_empty() => s,
        _ => return,
    };

    let pairs = crate::parse::parse_virtiofs_spec(&spec, |entry| {
        eprintln!("iii-init: warning: malformed virtiofs mount entry: {entry}");
    });

    for (tag, guest_path) in pairs {
        if let Err(e) = mkdir_p(&guest_path) {
            eprintln!("iii-init: warning: mkdir {guest_path} failed: {e}");
            continue;
        }

        match mount(
            Some(tag.as_str()),
            guest_path.as_str(),
            Some("virtiofs"),
            MsFlags::empty(),
            None::<&str>,
        ) {
            Ok(()) | Err(nix::Error::EBUSY) => {}
            Err(e) => {
                eprintln!("iii-init: warning: mount virtiofs {tag} -> {guest_path} failed: {e}");
            }
        }
    }
}

/// Mount cgroup2 and create a memory-limited worker cgroup.
///
/// Reads `III_WORKER_MEM_BYTES` to set `memory.max` on the worker cgroup.
/// The supervisor moves the worker process into this cgroup after spawn.
/// Fails gracefully if the kernel lacks cgroup v2 or memory controller support.
fn mount_cgroup2() -> Result<(), InitError> {
    mkdir_ignore_exists("/sys/fs/cgroup")?;
    mount_ignore_busy(
        Some("cgroup2"),
        "/sys/fs/cgroup",
        Some("cgroup2"),
        MsFlags::MS_RELATIME,
        None::<&str>,
    )?;

    // Enable memory controller for child cgroups.
    std::fs::write("/sys/fs/cgroup/cgroup.subtree_control", "+memory").map_err(|e| {
        InitError::WriteFile {
            path: "/sys/fs/cgroup/cgroup.subtree_control".into(),
            source: e,
        }
    })?;

    // Create a child cgroup for the worker process.
    mkdir_ignore_exists("/sys/fs/cgroup/worker")?;

    // Set memory limit from env var (passed by vm_boot.rs).
    if let Ok(mem_bytes) = std::env::var("III_WORKER_MEM_BYTES") {
        let _ = std::fs::write("/sys/fs/cgroup/worker/memory.max", &mem_bytes);
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mkdir_ignore_exists_on_existing_dir() {
        // /tmp always exists -- should return Ok
        let result = mkdir_ignore_exists("/tmp");
        assert!(result.is_ok());
    }

    #[test]
    fn test_mount_filesystems_is_callable() {
        // Compile-time check that the function signature is correct.
        // Actual mount operations require root, so we just verify the
        // function exists and returns the expected Result type.
        let _: fn() -> Result<(), InitError> = mount_filesystems;
    }

    #[test]
    fn test_mount_order_devtmpfs_before_proc() {
        // Verify the source code has the correct ordering by checking
        // that devtmpfs appears in the source before proc mount.
        let source = include_str!("mount.rs");
        let devtmpfs_pos = source.find("\"devtmpfs\"").expect("devtmpfs not found");
        let proc_pos = source.find("\"proc\"").expect("proc not found");
        let sysfs_pos = source.find("\"sysfs\"").expect("sysfs not found");
        let devpts_pos = source.find("\"devpts\"").expect("devpts not found");

        assert!(
            devtmpfs_pos < proc_pos,
            "devtmpfs must be mounted before proc"
        );
        assert!(proc_pos < sysfs_pos, "proc must be mounted before sysfs");
        assert!(
            sysfs_pos < devpts_pos,
            "sysfs must be mounted before devpts"
        );
    }

    #[test]
    fn test_tmpfs_mounts_after_dev_fd() {
        // /tmp and /run tmpfs must come after /dev/fd symlink.
        let source = include_str!("mount.rs");
        let symlink_pos = source
            .find("// 6. /dev/fd -> /proc/self/fd")
            .expect("/dev/fd symlink comment not found");
        let tmp_pos = source
            .find("// 7. /tmp -- tmpfs")
            .expect("/tmp mount comment not found");
        let run_pos = source
            .find("// 8. /run -- tmpfs")
            .expect("/run mount comment not found");

        assert!(
            symlink_pos < tmp_pos,
            "/dev/fd symlink must precede /tmp mount"
        );
        assert!(tmp_pos < run_pos, "/tmp must be mounted before /run");
    }

    #[test]
    fn test_dev_fd_symlink_after_proc() {
        // The /dev/fd symlink targets /proc/self/fd, so it must come after /proc mount.
        let source = include_str!("mount.rs");
        let proc_mount_pos = source
            .find("// 2. /proc -- proc")
            .expect("/proc mount comment not found");
        let symlink_pos = source
            .find("// 6. /dev/fd -> /proc/self/fd")
            .expect("/dev/fd symlink comment not found");

        assert!(
            proc_mount_pos < symlink_pos,
            "/proc mount must precede /dev/fd symlink"
        );
    }
}
