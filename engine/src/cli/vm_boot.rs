// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at support@motia.dev
// See LICENSE and PATENTS files for details.

//! Hidden `__vm-boot` subcommand — boots a libkrun microVM.
//!
//! Runs in a separate process (spawned via `current_exe() __vm-boot`)
//! for crash isolation. If libkrun segfaults, only this child dies.
//!
//! libkrun is loaded at runtime via dlopen (not linked at compile time)
//! so the iii binary works normally even when libkrun is not installed.

use std::ffi::{CString, c_void};
use std::os::raw::{c_char, c_int, c_uint};

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
    #[arg(long, default_value = "512")]
    pub ram: u32,

    /// Volume mounts (host_path:guest_path)
    #[arg(long)]
    pub mount: Vec<String>,

    /// Environment variables (KEY=VALUE)
    #[arg(long)]
    pub env: Vec<String>,
}

// ---- Runtime-loaded libkrun function pointers ----
// Loaded via dlopen so iii doesn't require libkrun at startup.

type KrunCreateCtx = unsafe extern "C" fn() -> c_int;
type KrunSetVmConfig = unsafe extern "C" fn(c_uint, c_uint, c_uint) -> c_int;
type KrunSetRoot = unsafe extern "C" fn(c_uint, *const c_char) -> c_int;
type KrunSetWorkdir = unsafe extern "C" fn(c_uint, *const c_char) -> c_int;
type KrunSetExec = unsafe extern "C" fn(c_uint, *const c_char, *const *const c_char, *const *const c_char) -> c_int;
type KrunAddVirtiofs = unsafe extern "C" fn(c_uint, *const c_char, *const c_char) -> c_int;
type KrunStartEnter = unsafe extern "C" fn(c_uint) -> c_int;
type KrunSetTsiScope = unsafe extern "C" fn(c_uint, *const c_char, *const c_char, u8) -> c_int;

struct Krun {
    create_ctx: KrunCreateCtx,
    set_vm_config: KrunSetVmConfig,
    set_root: KrunSetRoot,
    set_workdir: KrunSetWorkdir,
    set_exec: KrunSetExec,
    add_virtiofs: KrunAddVirtiofs,
    start_enter: KrunStartEnter,
    set_tsi_scope: KrunSetTsiScope,
    _handle: *mut c_void, // keep library loaded
}

unsafe extern "C" {
    fn dlopen(filename: *const c_char, flags: c_int) -> *mut c_void;
    fn dlsym(handle: *mut c_void, symbol: *const c_char) -> *mut c_void;
    fn dlerror() -> *const c_char;
}

const RTLD_NOW: c_int = 0x2;

impl Krun {
    fn load() -> Result<Self, String> {
        let lib_names: &[&str] = if cfg!(target_os = "macos") {
            &["libkrun.dylib", "libkrun.1.dylib"]
        } else {
            &["libkrun.so", "libkrun.so.1"]
        };

        let handle = unsafe {
            let mut h = std::ptr::null_mut();
            for name in lib_names {
                let cname = CString::new(*name).unwrap();
                h = dlopen(cname.as_ptr(), RTLD_NOW);
                if !h.is_null() {
                    break;
                }
            }
            if h.is_null() {
                let err = dlerror();
                let msg = if err.is_null() {
                    "unknown error".to_string()
                } else {
                    std::ffi::CStr::from_ptr(err).to_string_lossy().to_string()
                };
                return Err(format!("Failed to load libkrun: {}", msg));
            }
            h
        };

        unsafe fn sym<T>(handle: *mut c_void, name: &str) -> Result<T, String> {
            let cname = CString::new(name).unwrap();
            let ptr = unsafe { dlsym(handle, cname.as_ptr()) };
            if ptr.is_null() {
                return Err(format!("Symbol not found: {}", name));
            }
            Ok(unsafe { std::mem::transmute_copy(&ptr) })
        }

        unsafe {
            Ok(Krun {
                create_ctx: sym(handle, "krun_create_ctx")?,
                set_vm_config: sym(handle, "krun_set_vm_config")?,
                set_root: sym(handle, "krun_set_root")?,
                set_workdir: sym(handle, "krun_set_workdir")?,
                set_exec: sym(handle, "krun_set_exec")?,
                add_virtiofs: sym(handle, "krun_add_virtiofs")?,
                start_enter: sym(handle, "krun_start_enter")?,
                set_tsi_scope: sym(handle, "krun_set_tsi_scope")?,
                _handle: handle,
            })
        }
    }
}

/// Boot the VM. Called from `main()` when `__vm-boot` is parsed.
/// This function does NOT return — `krun_start_enter` replaces the process.
pub fn run(args: &VmBootArgs) -> ! {
    if !std::path::Path::new(&args.rootfs).exists() {
        eprintln!("error: rootfs path does not exist: {}", args.rootfs);
        std::process::exit(1);
    }

    if let Err(e) = boot_vm(args) {
        eprintln!("error: VM execution failed: {}", e);
        std::process::exit(1);
    }

    unreachable!()
}

