// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

//! Worker environment channel: host `__vm-boot` -> guest `iii-init`.
//!
//! The worker environment used to reach the guest on the kernel command
//! line (msb_krun's `KRUN_ENV`). That line only carries printable ASCII,
//! wraps values in unescaped quotes and is size-capped, so one value with
//! an accent, an emoji or a newline panicked the VM boot (MOT-5493). The
//! environment now crosses a dedicated virtio-console port as a single
//! length-prefixed JSON frame: any UTF-8 value arrives byte-for-byte, and
//! nothing the worker is given lands in the guest's `/proc/cmdline`.
//!
//! Frame: a 4-byte big-endian payload length, then a JSON array of
//! `[key, value]` pairs in the order they are applied (later wins).
//!
//! Errors never include a value: values are often credentials.

use std::io::{self, Read, Write};

/// Virtio-console port name, as seen in `/sys/class/virtio-ports/*/name`.
pub const ENV_PORT_NAME: &str = "iii.env";

/// Kernel-cmdline variable that tells iii-init to read the env frame from
/// the named port before it starts the worker.
pub const ENV_PORT_ENV: &str = "III_ENV_PORT";

/// Largest payload accepted. A bigger length header is treated as a
/// corrupt frame rather than an allocation request.
pub const MAX_PAYLOAD_BYTES: usize = 8 * 1024 * 1024;

/// Longest `KEY=VALUE` string execve accepts (Linux `MAX_ARG_STRLEN`, NUL
/// included). A longer variable crosses the port fine, then makes the
/// worker spawn fail with E2BIG and a message that names nothing.
pub const MAX_VAR_BYTES: usize = 128 * 1024;

/// Total env budget, counted as execve does (`KEY=VALUE` plus NUL). execve
/// caps argv + envp at a quarter of the stack limit, about 2 MiB with the
/// usual 8 MiB stack; half of that leaves room for the worker's own argv
/// and whatever its launch script adds.
pub const MAX_ENV_BYTES: usize = 1024 * 1024;

/// Ordered `(key, value)` pairs.
pub type EnvPairs = Vec<(String, String)>;

/// Checks that `key=value` can be exported to a process environment.
/// Names the key on failure, never the value.
pub fn validate_pair(key: &str, value: &str) -> Result<(), String> {
    if key.is_empty() {
        return Err("an environment variable has an empty name".to_string());
    }
    if key.contains('=') {
        return Err(format!("environment variable name {key:?} contains '='"));
    }
    if key.contains('\0') {
        return Err(format!(
            "environment variable name {key:?} contains a NUL byte"
        ));
    }
    if value.contains('\0') {
        return Err(format!(
            "the value of environment variable {key} contains a NUL byte"
        ));
    }
    let size = key.len() + value.len() + 2;
    if size > MAX_VAR_BYTES {
        return Err(format!(
            "environment variable {key} is {size} bytes, over the {MAX_VAR_BYTES}-byte limit \
             for one variable of a process"
        ));
    }
    Ok(())
}

/// Validates every pair and the total size a process would receive.
pub fn validate_env(env: &[(String, String)]) -> Result<(), String> {
    for (key, value) in env {
        validate_pair(key, value)?;
    }
    // Count each name once, as the process ends up with it: `set_var` keeps
    // the last occurrence.
    let mut seen = std::collections::HashSet::new();
    let total: usize = env
        .iter()
        .rev()
        .filter(|(key, _)| seen.insert(key.as_str()))
        .map(|(k, v)| k.len() + v.len() + 2)
        .sum();
    if total > MAX_ENV_BYTES {
        return Err(format!(
            "the worker environment is {total} bytes, over the {MAX_ENV_BYTES}-byte limit \
             a process can reliably receive"
        ));
    }
    Ok(())
}

/// Serialises `env` as the JSON payload (no length prefix). This is also
/// the format `__vm-boot --env-stdin` reads to EOF.
pub fn encode_payload(env: &[(String, String)]) -> io::Result<Vec<u8>> {
    validate_env(env).map_err(invalid)?;
    let json = serde_json::to_vec(env).map_err(|e| invalid(format!("encode env: {e}")))?;
    if json.len() > MAX_PAYLOAD_BYTES {
        return Err(invalid(format!(
            "the worker environment is {} bytes, over the {MAX_PAYLOAD_BYTES}-byte limit",
            json.len()
        )));
    }
    Ok(json)
}

/// Parses a JSON payload produced by [`encode_payload`].
pub fn decode_payload(bytes: &[u8]) -> io::Result<EnvPairs> {
    if bytes.len() > MAX_PAYLOAD_BYTES {
        return Err(invalid(format!(
            "env payload is {} bytes, over the {MAX_PAYLOAD_BYTES}-byte limit",
            bytes.len()
        )));
    }
    // serde_json errors can quote the offending input; report only where.
    let pairs: EnvPairs = serde_json::from_slice(bytes).map_err(|e| {
        invalid(format!(
            "malformed env payload at line {} column {}",
            e.line(),
            e.column()
        ))
    })?;
    validate_env(&pairs).map_err(invalid)?;
    Ok(pairs)
}

/// Builds the length-prefixed frame sent over the port.
pub fn encode_frame(env: &[(String, String)]) -> io::Result<Vec<u8>> {
    let payload = encode_payload(env)?;
    let len = u32::try_from(payload.len()).map_err(|_| invalid("env payload too large"))?;
    let mut frame = Vec::with_capacity(4 + payload.len());
    frame.extend_from_slice(&len.to_be_bytes());
    frame.extend_from_slice(&payload);
    Ok(frame)
}

