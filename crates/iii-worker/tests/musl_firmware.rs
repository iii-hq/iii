//! The release smoke test must exercise dlopen, not only `--version`.
//! A fully static musl binary starts but cannot load libkrunfw (MOT-4783).
#![cfg(all(target_os = "linux", target_env = "musl", feature = "embed-libkrunfw"))]

use std::ffi::{CStr, CString};

#[test]
fn musl_can_load_bundled_firmware() {
    #[cfg(target_arch = "x86_64")]
    let firmware = include_bytes!("../../../engine/firmware/libkrunfw-linux-x86_64.so");
    #[cfg(target_arch = "aarch64")]
    let firmware = include_bytes!("../../../engine/firmware/libkrunfw-linux-aarch64.so");

    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("libkrunfw.so");
    std::fs::write(&path, firmware).unwrap();
    let path = CString::new(path.as_os_str().as_encoded_bytes()).unwrap();

    // SAFETY: path is NUL-terminated and the checked-in firmware is a shared
    // library. The handle is closed only after its symbol has been inspected.
    unsafe {
        let handle = libc::dlopen(path.as_ptr(), libc::RTLD_NOW | libc::RTLD_LOCAL);
        if handle.is_null() {
            let error = libc::dlerror();
            let error = if error.is_null() {
                "unknown dlopen error".into()
            } else {
                CStr::from_ptr(error).to_string_lossy()
            };
            panic!("musl worker cannot load bundled firmware: {error}");
        }
        let kernel = libc::dlsym(handle, c"krunfw_get_kernel".as_ptr());
        libc::dlclose(handle);
        assert!(!kernel.is_null(), "firmware must export krunfw_get_kernel");
    }
}

#[test]
fn musl_can_unwind_without_libgcc_s() {
    assert!(std::panic::catch_unwind(|| panic!("expected unwind smoke test")).is_err());
}
