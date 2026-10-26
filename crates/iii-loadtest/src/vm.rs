// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

//! `iii-loadtest vm`: boots a libkrun microVM with a fixed number of vCPUs
//! and a fixed amount of RAM, and runs one command in it.
//!
//! The VM has no network device: the guest has its own loopback and nothing
//! else, which is all the suite needs (engine and load generator run in the
//! same guest). `--tsi` lets libkrun turn every guest INET socket into a host
//! socket instead, so a port the guest listens on is a port on the host: handy
//! to reach a guest engine by hand, but every socket operation then goes
//! through the VMM, which costs several times what a guest-local one does.
//!
//! The guest's PID 1 is `iii-init` (`/init.krun` in the rootfs, see
//! `scripts/make-rootfs.sh`), the same init iii's managed workers boot: it
//! mounts the `--mount` shares and runs the command with `/bin/sh -c`.
//!
//! `enter()` hands the process over to the VMM and exits it when the guest
//! powers off, so the suite runs this as a child process.

use std::convert::Infallible;
use std::path::PathBuf;

use anyhow::{Context, Result, bail};

#[derive(clap::Args, Debug, Clone)]
pub struct VmArgs {
    /// vCPUs of the guest.
    #[arg(long, default_value_t = 2)]
    pub vcpus: u8,

    /// Guest RAM in MiB.
    #[arg(long, default_value_t = 1024)]
    pub ram: u32,

    /// Host directory served as the guest's `/` (virtio-fs). Must hold
    /// `/init.krun` and `/bin/sh`.
    #[arg(long)]
    pub rootfs: PathBuf,

    /// Extra virtio-fs share, `host_path:guest_path`. Repeatable.
    #[arg(long = "mount")]
    pub mounts: Vec<String>,

    /// Guest environment variable, `KEY=VALUE`. Repeatable.
    #[arg(long = "env")]
    pub env: Vec<String>,

    /// Cap the command's cgroup inside the guest (`memory.max`), in MiB.
    /// Without it the command may use all of the guest's RAM.
    #[arg(long)]
    pub cgroup_mem: Option<u64>,

    /// Write the guest console (kernel and init output) here.
    #[arg(long)]
    pub console: Option<PathBuf>,

    /// libkrunfw shared library; defaults to `~/.iii/lib/libkrunfw.so`.
    #[arg(long)]
    pub krunfw: Option<PathBuf>,

    /// Working directory of the command inside the guest.
    #[arg(long, default_value = "/")]
    pub workdir: String,

    /// Bridge the guest's sockets to the host (libkrun TSI): guest listening
    /// ports appear on the host, guest connections reach the host's.
    #[arg(long)]
    pub tsi: bool,

    /// Command run inside the guest.
    #[arg(last = true, required = true)]
    pub cmd: Vec<String>,
}

/// The `III_VIRTIOFS_MOUNTS` value iii-init reads (`tag=/guest;tag=/guest`)
/// and the shares to attach, tagged in the same order.
fn mount_plan(mounts: &[String]) -> Result<(Vec<(String, String)>, String)> {
    let mut shares = Vec::new();
    let mut env = Vec::new();
    for (i, mount) in mounts.iter().enumerate() {
        let Some((host, guest)) = mount
            .split_once(':')
            .filter(|(h, g)| !h.is_empty() && !g.is_empty())
        else {
            bail!("invalid --mount {mount:?}: expected host_path:guest_path");
        };
        let tag = format!("virtiofs_{i}");
        env.push(format!("{tag}={guest}"));
        shares.push((tag, host.to_string()));
    }
    Ok((shares, env.join(";")))
}

/// One shell word, quoted only when it has to be.
pub fn shell_quote(word: &str) -> String {
    if !word.is_empty()
        && word
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "-_./:=,@%+".contains(c))
    {
        word.to_string()
    } else {
        format!("'{}'", word.replace('\'', "'\\''"))
    }
}

fn default_krunfw() -> Option<PathBuf> {
    let home = std::env::var_os("HOME")?;
    let path = PathBuf::from(home).join(".iii/lib/libkrunfw.so");
    path.exists().then_some(path)
}