fn boot_vm(args: &VmBootArgs) -> Result<(), String> {
    let krun = Krun::load()?;

    unsafe {
        let ctx_id = (krun.create_ctx)();
        if ctx_id < 0 {
            return Err(format!("krun_create_ctx() failed with code {}", ctx_id));
        }
        let ctx = ctx_id as u32;

        let ret = (krun.set_vm_config)(ctx, args.vcpus, args.ram);
        if ret < 0 {
            return Err(format!("krun_set_vm_config() failed with code {}", ret));
        }

        let rootfs = CString::new(args.rootfs.as_str()).map_err(|e| e.to_string())?;
        let ret = (krun.set_root)(ctx, rootfs.as_ptr());
        if ret < 0 {
            return Err(format!("krun_set_root() failed with code {}", ret));
        }

        let workdir = CString::new(args.workdir.as_str()).map_err(|e| e.to_string())?;
        let ret = (krun.set_workdir)(ctx, workdir.as_ptr());
        if ret < 0 {
            return Err(format!("krun_set_workdir() failed with code {}", ret));
        }

        for (i, mount_str) in args.mount.iter().enumerate() {
            let parts: Vec<&str> = mount_str.splitn(2, ':').collect();
            if parts.len() != 2 {
                return Err(format!(
                    "Invalid mount format '{}'. Expected host:guest",
                    mount_str
                ));
            }
            let tag = CString::new(format!("virtiofs_{}", i)).map_err(|e| e.to_string())?;
            let path = CString::new(parts[0]).map_err(|e| e.to_string())?;
            let ret = (krun.add_virtiofs)(ctx, tag.as_ptr(), path.as_ptr());
            if ret < 0 {
                return Err(format!(
                    "krun_add_virtiofs() failed for '{}': code {}",
                    mount_str, ret
                ));
            }
        }

        // Enable TSI networking (scope=3 = allow any IP)
        let ret = (krun.set_tsi_scope)(ctx, std::ptr::null(), std::ptr::null(), 3);
        if ret < 0 {
            eprintln!("  warning: krun_set_tsi_scope() returned {}, networking may be limited", ret);
        }

        let exec_cstr = CString::new(args.exec.as_str()).map_err(|e| e.to_string())?;
        let arg_cstrings: Vec<CString> = args
            .arg
            .iter()
            .map(|a| CString::new(a.as_str()).map_err(|e| e.to_string()))
            .collect::<Result<Vec<_>, _>>()?;
        let env_cstrings: Vec<CString> = args
            .env
            .iter()
            .map(|e| CString::new(e.as_str()).map_err(|er| er.to_string()))
            .collect::<Result<Vec<_>, _>>()?;

        // NOTE: Do NOT include exec_path as argv[0] — libkrun's init
        // automatically uses exec_path as argv[0].
        let mut argv_ptrs: Vec<*const i8> = Vec::new();
        for a in &arg_cstrings {
            argv_ptrs.push(a.as_ptr());
        }
        argv_ptrs.push(std::ptr::null());

        let mut envp_ptrs: Vec<*const i8> = Vec::new();
        for e in &env_cstrings {
            envp_ptrs.push(e.as_ptr());
        }
        envp_ptrs.push(std::ptr::null());

        let ret = (krun.set_exec)(
            ctx,
            exec_cstr.as_ptr(),
            argv_ptrs.as_ptr(),
            envp_ptrs.as_ptr(),
        );
        if ret < 0 {
            return Err(format!("krun_set_exec() failed with code {}", ret));
        }

        eprintln!(
            "  Booting VM (vcpus={}, ram={}MiB)...",
            args.vcpus, args.ram
        );
        let exit_code = (krun.start_enter)(ctx);

        if exit_code != 0 {
            eprintln!("  VM exited with code {}", exit_code);
        }

        std::process::exit(exit_code);
    }
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
            "--rootfs", "/tmp/rootfs",
            "--exec", "/usr/bin/python3",
            "--workdir", "/workspace",
            "--vcpus", "4",
            "--ram", "1024",
            "--env", "FOO=bar",
            "--arg", "script.py",
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
    fn test_vm_boot_args_defaults() {
        use clap::Parser;

        #[derive(Parser)]
        struct TestCli {
            #[command(flatten)]
            args: VmBootArgs,
        }

        let cli = TestCli::parse_from([
            "test",
            "--rootfs", "/tmp/rootfs",
            "--exec", "/usr/bin/node",
        ]);

        assert_eq!(cli.args.workdir, "/");
        assert_eq!(cli.args.vcpus, 2);
        assert_eq!(cli.args.ram, 512);
        assert!(cli.args.mount.is_empty());
        assert!(cli.args.env.is_empty());
        assert!(cli.args.arg.is_empty());
    }
}
