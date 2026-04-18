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
/// When swap is attached (`III_SWAP_DEV` set), also sets
/// `memory.swap.max = memory.max` so memory-hungry workers (bun,
/// large JVMs) can page to real disk-backed swap instead of hitting
/// the cgroup's hard OOM-kill limit.
///
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
        // If the host attached a swap disk, let this cgroup consume
        // all available swap — bounded by whatever size the host
        // provisioned for the swap device. cgroup v2 defaults
        // `memory.swap.max` to 0, which prevents ANY swap usage and
        // re-OOM-kills memory-hungry runtimes (bun) at memory.max
        // even with a swap device attached. "max" means "as much as
        // the system has."
        if std::env::var("III_SWAP_DEV").is_ok() {
            if let Err(e) =
                std::fs::write("/sys/fs/cgroup/worker/memory.swap.max", "max")
            {
                eprintln!(
                    "iii-init: warning: failed to widen memory.swap.max: {e}"
                );
            }
        }
    }

    Ok(())
}

/// Format and enable a block-device-backed swap partition, if the host
/// attached one. Keyed off `III_SWAP_DEV` (e.g. `/dev/vda`) set by
/// `vm_boot.rs` when `--swap-path` was provided.
///
/// Idempotent: if the device already has a swap signature (previous
/// boot), skip `mkswap` and go straight to `swapon`. This avoids
/// wiping the signature on every restart, which matters because the
/// host's sparse swap file accumulates written pages across boots and
/// re-formatting would drop them (and make them no longer sparse — the
/// file stays the size it grew to).
///
/// Errors are warnings, not fatal: a bun worker with a broken swap
/// path still runs (it just OOM-kills later at the cgroup limit like
/// before). Node/Python workers don't need swap at all.
pub fn setup_swap() {
    let dev = match std::env::var("III_SWAP_DEV") {
        Ok(d) if !d.is_empty() => d,
        _ => return,
    };
    // Check for existing swap signature. `blkid` would be cleaner but
    // isn't in the minimal rootfs; a magic-byte probe at offset 0xff6
    // is what mkswap writes and what the kernel checks.
    let has_swap_sig = read_swap_signature(&dev);
    if !has_swap_sig {
        // `mkswap` is provided by util-linux, available in every OCI
        // base image we ship (node, python, rust, bun).
        let status = std::process::Command::new("mkswap")
            .arg(&dev)
            .status();
        match status {
            Ok(s) if s.success() => {}
            Ok(s) => {
                eprintln!("iii-init: warning: mkswap {dev} exit {s}; skipping swapon");
                return;
            }
            Err(e) => {
                eprintln!("iii-init: warning: mkswap {dev} failed: {e}; skipping swapon");
                return;
            }
        }
    }
    let status = std::process::Command::new("swapon").arg(&dev).status();
    match status {
        Ok(s) if s.success() => {}
        Ok(s) => eprintln!("iii-init: warning: swapon {dev} exit {s}"),
        Err(e) => eprintln!("iii-init: warning: swapon {dev} failed: {e}"),
    }
}

/// Rewrite `MemTotal`, `MemAvailable`, and `MemFree` lines in a
/// snapshot of `/proc/meminfo` so they report `mem_total_kb` instead
/// of the host/VM total. Every other line is copied verbatim.
///
/// Pure string transform — tested in isolation. Format matches what
/// the kernel writes: left-aligned label ending in colon, value
/// right-justified to column 15, trailing " kB".
pub fn rewrite_meminfo(src: &str, mem_total_kb: u64) -> String {
    let mut out = String::with_capacity(src.len());
    for line in src.lines() {
        if line.starts_with("MemTotal:") {
            out.push_str(&format!("MemTotal:       {:>8} kB", mem_total_kb));
        } else if line.starts_with("MemAvailable:") {
            out.push_str(&format!("MemAvailable:   {:>8} kB", mem_total_kb));
        } else if line.starts_with("MemFree:") {
            out.push_str(&format!("MemFree:        {:>8} kB", mem_total_kb));
        } else {
            out.push_str(line);
        }
        out.push('\n');
    }
    out
}

/// Bind-mount a rewritten `/proc/meminfo` over the real one so guest
/// runtimes that read MemTotal directly (notably Bun's Zig allocator,
/// which ignores cgroup v2 `memory.max`) see the per-worker cap
/// instead of the whole VM's RAM.
///
/// LXCFS-lite: a single snapshot written at boot, then bind-mounted.
/// Values don't update live the way LXCFS's FUSE does; for Bun that's
/// fine because it reads MemTotal once at startup.
///
/// No-op when `III_WORKER_MEM_BYTES` is unset, zero, or parse-fails —
/// the override is purely additive; uncapped VMs keep the real
/// /proc/meminfo. Errors at any step become warnings so a bad bind
/// can't wedge worker startup.
pub fn override_proc_meminfo() {
    let mem_bytes: u64 = match std::env::var("III_WORKER_MEM_BYTES")
        .ok()
        .and_then(|s| s.parse().ok())
    {
        Some(b) if b > 0 => b,
        _ => return,
    };
    let mem_kb = mem_bytes / 1024;

    let existing = match std::fs::read_to_string("/proc/meminfo") {
        Ok(s) => s,
        Err(e) => {
            eprintln!("iii-init: warning: read /proc/meminfo failed: {e}");
            return;
        }
    };
    let rewritten = rewrite_meminfo(&existing, mem_kb);

    // /run is tmpfs (mounted in step 8 above) so the faux file has no
    // on-disk footprint. Name is distinctive enough that a curious
    // operator can `cat /run/iii-meminfo` to confirm what the worker
    // is seeing.
    let faux_path = "/run/iii-meminfo";
    if let Err(e) = std::fs::write(faux_path, rewritten.as_bytes()) {
        eprintln!("iii-init: warning: write {faux_path} failed: {e}");
        return;
    }

    match mount(
        Some(faux_path),
        "/proc/meminfo",
        None::<&str>,
        MsFlags::MS_BIND,
        None::<&str>,
    ) {
        Ok(()) => {}
        Err(e) => {
            eprintln!("iii-init: warning: bind-mount {faux_path} over /proc/meminfo failed: {e}");
        }
    }
}

