// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

//! Shared HTTP download primitives used by both the binary-worker and
//! bundle-worker install paths.
//!
//! Both callers pull artifacts from CDN-fronted HTTPS endpoints, verify
//! a SHA-256 against the registry-provided expected digest, enforce a
//! caller-supplied size cap, and assert basic response sanity (status
//! code, optional content-type prefix, optional content-length pre-check
//! before any bytes stream). The differences between callers are
//! lifecycle, not protocol — binary workers buffer to memory before
//! verifying, bundle workers stream straight to disk while hashing.
//! This module exposes the streaming primitive that lets each lifecycle
//! make its own buffering choice without duplicating the wire-level
//! checks.

use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

/// Options controlling [`stream_response_to_path`].
///
/// `max_size` is enforced both pre-stream (against the `Content-Length`
/// header when present) and during the stream itself (so a server that
/// omits or lies about `Content-Length` cannot fool the cap).
///
/// `allowed_content_type_prefix`, when set, requires the response
/// `Content-Type` to start with that string. Use `Some("application/")`
/// for archive endpoints to reject mis-served HTML error pages cached
/// by an upstream CDN.
///
/// `url_for_logs` is the URL string callers want to appear in log
/// messages. Callers should pre-strip query parameters with
/// `sanitize_url_for_logs` before passing pre-signed CDN tokens here.
pub struct StreamOptions<'a> {
    pub max_size: u64,
    pub allowed_content_type_prefix: Option<&'a str>,
    pub url_for_logs: &'a str,
}

/// Outcome of a successful streaming download.
pub struct StreamOutcome {
    /// Total bytes written to the destination path.
    pub bytes_written: u64,
    /// Lowercase hex SHA-256 of the bytes streamed. Callers compare
    /// against the registry-supplied expected digest; this module does
    /// not compare for them so the same primitive can serve flows that
    /// don't have an expected digest at all (legacy binary downloads).
    pub sha256_hex: String,
}

/// Streams an already-issued HTTP response body to `dest` while
/// computing a SHA-256 and enforcing `opts.max_size`.
///
/// Caller is responsible for:
///   * issuing the request (so it can pick the right HTTP client —
///     redirect-allowed for the binary path, redirect-allowed-with-
///     ad-hoc-loop for OCI-style auth flows, etc.),
///   * checking the response status (we treat any successful status the
///     caller hands us as valid),
///   * comparing the returned SHA-256 against an expected digest,
///   * cleaning up the destination file on failure (this function
///     attempts a best-effort `remove_file` on error but a caller
///     holding a tempdir guard will always own final cleanup).
///
/// Pre-stream guards:
///   * Content-Type must start with `opts.allowed_content_type_prefix`
///     when that is set.
///   * Content-Length must not exceed `opts.max_size` when present.
///
/// In-stream guards:
///   * Each chunk read is counted; the cumulative byte count must not
///     exceed `opts.max_size`. Servers that lie about Content-Length
///     are caught here.
pub async fn stream_response_to_path(
    response: reqwest::Response,
    dest: &Path,
    opts: StreamOptions<'_>,
) -> Result<StreamOutcome, DownloadError> {
    use futures::StreamExt as _;
    use tokio::io::AsyncWriteExt as _;

    if let Some(prefix) = opts.allowed_content_type_prefix {
        if let Some(ct) = response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
        {
            if !ct.to_ascii_lowercase().starts_with(prefix) {
                return Err(DownloadError::UnexpectedContentType {
                    url: opts.url_for_logs.to_string(),
                    content_type: ct.to_string(),
                    expected_prefix: prefix.to_string(),
                });
            }
        }
    }

    if let Some(len) = response.content_length()
        && len > opts.max_size
    {
        return Err(DownloadError::TooLarge {
            url: opts.url_for_logs.to_string(),
            announced: Some(len),
            limit: opts.max_size,
        });
    }

    let mut file = tokio::fs::File::create(dest)
        .await
        .map_err(|source| DownloadError::Io {
            path: dest.to_path_buf(),
            source,
        })?;

    let mut hasher = Sha256::new();
    let mut total: u64 = 0;
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let bytes = chunk.map_err(|e| DownloadError::Network {
            url: opts.url_for_logs.to_string(),
            message: format!("read failed: {e}"),
        })?;
        total = total.saturating_add(bytes.len() as u64);
        if total > opts.max_size {
            // Don't leave a half-archive behind.
            drop(file);
            let _ = std::fs::remove_file(dest);
            return Err(DownloadError::TooLarge {
                url: opts.url_for_logs.to_string(),
                announced: None,
                limit: opts.max_size,
            });
        }
        hasher.update(&bytes);
        file.write_all(&bytes)
            .await
            .map_err(|source| DownloadError::Io {
                path: dest.to_path_buf(),
                source,
            })?;
    }
    file.flush().await.map_err(|source| DownloadError::Io {
        path: dest.to_path_buf(),
        source,
    })?;
    drop(file);

    Ok(StreamOutcome {
        bytes_written: total,
        sha256_hex: format!("{:x}", hasher.finalize()),
    })
}

