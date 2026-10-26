// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

//! `iii-loadtest rootfs`: the smallest guest root that runs host binaries.
//!
//! The engine and this tool are built for the host, so the guest gets the
//! host's own shared libraries: every binary is copied with what `ldd` says
//! it loads, at the same paths. Add `/bin/sh` (iii-init runs the command
//! through it) and iii-init itself as `/init.krun`, and nothing else: no
//! distribution image, no package manager, no services competing with the
//! engine for the VM.
//!
//! iii-init boots on a tmpfs and binds only the usual top-level directories
//! of the rootfs into it (`bin`, `etc`, `usr`, `opt`, …), so the binaries go
//! to `/opt/bench/bin`.

use std::collections::BTreeSet;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, bail};

#[derive(clap::Args, Debug, Clone)]
pub struct RootfsArgs {
    /// Directory to create (or refresh).
    #[arg(long)]
    pub out: PathBuf,

    /// iii-init, the guest's PID 1 (the static binary iii installs).
    #[arg(long)]
    pub init: Option<PathBuf>,

    /// Binary to copy into `/opt/bench/bin`, with its libraries. Repeatable.
    #[arg(long = "bin")]
    pub bins: Vec<PathBuf>,
}

/// Where the guest finds the binaries, relative to `/`.
pub const BIN_DIR: &str = "opt/bench/bin";

pub fn default_init() -> Option<PathBuf> {
    let home = std::env::var_os("HOME")?;
    let path = PathBuf::from(home).join(".local/bin/iii-init");
    path.exists().then_some(path)
}

pub fn build(args: &RootfsArgs) -> Result<()> {
    let out = &args.out;
    let init = args
        .init
        .clone()
        .or_else(default_init)
        .context("no iii-init: pass --init (it ships with iii as ~/.local/bin/iii-init)")?;

    for dir in [
        "bin", "dev", "etc", "proc", "run", "sys", "tmp", "root", BIN_DIR,
    ] {
        std::fs::create_dir_all(out.join(dir))?;
    }
    std::fs::set_permissions(out.join("tmp"), std::fs::Permissions::from_mode(0o1777))?;

    copy_file(&init, &out.join("init.krun"))?;

    let shell = std::fs::canonicalize("/bin/sh").context("resolve /bin/sh")?;
    copy_file(&shell, &out.join("bin/sh"))?;
    let mut libs = libraries(&shell)?;

    for bin in &args.bins {
        let name = bin
            .file_name()
            .with_context(|| format!("{} has no file name", bin.display()))?;
        copy_file(bin, &out.join(BIN_DIR).join(name))?;
        libs.extend(libraries(bin)?);
    }
    for lib in libs {
        let real = std::fs::canonicalize(&lib).with_context(|| format!("resolve {lib}"))?;
        copy_file(&real, &out.join(lib.trim_start_matches('/')))?;
    }

    std::fs::write(out.join("etc/passwd"), "root:x:0:0:root:/root:/bin/sh\n")?;
    std::fs::write(out.join("etc/group"), "root:x:0:\n")?;
    std::fs::write(out.join("etc/hosts"), "127.0.0.1 localhost\n")?;
    Ok(())
}

/// The absolute paths `ldd` lists for `bin`, the loader included.
fn libraries(bin: &Path) -> Result<BTreeSet<String>> {
    let output = Command::new("ldd")
        .arg(bin)
        .output()
        .context("run ldd (from the C library's tools)")?;
    let text = String::from_utf8_lossy(&output.stdout);
    if !output.status.success() {
        // A static binary: ldd says so and exits 1.
        if text.contains("not a dynamic executable") || text.contains("statically linked") {
            return Ok(BTreeSet::new());
        }
        bail!(
            "ldd {} failed: {}",
            bin.display(),
            String::from_utf8_lossy(&output.stderr)
        );
    }
    Ok(parse_ldd(&text))
}

fn parse_ldd(text: &str) -> BTreeSet<String> {
    let mut libs = BTreeSet::new();
    for line in text.lines() {
        let line = line.trim();
        if line.contains("not found") {
            continue;
        }
        let path = match line.split_once("=>") {
            Some((_, rest)) => rest.split_whitespace().next(),
            None => line.split_whitespace().next(),
        };
        if let Some(path) = path.filter(|p| p.starts_with('/')) {
            libs.insert(path.to_string());
        }
    }
    libs
}

fn copy_file(from: &Path, to: &Path) -> Result<()> {
    if let Some(parent) = to.parent() {
        std::fs::create_dir_all(parent)?;
    }
    // Replace rather than overwrite: a binary the guest still has open from
    // an earlier run keeps its old inode.
    let _ = std::fs::remove_file(to);
    std::fs::copy(from, to)
        .with_context(|| format!("copy {} to {}", from.display(), to.display()))?;
    let mode = std::fs::metadata(from)?.permissions().mode();
    std::fs::set_permissions(to, std::fs::Permissions::from_mode(mode | 0o500))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ldd_output_gives_every_absolute_path() {
        let text = "\tlinux-vdso.so.1 (0x00007ffd)\n\
            \tlibgcc_s.so.1 => /lib/x86_64-linux-gnu/libgcc_s.so.1 (0x00007f)\n\
            \tlibc.so.6 => /lib/x86_64-linux-gnu/libc.so.6 (0x00007f)\n\
            \t/lib64/ld-linux-x86-64.so.2 (0x00007f)\n\
            \tlibmissing.so => not found\n";
        let libs: Vec<String> = parse_ldd(text).into_iter().collect();
        assert_eq!(
            libs,
            vec![
                "/lib/x86_64-linux-gnu/libc.so.6",
                "/lib/x86_64-linux-gnu/libgcc_s.so.1",
                "/lib64/ld-linux-x86-64.so.2",
            ]
        );
    }
}
