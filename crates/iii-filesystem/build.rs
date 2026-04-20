use std::path::PathBuf;

fn main() {
    println!("cargo::rustc-check-cfg=cfg(has_init_binary)");
    println!("cargo:rerun-if-changed=build.rs");

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

        // Track the cross-compiled binary itself rather than the iii-init
        // source tree. iii-init links iii-supervisor as a library, and a
        // source-tree allowlist silently misses edits to transitive deps
        // like iii-supervisor — which is exactly how the in-VM
        // process-group kill fix ended up shipped-but-not-embedded: the
        // linux-musl iii-init was rebuilt by `make sandbox`, but this
        // build.rs never re-ran, the OUT_DIR snapshot stayed frozen at
        // a pre-fix copy, and iii-worker embedded the stale bytes.
        //
        // `rerun-if-changed` accepts a path that doesn't exist yet —
        // cargo tracks the (non-)existence and reruns when the file
        // appears or its mtime moves. This is exactly the semantics we
        // want: "re-copy whenever the upstream binary changes, for any
        // reason, in any dep".
        println!("cargo:rerun-if-changed={}", binary_path.display());

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