/// Strips both the query string AND any userinfo from a URL for safe
/// logging.
///
/// Pre-signed CDN URLs carry their auth token in the query string
/// (S3-style `X-Amz-Signature=...`). Some hostile or misconfigured
/// registries hand back URLs with embedded credentials
/// (`https://user:password@host/path`). Either form turns a leaky log
/// into a download credential — strip both.
///
/// Falls back to the substring before the first `?` when the input
/// isn't parseable as a URL, so it never panics on bad input.
pub fn sanitize_url_for_logs(input: &str) -> String {
    match reqwest::Url::parse(input) {
        Ok(mut u) => {
            u.set_query(None);
            // set_username / set_password return Result<(), ()> for
            // schemes that don't permit userinfo (data:, file:, ...).
            // Ignore failures — they mean userinfo wasn't there to
            // strip.
            let _ = u.set_username("");
            let _ = u.set_password(None);
            u.to_string()
        }
        Err(_) => input.split('?').next().unwrap_or(input).to_string(),
    }
}

/// Validates an artifact URL against the basic SSRF guard policy
/// before any network I/O is issued:
///
/// 1. Scheme must be exactly `https`. Plain HTTP, `file:`, `ftp:`,
///    `gopher:`, and anything else are rejected. A pre-signed CDN URL
///    cannot legitimately use a non-HTTPS scheme.
/// 2. Host must be present and must not be a reserved hostname
///    (`localhost`, `localhost.localdomain`, `ip6-localhost`,
///    `ip6-loopback`).
/// 3. When the host is a literal IP, it must be a globally-routable
///    address. Loopback (`127.0.0.0/8`, `::1`), link-local
///    (`169.254.0.0/16` — covers AWS/GCE IMDS — and `fe80::/10`),
///    unspecified (`0.0.0.0`, `::`), multicast, broadcast, documentation
///    (`192.0.2.0/24`, `198.51.100.0/24`, `203.0.113.0/24`,
///    `2001:db8::/32`), and RFC-1918 private (`10/8`, `172.16/12`,
///    `192.168/16`) ranges are rejected.
///
/// Hostname-based hosts are NOT resolved here: that would add a DNS
/// round-trip to every install and the caller already trusts the
/// registry's TLS chain. A hostile registry that publishes
/// `https://internal.corp/...` still gets a TCP connect attempt — but
/// the cap is to refuse the obviously-internal IP literals and reserved
/// names a non-malicious registry would never use.
pub fn validate_archive_url_for_ssrf(url_str: &str) -> Result<(), SsrfError> {
    let url = reqwest::Url::parse(url_str).map_err(|e| SsrfError::Malformed {
        url: url_str.to_string(),
        reason: format!("{e}"),
    })?;

    if url.scheme() != "https" {
        return Err(SsrfError::DisallowedScheme {
            url: sanitize_url_for_logs(url_str),
            scheme: url.scheme().to_string(),
        });
    }

    let host = url.host_str().ok_or_else(|| SsrfError::Malformed {
        url: sanitize_url_for_logs(url_str),
        reason: "URL has no host component".to_string(),
    })?;

    // Reserved hostnames a CDN URL would never legitimately use.
    let host_lower = host.to_ascii_lowercase();
    if matches!(
        host_lower.as_str(),
        "localhost" | "localhost.localdomain" | "ip6-localhost" | "ip6-loopback"
    ) {
        return Err(SsrfError::DisallowedHost {
            url: sanitize_url_for_logs(url_str),
            host: host.to_string(),
            reason: "reserved hostname".to_string(),
        });
    }

    // If the host parses as a literal IP (IPv4 dotted-quad or IPv6
    // bracketed/bare), enforce the non-routable check. Hostname-only
    // hosts (e.g. `cdn.example.com`) skip this check — DNS resolution
    // would be a separate, slower defense layer outside v1 scope.
    if let Some(ip) = parse_host_as_ip(host)
        && let Some(reason) = ip_is_non_routable(ip)
    {
        return Err(SsrfError::DisallowedHost {
            url: sanitize_url_for_logs(url_str),
            host: ip.to_string(),
            reason: reason.to_string(),
        });
    }

    Ok(())
}