/// Writes one frame.
pub fn write_frame<W: Write>(writer: &mut W, env: &[(String, String)]) -> io::Result<()> {
    let frame = encode_frame(env)?;
    writer.write_all(&frame)?;
    writer.flush()
}

/// Reads one frame with `read_exact`. The guest uses its own patient reader
/// for the virtio port (transient EOFs, deadline) and calls
/// [`frame_len`] / [`decode_payload`] directly.
pub fn read_frame<R: Read>(reader: &mut R) -> io::Result<EnvPairs> {
    let mut header = [0u8; 4];
    reader.read_exact(&mut header)?;
    let len = frame_len(header)?;
    let mut payload = vec![0u8; len];
    reader.read_exact(&mut payload)?;
    decode_payload(&payload)
}

/// Validates a frame header and returns the payload length.
pub fn frame_len(header: [u8; 4]) -> io::Result<usize> {
    let len = u32::from_be_bytes(header) as usize;
    if len > MAX_PAYLOAD_BYTES {
        return Err(invalid(format!(
            "env frame announces {len} bytes, over the {MAX_PAYLOAD_BYTES}-byte limit"
        )));
    }
    Ok(len)
}

fn invalid(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.into())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pairs(items: &[(&str, &str)]) -> EnvPairs {
        items
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    #[test]
    fn values_the_kernel_cmdline_rejected_round_trip() {
        let env = pairs(&[
            ("GREETING", "Olá, São Paulo — ça va? 💖"),
            ("QUOTED", r#"say "hi" and 'bye'"#),
            ("PEM", "-----BEGIN KEY-----\nabc\tdef\n-----END KEY-----\n"),
            ("WITH_EQUALS", "a=b=c"),
            ("EMPTY", ""),
            ("SPACES", "  leading and trailing  "),
        ]);
        let frame = encode_frame(&env).unwrap();
        let decoded = read_frame(&mut frame.as_slice()).unwrap();
        assert_eq!(decoded, env);
    }

    #[test]
    fn order_and_duplicates_are_preserved() {
        let env = pairs(&[("A", "1"), ("B", "2"), ("A", "3")]);
        let decoded = read_frame(&mut encode_frame(&env).unwrap().as_slice()).unwrap();
        assert_eq!(decoded, env);
    }

    #[test]
    fn payload_round_trips_without_a_frame() {
        let env = pairs(&[("K", "válue\n")]);
        let payload = encode_payload(&env).unwrap();
        assert_eq!(decode_payload(&payload).unwrap(), env);
    }

    #[test]
    fn invalid_names_and_nul_values_are_rejected_naming_only_the_key() {
        for (key, value) in [("", "x"), ("A=B", "x"), ("A\0B", "x")] {
            assert!(encode_frame(&pairs(&[(key, value)])).is_err(), "{key:?}");
        }
        let err = encode_frame(&pairs(&[("API_KEY", "sk-secret\0tail")])).unwrap_err();
        let message = err.to_string();
        assert!(message.contains("API_KEY"), "{message}");
        assert!(!message.contains("sk-secret"), "{message}");
    }

    #[test]
    fn malformed_payload_error_does_not_echo_the_input() {
        let err = decode_payload(br#"[["API_KEY", "sk-secret"]"#).unwrap_err();
        assert!(!err.to_string().contains("sk-secret"), "{err}");
        let err = decode_payload(br#"{"API_KEY": "sk-secret"}"#).unwrap_err();
        assert!(!err.to_string().contains("sk-secret"), "{err}");
    }

    #[test]
    fn a_variable_execve_cannot_take_is_rejected_naming_the_key() {
        let value = "x".repeat(MAX_VAR_BYTES);
        let err = encode_payload(&pairs(&[("HUGE", &value)])).unwrap_err();
        assert!(err.to_string().contains("HUGE"), "{err}");
        let just_fits = "x".repeat(MAX_VAR_BYTES - "HUGE".len() - 2);
        assert!(encode_payload(&pairs(&[("HUGE", &just_fits)])).is_ok());
    }

    #[test]
    fn an_env_too_large_for_execve_is_rejected() {
        let value = "x".repeat(100 * 1024);
        let env: EnvPairs = (0..11).map(|i| (format!("V{i}"), value.clone())).collect();
        let err = encode_payload(&env).unwrap_err();
        assert!(err.to_string().contains("limit"), "{err}");
        let payload = serde_json::to_vec(&env).unwrap();
        assert!(decode_payload(&payload).is_err(), "decode enforces it too");
    }

    #[test]
    fn a_repeated_name_counts_once_toward_the_total() {
        let value = "x".repeat(120 * 1024);
        let env: EnvPairs = (0..9)
            .map(|_| ("SAME".to_string(), value.clone()))
            .collect();
        assert!(encode_payload(&env).is_ok());
    }

    #[test]
    fn decoded_pairs_are_validated() {
        let err = decode_payload(br#"[["A=B", "x"]]"#).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::InvalidData);
    }

    #[test]
    fn oversized_header_is_rejected_before_allocating() {
        let header = ((MAX_PAYLOAD_BYTES + 1) as u32).to_be_bytes();
        assert!(frame_len(header).is_err());
        assert!(read_frame(&mut header.as_slice()).is_err());
    }

    #[test]
    fn truncated_frame_is_an_unexpected_eof() {
        let frame = encode_frame(&pairs(&[("A", "1")])).unwrap();
        let err = read_frame(&mut &frame[..frame.len() - 1]).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::UnexpectedEof);
    }
}
