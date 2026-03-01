// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at support@motia.dev
// See LICENSE and PATENTS files for details.

use std::{fmt, net::IpAddr};

use glob::Pattern;
use reqwest::Url;

#[derive(Debug, Clone)]
pub struct UrlValidatorConfig {
    pub allowlist: Vec<String>,
    pub block_private_ips: bool,
    pub require_https: bool,
}

impl Default for UrlValidatorConfig {
    fn default() -> Self {
        Self {
            allowlist: vec!["*".to_string()],
            block_private_ips: true,
            require_https: true,
        }
    }
}

#[derive(Debug)]
pub enum SecurityError {
    InvalidUrl,
    HttpsRequired,
    UrlNotAllowed,
    PrivateIpBlocked,
    DnsLookupFailed,
}

impl fmt::Display for SecurityError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SecurityError::InvalidUrl => write!(f, "Invalid URL"),
            SecurityError::HttpsRequired => write!(f, "HTTPS is required"),
            SecurityError::UrlNotAllowed => write!(f, "URL not in allowlist"),
            SecurityError::PrivateIpBlocked => write!(f, "Private IP blocked"),
            SecurityError::DnsLookupFailed => write!(f, "DNS lookup failed"),
        }
    }
}

impl std::error::Error for SecurityError {}

#[derive(Debug, Clone)]
pub struct UrlValidator {
    allowlist: Vec<Pattern>,
    block_private_ips: bool,
    require_https: bool,
}

impl UrlValidator {
    pub fn new(config: UrlValidatorConfig) -> Result<Self, SecurityError> {
        let mut allowlist = Vec::with_capacity(config.allowlist.len());
        for pattern in config.allowlist {
            let compiled = Pattern::new(&pattern).map_err(|_| SecurityError::InvalidUrl)?;
            allowlist.push(compiled);
        }
        Ok(Self {
            allowlist,
            block_private_ips: config.block_private_ips,
            require_https: config.require_https,
        })
    }

    pub async fn validate(&self, url: &str) -> Result<(), SecurityError> {
        let parsed = Url::parse(url).map_err(|_| SecurityError::InvalidUrl)?;

        if self.require_https && parsed.scheme() != "https" {
            return Err(SecurityError::HttpsRequired);
        }

        let host = parsed.host_str().ok_or(SecurityError::InvalidUrl)?;
        let allowed = self.allowlist.iter().any(|pattern| pattern.matches(host));
        if !allowed {
            return Err(SecurityError::UrlNotAllowed);
        }

        if self.block_private_ips && self.is_private_ip(host).await? {
            return Err(SecurityError::PrivateIpBlocked);
        }

        Ok(())
    }

    async fn is_private_ip(&self, host: &str) -> Result<bool, SecurityError> {
        if let Ok(ip) = host.parse::<IpAddr>() {
            return Ok(is_private_ip(&ip));
        }

        let addrs = tokio::net::lookup_host(format!("{}:443", host))
            .await
            .map_err(|_| SecurityError::DnsLookupFailed)?;
        for addr in addrs {
            let ip = addr.ip();
            if is_private_ip(&ip) {
                return Ok(true);
            }
        }
        Ok(false)
    }
}

fn is_private_ip(ip: &IpAddr) -> bool {
    match ip {
        IpAddr::V4(ipv4) => {
            ipv4.is_loopback()
                || ipv4.is_private()
                || ipv4.is_link_local()
                || ipv4.is_broadcast()
                || ipv4.is_documentation()
                || ipv4.is_unspecified()
        }
        IpAddr::V6(ipv6) => {
            ipv6.is_loopback()
                || is_ipv6_unique_local(ipv6)
                || is_ipv6_link_local(ipv6)
                || ipv6.is_unspecified()
                || ipv6.is_multicast()
        }
    }
}

fn is_ipv6_unique_local(ipv6: &std::net::Ipv6Addr) -> bool {
    (ipv6.segments()[0] & 0xfe00) == 0xfc00
}