/// Returns `Some(reason)` if the literal IP is in a non-routable range
/// (loopback, link-local, private, etc.) and must be refused for
/// artifact downloads. `None` means the IP is globally routable as far
/// as we can tell from a literal inspection.
///
/// Uses the std::net unstable helpers via stable equivalents so we can
/// compile on stable. The conditions mirror RFC 6890 / 3927 / 1918 /
/// 4291 / 5737 / 3849.
fn ip_is_non_routable(ip: std::net::IpAddr) -> Option<&'static str> {
    use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
    match ip {
        IpAddr::V4(v4) => {
            if v4.is_loopback() {
                return Some("loopback IPv4 address");
            }
            if v4.is_link_local() {
                return Some("link-local IPv4 address (e.g. AWS/GCE IMDS 169.254.169.254)");
            }
            if v4.is_unspecified() {
                return Some("unspecified IPv4 address (0.0.0.0)");
            }
            if v4.is_broadcast() {
                return Some("broadcast IPv4 address");
            }
            if v4.is_multicast() {
                return Some("multicast IPv4 address");
            }
            if v4.is_private() {
                return Some("RFC-1918 private IPv4 address");
            }
            // RFC-6598 carrier-grade NAT: 100.64.0.0/10.
            let octets = v4.octets();
            if octets[0] == 100 && (64..=127).contains(&octets[1]) {
                return Some("RFC-6598 carrier-grade NAT IPv4 address");
            }
            // RFC-5737 documentation ranges.
            if v4.octets()[..3] == [192, 0, 2]
                || v4.octets()[..3] == [198, 51, 100]
                || v4.octets()[..3] == [203, 0, 113]
            {
                return Some("RFC-5737 documentation IPv4 address");
            }
            // 0.0.0.0/8 — current network.
            if v4.octets()[0] == 0 {
                return Some("0.0.0.0/8 reserved IPv4 range");
            }
            // 127/8 already handled by is_loopback.
            // 255.255.255.255 already by is_broadcast.
            None
        }
        IpAddr::V6(v6) => {
            if v6.is_loopback() {
                return Some("loopback IPv6 address (::1)");
            }
            if v6.is_unspecified() {
                return Some("unspecified IPv6 address (::)");
            }
            if v6.is_multicast() {
                return Some("multicast IPv6 address");
            }
            let segments = v6.segments();
            // fe80::/10 — link-local.
            if segments[0] & 0xffc0 == 0xfe80 {
                return Some("link-local IPv6 address (fe80::/10)");
            }
            // fc00::/7 — unique local (RFC 4193).
            if segments[0] & 0xfe00 == 0xfc00 {
                return Some("unique-local IPv6 address (fc00::/7)");
            }
            // 2001:db8::/32 — documentation.
            if segments[0] == 0x2001 && segments[1] == 0x0db8 {
                return Some("RFC-3849 documentation IPv6 address");
            }
            // IPv4-mapped IPv6 (::ffff:0:0/96): recurse on the embedded v4.
            if let Some(mapped) = v6.to_ipv4_mapped() {
                let _ = Ipv4Addr::from(mapped); // type-check
                return ip_is_non_routable(IpAddr::V4(mapped));
            }
            // IPv4-compatible (::a.b.c.d/96, deprecated) and IPv4-in-IPv6.
            if segments[0..5] == [0; 5]
                && segments[5] == 0
                && let Some(v4) = ipv4_in_ipv6_compat(&v6)
            {
                return ip_is_non_routable(IpAddr::V4(v4));
            }
            // IPv4-translated (::ffff:0:0:0/96 NAT64 SIIT) handled by to_ipv4_mapped.
            // Catch-all: ::/128 already by unspecified; 100::/64 discard prefix.
            if segments[0] == 0x0100 && segments[1..4] == [0, 0, 0] {
                return Some("RFC-6666 IPv6 discard prefix (100::/64)");
            }
            // 2002::/16 6to4 — allow (publicly routable).
            let _ = Ipv6Addr::from(segments);
            None
        }
    }
}

