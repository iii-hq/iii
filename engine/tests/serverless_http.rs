use iii::invocation::url_validator::{SecurityError, UrlValidator, UrlValidatorConfig};

#[tokio::test]
async fn test_url_validator_private_ip() {
    let config = UrlValidatorConfig {
        allowlist: vec!["127.0.0.1".to_string()],
        block_private_ips: true,
        require_https: false,
    };
    let validator = UrlValidator::new(config).unwrap();
    let result = validator.validate("http://127.0.0.1/test").await;
    assert!(matches!(result, Err(SecurityError::PrivateIpBlocked)));
}

#[tokio::test]
async fn test_url_validator_allowlist() {
    let config = UrlValidatorConfig {
        allowlist: vec!["example.com".to_string()],
        block_private_ips: false,
        require_https: false,
    };
    let validator = UrlValidator::new(config).unwrap();
    let result = validator.validate("http://example.com/path").await;
    assert!(result.is_ok());
}