fn is_ipv6_link_local(ipv6: &std::net::Ipv6Addr) -> bool {
    (ipv6.segments()[0] & 0xffc0) == 0xfe80
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{Ipv4Addr, Ipv6Addr};

    // ---- UrlValidatorConfig defaults ----

    #[test]
    fn test_url_validator_config_default() {
        let config = UrlValidatorConfig::default();
        assert_eq!(config.allowlist, vec!["*".to_string()]);
        assert!(config.block_private_ips);
        assert!(config.require_https);
    }

    // ---- UrlValidator construction ----

    #[test]
    fn test_url_validator_new_with_defaults() {
        let validator = UrlValidator::new(UrlValidatorConfig::default());
        assert!(validator.is_ok());
    }

    #[test]
    fn test_url_validator_new_with_invalid_glob() {
        let config = UrlValidatorConfig {
            allowlist: vec!["[invalid".to_string()],
            block_private_ips: false,
            require_https: false,
        };
        let result = UrlValidator::new(config);
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), SecurityError::InvalidUrl));
    }

    // ---- HTTPS enforcement ----

    #[tokio::test]
    async fn test_url_validator_https_required_blocks_http() {
        let validator = UrlValidator::new(UrlValidatorConfig {
            allowlist: vec!["*".to_string()],
            block_private_ips: false,
            require_https: true,
        })
        .unwrap();

        let result = validator.validate("http://example.com/path").await;
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), SecurityError::HttpsRequired));
    }

    #[tokio::test]
    async fn test_url_validator_https_required_allows_https() {
        let validator = UrlValidator::new(UrlValidatorConfig {
            allowlist: vec!["*".to_string()],
            block_private_ips: false,
            require_https: true,
        })
        .unwrap();

        let result = validator.validate("https://example.com/path").await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_url_validator_https_not_required_allows_http() {
        let validator = UrlValidator::new(UrlValidatorConfig {
            allowlist: vec!["*".to_string()],
            block_private_ips: false,
            require_https: false,
        })
        .unwrap();

        let result = validator.validate("http://example.com/path").await;
        assert!(result.is_ok());
    }

    // ---- Allowlist ----

    #[tokio::test]
    async fn test_url_validator_allowlist_wildcard() {
        let validator = UrlValidator::new(UrlValidatorConfig {
            allowlist: vec!["*".to_string()],
            block_private_ips: false,
            require_https: false,
        })
        .unwrap();

        assert!(
            validator
                .validate("https://anything.example.com")
                .await
                .is_ok()
        );
    }

    #[tokio::test]
    async fn test_url_validator_allowlist_specific_host() {
        let validator = UrlValidator::new(UrlValidatorConfig {
            allowlist: vec!["api.example.com".to_string()],
            block_private_ips: false,
            require_https: false,
        })
        .unwrap();

        assert!(
            validator
                .validate("https://api.example.com/webhook")
                .await
                .is_ok()
        );

        let result = validator.validate("https://evil.com/webhook").await;
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), SecurityError::UrlNotAllowed));
    }

    #[tokio::test]
    async fn test_url_validator_allowlist_glob_pattern() {
        let validator = UrlValidator::new(UrlValidatorConfig {
            allowlist: vec!["*.example.com".to_string()],
            block_private_ips: false,
            require_https: false,
        })
        .unwrap();

        assert!(
            validator
                .validate("https://api.example.com/webhook")
                .await
                .is_ok()
        );
        assert!(validator.validate("https://sub.example.com").await.is_ok());

        let result = validator.validate("https://example.com").await;
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), SecurityError::UrlNotAllowed));
    }

    #[tokio::test]
    async fn test_url_validator_allowlist_multiple_patterns() {
        let validator = UrlValidator::new(UrlValidatorConfig {
            allowlist: vec!["api.example.com".to_string(), "hooks.slack.com".to_string()],
            block_private_ips: false,
            require_https: false,
        })
        .unwrap();

        assert!(
            validator
                .validate("https://api.example.com/v1")
                .await
                .is_ok()
        );
        assert!(
            validator
                .validate("https://hooks.slack.com/services/x")
                .await
                .is_ok()
        );

        let result = validator.validate("https://other.com").await;
        assert!(result.is_err());
    }

    // ---- Public URLs allowed ----

    #[tokio::test]
    async fn test_url_validator_allows_public() {
        let validator = UrlValidator::new(UrlValidatorConfig {
            allowlist: vec!["*".to_string()],
            block_private_ips: true,
            require_https: false,
        })
        .unwrap();

        // 8.8.8.8 is a public IP (Google DNS).
        let result = validator.validate("https://8.8.8.8/api").await;
        assert!(result.is_ok());
    }

    // ---- Private IPs blocked ----

    #[tokio::test]
    async fn test_url_validator_blocks_private_loopback() {
        let validator = UrlValidator::new(UrlValidatorConfig {
            allowlist: vec!["*".to_string()],
            block_private_ips: true,
            require_https: false,
        })
        .unwrap();

        let result = validator.validate("http://127.0.0.1:8080/internal").await;
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            SecurityError::PrivateIpBlocked
        ));
    }

    #[tokio::test]
    async fn test_url_validator_blocks_private_rfc1918() {
        let validator = UrlValidator::new(UrlValidatorConfig {
            allowlist: vec!["*".to_string()],
            block_private_ips: true,
            require_https: false,
        })
        .unwrap();

        let result = validator.validate("http://10.0.0.1/api").await;
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            SecurityError::PrivateIpBlocked
        ));

        let result = validator.validate("http://192.168.1.1/api").await;
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            SecurityError::PrivateIpBlocked
        ));

        let result = validator.validate("http://172.16.0.1/api").await;
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            SecurityError::PrivateIpBlocked
        ));
    }

    #[tokio::test]
    async fn test_url_validator_private_ip_allowed_when_not_blocked() {
        let validator = UrlValidator::new(UrlValidatorConfig {
            allowlist: vec!["*".to_string()],
            block_private_ips: false,
            require_https: false,
        })
        .unwrap();

        let result = validator.validate("http://127.0.0.1:8080/internal").await;
        assert!(result.is_ok());
    }

    // ---- Invalid URL ----

    #[tokio::test]
    async fn test_url_validator_rejects_invalid_url() {
        let validator = UrlValidator::new(UrlValidatorConfig {
            allowlist: vec!["*".to_string()],
            block_private_ips: false,
            require_https: false,
        })
        .unwrap();

        let result = validator.validate("not a url at all").await;
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), SecurityError::InvalidUrl));
    }

    #[tokio::test]
    async fn test_url_validator_reports_dns_lookup_failures() {
        let validator = UrlValidator::new(UrlValidatorConfig {
            allowlist: vec!["*".to_string()],
            block_private_ips: true,
            require_https: true,
        })
        .unwrap();

        let result = validator.validate("https://nonexistent.invalid").await;
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            SecurityError::DnsLookupFailed
        ));
    }

    #[tokio::test]
    async fn test_url_validator_checks_dns_results_for_public_hosts() {
        let validator = UrlValidator::new(UrlValidatorConfig {
            allowlist: vec!["example.com".to_string()],
            block_private_ips: true,
            require_https: true,
        })
        .unwrap();

        let result = validator.validate("https://example.com").await;
        assert!(result.is_ok());
    }

    // ---- is_private_ip unit tests ----

    #[test]
    fn test_is_private_ip_v4_loopback() {
        assert!(is_private_ip(&IpAddr::V4(Ipv4Addr::LOCALHOST)));
    }

    #[test]
    fn test_is_private_ip_v4_private_ranges() {
        assert!(is_private_ip(&IpAddr::V4(Ipv4Addr::new(10, 0, 0, 1))));
        assert!(is_private_ip(&IpAddr::V4(Ipv4Addr::new(172, 16, 0, 1))));
        assert!(is_private_ip(&IpAddr::V4(Ipv4Addr::new(192, 168, 0, 1))));
    }

    #[test]
    fn test_is_private_ip_v4_link_local() {
        assert!(is_private_ip(&IpAddr::V4(Ipv4Addr::new(169, 254, 1, 1))));
    }

    #[test]
    fn test_is_private_ip_v4_broadcast() {
        assert!(is_private_ip(&IpAddr::V4(Ipv4Addr::BROADCAST)));
    }

    #[test]
    fn test_is_private_ip_v4_unspecified() {
        assert!(is_private_ip(&IpAddr::V4(Ipv4Addr::UNSPECIFIED)));
    }

    #[test]
    fn test_is_private_ip_v4_public() {
        assert!(!is_private_ip(&IpAddr::V4(Ipv4Addr::new(8, 8, 8, 8))));
        assert!(!is_private_ip(&IpAddr::V4(Ipv4Addr::new(1, 1, 1, 1))));
    }

    #[test]
    fn test_is_private_ip_v6_loopback() {
        assert!(is_private_ip(&IpAddr::V6(Ipv6Addr::LOCALHOST)));
    }

    #[test]
    fn test_is_private_ip_v6_unspecified() {
        assert!(is_private_ip(&IpAddr::V6(Ipv6Addr::UNSPECIFIED)));
    }

    #[test]
    fn test_is_private_ip_v6_unique_local() {
        // fc00::/7 range
        let ip: Ipv6Addr = "fd00::1".parse().unwrap();
        assert!(is_private_ip(&IpAddr::V6(ip)));
    }

    #[test]
    fn test_is_private_ip_v6_link_local() {
        // fe80::/10 range
        let ip: Ipv6Addr = "fe80::1".parse().unwrap();
        assert!(is_private_ip(&IpAddr::V6(ip)));
    }

    #[test]
    fn test_is_private_ip_v6_public() {
        let ip: Ipv6Addr = "2001:4860:4860::8888".parse().unwrap();
        assert!(!is_private_ip(&IpAddr::V6(ip)));
    }

    // ---- SecurityError Display ----

    #[test]
    fn test_security_error_display() {
        assert_eq!(SecurityError::InvalidUrl.to_string(), "Invalid URL");
        assert_eq!(
            SecurityError::HttpsRequired.to_string(),
            "HTTPS is required"
        );
        assert_eq!(
            SecurityError::UrlNotAllowed.to_string(),
            "URL not in allowlist"
        );
        assert_eq!(
            SecurityError::PrivateIpBlocked.to_string(),
            "Private IP blocked"
        );
        assert_eq!(
            SecurityError::DnsLookupFailed.to_string(),
            "DNS lookup failed"
        );
    }
}