/// Parses a URL host string as an `IpAddr`. Handles both plain IPv4
/// (`"127.0.0.1"`) and bracketed IPv6 (`"[::1]"`) — the latter is how
/// `reqwest::Url::host_str()` returns IPv6 hosts to keep the form
/// roundtrip-safe.
fn parse_host_as_ip(host: &str) -> Option<std::net::IpAddr> {
    use std::net::IpAddr;
    let trimmed = if let Some(s) = host.strip_prefix('[')
        && let Some(s) = s.strip_suffix(']')
    {
        s
    } else {
        host
    };
    trimmed.parse::<IpAddr>().ok()
}

fn ipv4_in_ipv6_compat(v6: &std::net::Ipv6Addr) -> Option<std::net::Ipv4Addr> {
    let s = v6.segments();
    // ::a.b.c.d
    if s[0..6] == [0, 0, 0, 0, 0, 0] && (s[6] != 0 || s[7] != 0) {
        let a = (s[6] >> 8) as u8;
        let b = (s[6] & 0xff) as u8;
        let c = (s[7] >> 8) as u8;
        let d = (s[7] & 0xff) as u8;
        Some(std::net::Ipv4Addr::new(a, b, c, d))
    } else {
        None
    }
}

/// Errors raised by `validate_archive_url_for_ssrf` before any wire I/O.
#[derive(Debug)]
pub enum SsrfError {
    Malformed {
        url: String,
        reason: String,
    },
    DisallowedScheme {
        url: String,
        scheme: String,
    },
    DisallowedHost {
        url: String,
        host: String,
        reason: String,
    },
}

impl std::fmt::Display for SsrfError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Malformed { url, reason } => {
                write!(f, "malformed archive URL {url:?}: {reason}")
            }
            Self::DisallowedScheme { url, scheme } => write!(
                f,
                "archive URL {url:?} uses disallowed scheme {scheme:?}; only https is permitted"
            ),
            Self::DisallowedHost { url, host, reason } => write!(
                f,
                "archive URL {url:?} refers to host {host:?} ({reason}); refusing to fetch"
            ),
        }
    }
}

impl std::error::Error for SsrfError {}

/// Wire-level download errors. Per-feature install paths translate
/// these into their preferred `WorkerOpError` shapes (binary keeps
/// stringly-typed for backwards compat with existing tests, bundle maps
/// to `WorkerOpError::Download`).
#[derive(Debug)]
pub enum DownloadError {
    Network {
        url: String,
        message: String,
    },
    UnexpectedContentType {
        url: String,
        content_type: String,
        expected_prefix: String,
    },
    TooLarge {
        url: String,
        announced: Option<u64>,
        limit: u64,
    },
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
}

impl std::fmt::Display for DownloadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Network { url, message } => {
                write!(f, "network error for {url:?}: {message}")
            }
            Self::UnexpectedContentType {
                url,
                content_type,
                expected_prefix,
            } => write!(
                f,
                "{url:?} returned content-type {content_type:?}; expected one starting with {expected_prefix:?}"
            ),
            Self::TooLarge {
                url,
                announced,
                limit,
            } => match announced {
                Some(n) => write!(
                    f,
                    "{url:?} content-length {:.1} MiB exceeds limit {:.1} MiB",
                    *n as f64 / 1_048_576.0,
                    *limit as f64 / 1_048_576.0,
                ),
                None => write!(
                    f,
                    "{url:?} body exceeded {:.1} MiB during stream",
                    *limit as f64 / 1_048_576.0,
                ),
            },
            Self::Io { path, source } => {
                write!(f, "I/O error at {}: {source}", path.display())
            }
        }
    }
}

