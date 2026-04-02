use std::path::PathBuf;

fn main() {
    println!("cargo::rustc-check-cfg=cfg(has_init_binary)");
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=../iii-init/src/main.rs");
    println!("cargo:rerun-if-changed=../iii-init/src/supervisor.rs");
    println!("cargo:rerun-if-changed=../iii-init/src/mount.rs");
    println!("cargo:rerun-if-changed=../iii-init/src/network.rs");
    println!("cargo:rerun-if-changed=../iii-init/src/rlimit.rs");
    println!("cargo:rerun-if-changed=../iii-init/src/error.rs");

    let out_dir = PathBuf::from(std::env::var("OUT_DIR").unwrap());
    let dest = out_dir.join("iii-init");

    if cfg!(feature = "embed-init") {
        let arch = std::env::var("CARGO_CFG_TARGET_ARCH").unwrap_or_default();
        // VMs always run Linux guests, so init is always a Linux musl binary
        // regardless of the host OS (macOS uses the same Linux guest arch).
        let triple = match arch.as_str() {
            "x86_64" => "x86_64-unknown-linux-musl",
            "aarch64" => "aarch64-unknown-linux-musl",
            _ => "",
        };

        let workspace_root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
        let binary_path = workspace_root
            .join("target")
            .join(triple)
            .join("release")
            .join("iii-init");

        if !triple.is_empty() && binary_path.is_file() {
            std::fs::copy(&binary_path, &dest).expect("failed to copy iii-init to OUT_DIR");
        } else {
            // Placeholder: single zero byte so include_bytes! never fails.
            std::fs::write(&dest, [0u8]).expect("failed to write iii-init placeholder");
        }
    } else {
        // Feature disabled: write a single zero byte as placeholder.
        std::fs::write(&dest, [0u8]).expect("failed to write iii-init placeholder");
    }

    // Tell rustc whether a real binary (>1 byte) was embedded.
    let meta = std::fs::metadata(&dest).expect("iii-init dest missing");
    if meta.len() > 1 {
        println!("cargo:rustc-cfg=has_init_binary");
    }
}