/// Read the 10-byte swap signature at offset 0xff6 to tell whether
/// `mkswap` has already been run on this device. The signature is
/// "SWAPSPACE2" for mkswap v2 (every currently-in-use format).
fn read_swap_signature(dev: &str) -> bool {
    use std::io::{Read, Seek, SeekFrom};
    let mut f = match std::fs::File::open(dev) {
        Ok(f) => f,
        Err(_) => return false,
    };
    if f.seek(SeekFrom::Start(0xff6)).is_err() {
        return false;
    }
    let mut buf = [0u8; 10];
    if f.read_exact(&mut buf).is_err() {
        return false;
    }
    &buf == b"SWAPSPACE2"
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
    fn rewrite_meminfo_caps_memtotal_memfree_memavailable() {
        // Realistic host snippet (abbreviated). Important: preserves
        // every non-capped line verbatim, and the cap value appears in
        // the three lines Bun/node care about.
        let src = "MemTotal:       16384000 kB\n\
                   MemFree:         8000000 kB\n\
                   MemAvailable:   12000000 kB\n\
                   Buffers:          100000 kB\n\
                   SwapTotal:       2097152 kB\n";
        let out = rewrite_meminfo(src, 524288);
        assert!(out.contains("MemTotal:"));
        assert!(out.contains("524288 kB"));
        assert!(out.contains("MemFree:"));
        assert!(out.contains("MemAvailable:"));
        // Untouched lines must survive verbatim.
        assert!(out.contains("Buffers:          100000 kB"));
        assert!(out.contains("SwapTotal:       2097152 kB"));
        // Exactly one MemTotal line after rewrite.
        assert_eq!(out.matches("MemTotal:").count(), 1);
    }

    #[test]
    fn rewrite_meminfo_preserves_line_count() {
        let src = "MemTotal:       16384000 kB\n\
                   MemFree:         8000000 kB\n\
                   MemAvailable:   12000000 kB\n\
                   Buffers:          100000 kB\n";
        let out = rewrite_meminfo(src, 1024);
        assert_eq!(out.lines().count(), 4);
    }

    #[test]
    fn rewrite_meminfo_empty_input_yields_empty_output() {
        assert_eq!(rewrite_meminfo("", 1024), "");
    }

    #[test]
    fn read_swap_signature_returns_false_for_missing_path() {
        assert!(!read_swap_signature("/nonexistent/path/does/not/exist"));
    }

    #[test]
    fn read_swap_signature_returns_false_for_empty_file() {
        let tmp = tempfile::NamedTempFile::new().unwrap();
        assert!(!read_swap_signature(tmp.path().to_str().unwrap()));
    }

    #[test]
    fn read_swap_signature_detects_mkswap_magic_at_offset() {
        // Reproduce the exact byte layout mkswap v2 writes: zeros,
        // then "SWAPSPACE2" at offset 0xff6. Any other byte pattern
        // at that offset must read as "not formatted."
        use std::io::{Seek, SeekFrom, Write};
        let mut tmp = tempfile::NamedTempFile::new().unwrap();
        tmp.as_file_mut().set_len(0x1000).unwrap();
        tmp.as_file_mut().seek(SeekFrom::Start(0xff6)).unwrap();
        tmp.as_file_mut().write_all(b"SWAPSPACE2").unwrap();
        tmp.as_file_mut().sync_all().unwrap();
        assert!(read_swap_signature(tmp.path().to_str().unwrap()));
    }

    #[test]
    fn read_swap_signature_rejects_wrong_magic() {
        // A sparse file with zeros at the signature offset is a fresh
        // swap.img the host just created — must NOT claim it's
        // already formatted, or setup_swap would skip mkswap and
        // swapon would fail on raw zeros.
        use std::io::{Seek, SeekFrom, Write};
        let mut tmp = tempfile::NamedTempFile::new().unwrap();
        tmp.as_file_mut().set_len(0x1000).unwrap();
        tmp.as_file_mut().seek(SeekFrom::Start(0xff6)).unwrap();
        tmp.as_file_mut().write_all(b"GARBAGE\0\0\0").unwrap();
        tmp.as_file_mut().sync_all().unwrap();
        assert!(!read_swap_signature(tmp.path().to_str().unwrap()));
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