impl std::error::Error for DownloadError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitize_url_strips_query() {
        assert_eq!(
            sanitize_url_for_logs("https://cdn.example.com/foo.tar.gz?token=secret"),
            "https://cdn.example.com/foo.tar.gz"
        );
    }

    #[test]
    fn sanitize_url_falls_back_on_bad_input() {
        assert_eq!(sanitize_url_for_logs("not-a-url?token=secret"), "not-a-url");
    }

    #[test]
    fn sanitize_url_preserves_path_and_port() {
        assert_eq!(
            sanitize_url_for_logs("https://cdn.example.com:8443/v1/foo/bar.tar.gz?sig=x"),
            "https://cdn.example.com:8443/v1/foo/bar.tar.gz"
        );
    }

    #[test]
    fn download_error_display_includes_url() {
        let e = DownloadError::Network {
            url: "https://x".into(),
            message: "boom".into(),
        };
        assert!(format!("{e}").contains("https://x"));
        assert!(format!("{e}").contains("boom"));
    }

    #[test]
    fn download_error_too_large_announced_renders_megabytes() {
        let e = DownloadError::TooLarge {
            url: "https://x".into(),
            announced: Some(128 * 1024 * 1024),
            limit: 64 * 1024 * 1024,
        };
        let s = format!("{e}");
        assert!(s.contains("128"), "expected announced MiB in: {s}");
        assert!(s.contains("64"), "expected limit MiB in: {s}");
    }

    // -------------------- SSRF guard --------------------

    #[test]
    fn ssrf_accepts_public_https() {
        validate_archive_url_for_ssrf("https://cdn.example.com/v1/foo.tar.gz?sig=x")
            .expect("public https accepted");
        validate_archive_url_for_ssrf("https://8.8.8.8/foo.tar.gz").expect("public IPv4 accepted");
        validate_archive_url_for_ssrf("https://[2606:4700:4700::1111]/foo.tar.gz")
            .expect("public IPv6 accepted");
    }

    #[test]
    fn ssrf_rejects_non_https_scheme() {
        let err = validate_archive_url_for_ssrf("http://cdn.example.com/foo.tar.gz")
            .expect_err("plain http rejected");
        assert!(matches!(err, SsrfError::DisallowedScheme { .. }));
        let err =
            validate_archive_url_for_ssrf("file:///etc/passwd").expect_err("file scheme rejected");
        assert!(matches!(err, SsrfError::DisallowedScheme { .. }));
        let err = validate_archive_url_for_ssrf("ftp://ftp.example.com/foo.tar.gz")
            .expect_err("ftp scheme rejected");
        assert!(matches!(err, SsrfError::DisallowedScheme { .. }));
    }

    #[test]
    fn ssrf_rejects_reserved_hostnames() {
        let err = validate_archive_url_for_ssrf("https://localhost/foo.tar.gz")
            .expect_err("localhost rejected");
        assert!(matches!(err, SsrfError::DisallowedHost { .. }));
        let err = validate_archive_url_for_ssrf("https://LocalHost.LocalDomain/foo.tar.gz")
            .expect_err("localhost.localdomain rejected case-insensitive");
        assert!(matches!(err, SsrfError::DisallowedHost { .. }));
    }

    #[test]
    fn ssrf_rejects_loopback_ipv4() {
        let err = validate_archive_url_for_ssrf("https://127.0.0.1/foo.tar.gz")
            .expect_err("loopback rejected");
        assert!(matches!(err, SsrfError::DisallowedHost { .. }));
        let err = validate_archive_url_for_ssrf("https://127.42.42.42/foo.tar.gz")
            .expect_err("127/8 rejected");
        assert!(matches!(err, SsrfError::DisallowedHost { .. }));
    }

    #[test]
    fn ssrf_rejects_link_local_ipv4_imds() {
        let err = validate_archive_url_for_ssrf("https://169.254.169.254/latest/meta-data/")
            .expect_err("IMDS rejected");
        assert!(matches!(err, SsrfError::DisallowedHost { .. }));
    }

    #[test]
    fn ssrf_rejects_rfc1918_private_ipv4() {
        for url in [
            "https://10.0.0.1/foo.tar.gz",
            "https://172.16.0.1/foo.tar.gz",
            "https://192.168.1.1/foo.tar.gz",
        ] {
            let err = validate_archive_url_for_ssrf(url)
                .expect_err(&format!("rfc1918 not rejected for {url}"));
            assert!(matches!(err, SsrfError::DisallowedHost { .. }));
        }
    }

    #[test]
    fn ssrf_rejects_rfc6598_cgnat_ipv4() {
        let err = validate_archive_url_for_ssrf("https://100.64.0.1/foo.tar.gz")
            .expect_err("CGNAT rejected");
        assert!(matches!(err, SsrfError::DisallowedHost { .. }));
    }

    #[test]
    fn ssrf_rejects_unspecified_and_broadcast() {
        let err = validate_archive_url_for_ssrf("https://0.0.0.0/foo.tar.gz")
            .expect_err("0.0.0.0 rejected");
        assert!(matches!(err, SsrfError::DisallowedHost { .. }));
        let err = validate_archive_url_for_ssrf("https://255.255.255.255/foo.tar.gz")
            .expect_err("broadcast rejected");
        assert!(matches!(err, SsrfError::DisallowedHost { .. }));
    }

    #[test]
    fn ssrf_rejects_loopback_ipv6() {
        let err = validate_archive_url_for_ssrf("https://[::1]/foo.tar.gz")
            .expect_err("ipv6 loopback rejected");
        assert!(matches!(err, SsrfError::DisallowedHost { .. }));
    }

    #[test]
    fn ssrf_rejects_link_local_ipv6() {
        let err = validate_archive_url_for_ssrf("https://[fe80::1]/foo.tar.gz")
            .expect_err("fe80 rejected");
        assert!(matches!(err, SsrfError::DisallowedHost { .. }));
    }

    #[test]
    fn ssrf_rejects_unique_local_ipv6() {
        let err = validate_archive_url_for_ssrf("https://[fc00::1]/foo.tar.gz")
            .expect_err("fc00 rejected");
        assert!(matches!(err, SsrfError::DisallowedHost { .. }));
        let err = validate_archive_url_for_ssrf("https://[fd00::1]/foo.tar.gz")
            .expect_err("fd00 rejected");
        assert!(matches!(err, SsrfError::DisallowedHost { .. }));
    }

    #[test]
    fn ssrf_rejects_ipv4_mapped_loopback() {
        // ::ffff:127.0.0.1 — same target as 127.0.0.1, must also be refused.
        let err = validate_archive_url_for_ssrf("https://[::ffff:127.0.0.1]/foo.tar.gz")
            .expect_err("ipv4-mapped loopback rejected");
        assert!(matches!(err, SsrfError::DisallowedHost { .. }));
    }

    #[test]
    fn ssrf_rejects_documentation_ranges() {
        let err = validate_archive_url_for_ssrf("https://192.0.2.1/foo.tar.gz")
            .expect_err("TEST-NET-1 rejected");
        assert!(matches!(err, SsrfError::DisallowedHost { .. }));
        let err = validate_archive_url_for_ssrf("https://[2001:db8::1]/foo.tar.gz")
            .expect_err("ipv6 doc rejected");
        assert!(matches!(err, SsrfError::DisallowedHost { .. }));
    }

    #[test]
    fn ssrf_rejects_malformed_url() {
        let err =
            validate_archive_url_for_ssrf("not a url at all").expect_err("malformed rejected");
        assert!(matches!(err, SsrfError::Malformed { .. }));
    }

    #[test]
    fn ssrf_error_display_does_not_leak_query() {
        let err = validate_archive_url_for_ssrf("https://localhost/path?token=supersecret")
            .expect_err("rejected");
        let msg = format!("{err}");
        assert!(
            !msg.contains("supersecret"),
            "ssrf error must not include query token: {msg}"
        );
    }
}
