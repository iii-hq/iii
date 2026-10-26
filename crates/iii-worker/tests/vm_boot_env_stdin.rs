// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

//! `__vm-boot --env-stdin` as a real subprocess (MOT-5492). No KVM needed:
//! `run()` drains stdin before anything else, and a nonexistent rootfs makes
//! it exit right after.

use std::io::Write;
use std::process::{Command, Stdio};

fn vm_boot(stdin: &[u8]) -> (i32, String) {
    let mut child = Command::new(env!("CARGO_BIN_EXE_iii-worker"))
        .args([
            "__vm-boot",
            "--rootfs",
            "/nonexistent/mot-5493",
            "--exec",
            "/bin/true",
            "--env-stdin",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn iii-worker __vm-boot");
    // Blocks if the child stops reading before EOF; a hang here is the bug.
    child.stdin.take().unwrap().write_all(stdin).unwrap();
    let out = child.wait_with_output().unwrap();
    (
        out.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

#[test]
fn a_valid_payload_is_drained_before_the_rootfs_check() {
    let (code, stderr) = vm_boot(br#"[["API_KEY","sk-secret"],["GREETING","Ol\u00e1"]]"#);
    assert_eq!(code, 1, "{stderr}");
    assert!(stderr.contains("rootfs path does not exist"), "{stderr}");
    assert!(!stderr.contains("sk-secret"), "{stderr}");
}

#[test]
fn a_payload_larger_than_the_pipe_buffer_never_blocks_the_launcher() {
    let payload = format!(r#"[["BIG","{}"]]"#, "x".repeat(120 * 1024));
    let (code, stderr) = vm_boot(payload.as_bytes());
    assert_eq!(code, 1, "{stderr}");
    assert!(stderr.contains("rootfs path does not exist"), "{stderr}");
}

#[test]
fn an_empty_stdin_says_the_launcher_is_from_another_release() {
    let (code, stderr) = vm_boot(b"");
    assert_eq!(code, 1, "{stderr}");
    assert!(stderr.contains("same release"), "{stderr}");
}

#[test]
fn a_rejected_payload_names_the_key_and_never_echoes_the_value() {
    let (code, stderr) = vm_boot(br#"[["API_KEY","sk-secret\u0000tail"]]"#);
    assert_eq!(code, 1, "{stderr}");
    assert!(stderr.contains("API_KEY"), "{stderr}");
    assert!(!stderr.contains("sk-secret"), "{stderr}");
}