pub fn boot(args: VmArgs) -> Result<Infallible> {
    use msb_krun::VmBuilder;

    std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open("/dev/kvm")
        .context("/dev/kvm is not usable by this user")?;
    if !args.rootfs.join("init.krun").exists() {
        bail!(
            "{} has no init.krun: build it with crates/iii-loadtest/scripts/make-rootfs.sh",
            args.rootfs.display()
        );
    }
    let krunfw = args
        .krunfw
        .clone()
        .or_else(default_krunfw)
        .context("no libkrunfw: pass --krunfw or install iii (~/.iii/lib/libkrunfw.so)")?;

    // Every connection the guest makes is a host fd held by this process.
    raise_nofile();

    // iii's passthrough, not msb_krun's: msb_krun's answers every lookup of
    // `/init.krun` with an init of its own (an aarch64 one in 0.1.16), and
    // this one serves the iii-init that is in the rootfs.
    let root = iii_filesystem::PassthroughFs::builder()
        .root_dir(&args.rootfs)
        .build()
        .map_err(|e| anyhow::anyhow!("rootfs {}: {e}", args.rootfs.display()))?;
    let (shares, mounts_env) = mount_plan(&args.mounts)?;
    let command = args
        .cmd
        .iter()
        .map(|w| shell_quote(w))
        .collect::<Vec<_>>()
        .join(" ");

    let ram = boot_safe_ram(args.ram);
    if ram != args.ram {
        eprintln!(
            "iii-loadtest vm: {} MiB of RAM does not boot on x86_64 (libkrun); using {ram}",
            args.ram
        );
    }
    let mut builder = VmBuilder::new()
        .machine(|m| {
            m.vcpus(args.vcpus)
                .memory_mib(ram as usize)
                .enable_inet_hijack(args.tsi)
        })
        .kernel(|k| k.krunfw_path(&krunfw).init_path("/init.krun"))
        .fs(|fs| fs.tag("/dev/root").custom(Box::new(root)));
    for (tag, host) in shares {
        builder = builder.fs(move |fs| fs.tag(&tag).path(&host));
    }

    let env = args.env.clone();
    let cgroup_mem = args.cgroup_mem;
    let workdir = args.workdir.clone();
    builder = builder.exec(move |mut e| {
        e = e
            .path("/init.krun")
            .workdir(&workdir)
            .rlimit("RLIMIT_NOFILE", 1_048_576, 1_048_576)
            .env("III_WORKER_CMD", &command)
            .env("PATH", "/usr/local/bin:/usr/bin:/bin");
        if !mounts_env.is_empty() {
            e = e.env("III_VIRTIOFS_MOUNTS", &mounts_env);
        }
        if let Some(mib) = cgroup_mem {
            e = e.env("III_WORKER_MEM_BYTES", &(mib * 1024 * 1024).to_string());
        }
        for pair in &env {
            if let Some((key, value)) = pair.split_once('=') {
                e = e.env(key, value);
            }
        }
        e
    });
    if let Some(console) = args.console.clone() {
        builder = builder.console(move |c| c.output(console));
    }

    let vm = builder
        .build()
        .map_err(|e| anyhow::anyhow!("VM build failed: {e}"))?;

    // SIGTERM / SIGINT power the guest off through the VMM instead of
    // killing it mid-write, so the console and the shares are flushed. The
    // mask is set before any thread starts, so every thread inherits it and
    // only the `sigwait` one receives the signal.
    block_signals();
    let exit = vm.exit_handle();
    std::thread::spawn(move || {
        let mut set: libc::sigset_t = unsafe { std::mem::zeroed() };
        let mut sig: libc::c_int = 0;
        unsafe {
            libc::sigemptyset(&mut set);
            libc::sigaddset(&mut set, libc::SIGTERM);
            libc::sigaddset(&mut set, libc::SIGINT);
            libc::sigwait(&set, &mut sig);
        }
        exit.trigger();
    });

    vm.enter()
        .map_err(|e| anyhow::anyhow!("VM execution failed: {e}"))
}

/// Blocks SIGTERM / SIGINT on the calling thread and every thread it spawns
/// afterwards.
fn block_signals() {
    unsafe {
        let mut set: libc::sigset_t = std::mem::zeroed();
        libc::sigemptyset(&mut set);
        libc::sigaddset(&mut set, libc::SIGTERM);
        libc::sigaddset(&mut set, libc::SIGINT);
        libc::pthread_sigmask(libc::SIG_BLOCK, &set, std::ptr::null_mut());
    }
}

/// Where x86_64 guests' 32-bit MMIO gap starts (0xD000_0000), in MiB.
const MMIO_GAP_MIB: u32 = 3328;

/// With msb_krun 0.1.16 on x86_64, a guest whose RAM ends in the 32 MiB just
/// below the MMIO gap boots nothing and exits 0 without a word (3290 MiB
/// boots, 3310 and 3328 do not, 3329 does). Such a size is moved past the gap.
pub fn boot_safe_ram(mib: u32) -> u32 {
    if cfg!(target_arch = "x86_64") && (MMIO_GAP_MIB - 32..=MMIO_GAP_MIB).contains(&mib) {
        MMIO_GAP_MIB + 1
    } else {
        mib
    }
}

pub fn raise_nofile() {
    unsafe {
        let mut rlim: libc::rlimit = std::mem::zeroed();
        if libc::getrlimit(libc::RLIMIT_NOFILE, &mut rlim) == 0 && rlim.rlim_cur < rlim.rlim_max {
            rlim.rlim_cur = rlim.rlim_max;
            libc::setrlimit(libc::RLIMIT_NOFILE, &rlim);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mounts_are_tagged_in_order() {
        let (shares, env) =
            mount_plan(&["/a:/bench/bin".to_string(), "/b:/bench/run".to_string()]).unwrap();
        assert_eq!(
            shares,
            vec![
                ("virtiofs_0".to_string(), "/a".to_string()),
                ("virtiofs_1".to_string(), "/b".to_string())
            ]
        );
        assert_eq!(env, "virtiofs_0=/bench/bin;virtiofs_1=/bench/run");
        assert!(mount_plan(&["/a".to_string()]).is_err());
        assert!(mount_plan(&[":/a".to_string()]).is_err());
    }

    #[test]
    #[cfg(target_arch = "x86_64")]
    fn ram_that_ends_just_below_the_mmio_gap_is_moved_past_it() {
        assert_eq!(boot_safe_ram(3290), 3290);
        assert_eq!(boot_safe_ram(3310), 3329);
        assert_eq!(boot_safe_ram(3328), 3329);
        assert_eq!(boot_safe_ram(4096), 4096);
    }

    #[test]
    fn shell_words_are_quoted_only_when_needed() {
        assert_eq!(shell_quote("/bench/bin/iii"), "/bench/bin/iii");
        assert_eq!(shell_quote("a b"), "'a b'");
        assert_eq!(shell_quote("it's"), "'it'\\''s'");
        assert_eq!(shell_quote(""), "''");
    }
}
