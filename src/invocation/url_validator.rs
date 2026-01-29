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
    ip.is_loopback() || ip.is_private() || ip.is_link_local()
}
