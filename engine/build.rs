use std::path::PathBuf;

fn main() {
    println!("cargo::rustc-check-cfg=cfg(has_libkrunfw)");
    println!("cargo:rerun-if-changed=build.rs");

    let out_dir = PathBuf::from(std::env::var("OUT_DIR").unwrap());
    let engine_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));

    // --- libkrunfw embedding ---
    // The VMM (msb_krun) is compiled as a Rust dependency -- no external libkrun needed.
    // Only libkrunfw (guest kernel firmware) requires embedding.
    let libkrunfw_dest = out_dir.join("libkrunfw");
    if cfg!(feature = "embed-libkrunfw") {
        let os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
        let arch = std::env::var("CARGO_CFG_TARGET_ARCH").unwrap_or_default();
        let (target_os, ext) = if os == "macos" {
            ("darwin", "dylib")
        } else {
            ("linux", "so")
        };

        let fw_path = engine_dir
            .join("firmware")
            .join(format!("libkrunfw-{target_os}-{arch}.{ext}"));

        if fw_path.is_file() {
            std::fs::copy(&fw_path, &libkrunfw_dest)
                .expect("failed to copy libkrunfw from firmware/");
            println!("cargo:rerun-if-changed={}", fw_path.display());
        } else {
            std::fs::write(&libkrunfw_dest, [0u8])
                .expect("failed to write libkrunfw placeholder");
        }
    } else {
        std::fs::write(&libkrunfw_dest, [0u8]).expect("failed to write libkrunfw placeholder");
    }
    if std::fs::metadata(&libkrunfw_dest).map_or(false, |m| m.len() > 1) {
        println!("cargo:rustc-cfg=has_libkrunfw");
    }
}
