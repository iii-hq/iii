// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

//! msb_krun links `-lcap-ng`, which needs the development symlink
//! (`libcap-ng.so`, from libcap-ng-dev; CI installs it). A machine that only
//! has the runtime library gets a symlink to it in OUT_DIR instead, so the
//! load test builds without installing anything.

use std::path::Path;

const DIRS: &[&str] = &[
    "/usr/lib/x86_64-linux-gnu",
    "/usr/lib/aarch64-linux-gnu",
    "/usr/lib64",
    "/usr/lib",
    "/lib64",
    "/lib",
];

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("linux") {
        return;
    }
    if DIRS
        .iter()
        .any(|d| Path::new(d).join("libcap-ng.so").exists())
    {
        return;
    }
    let Some(runtime) = DIRS
        .iter()
        .map(|d| Path::new(d).join("libcap-ng.so.0"))
        .find(|p| p.exists())
    else {
        return;
    };
    let out = std::path::PathBuf::from(std::env::var("OUT_DIR").unwrap()).join("cap-ng");
    std::fs::create_dir_all(&out).unwrap();
    let link = out.join("libcap-ng.so");
    let _ = std::fs::remove_file(&link);
    std::os::unix::fs::symlink(&runtime, &link).unwrap();
    println!("cargo:rustc-link-search=native={}", out.display());
}
