//! Integration tests for iii-init.
//!
//! These tests import and exercise real crate types and functions rather than
//! reimplementing logic from scratch. All iii-init functionality is Linux-only,
//! so every test is gated with `#[cfg(target_os = "linux")]`.

// These tests inspect source code and formatting conventions, no Linux APIs needed.

/// Verify that configure_network does NOT bring up the loopback interface.
/// Without lo, 127.0.0.1 traffic routes through eth0 to the host via smoltcp.
#[test]
fn configure_network_source_omits_loopback_setup() {
    let source = include_str!("../src/network.rs");
    let fn_start = source
        .find("fn configure_network")
        .expect("configure_network function exists");
    let block = &source[fn_start..];
    let closure_start = block.find("let result = (||").expect("closure exists");
    let closure_end = block[closure_start..].find("})();").expect("closure end");
    let closure_body = &block[closure_start..closure_start + closure_end];

    // The closure must NOT bring up lo -- that's the whole point of the change.
    assert!(
        !closure_body.contains(r#"set_interface_up(sock, b"lo\0")"#),
        "configure_network must NOT bring up the loopback interface; \
         127.0.0.1 traffic should route through eth0 to reach the host"
    );

    // It MUST still configure eth0.
    assert!(
        closure_body.contains("eth0"),
        "configure_network must still configure eth0"
    );
}

/// Verify the /etc/hosts content format maps localhost to a gateway IP,
/// not to 127.0.0.1 (which would be the unreachable guest loopback).
#[test]
fn etc_hosts_format_maps_localhost_to_gateway() {
    let source = include_str!("../src/network.rs");
    // The write call should produce "<gateway>\tlocalhost\n", not "127.0.0.1\tlocalhost"
    assert!(
        source.contains(r#"format!("{gw}\tlocalhost\n")"#),
        "should write /etc/hosts mapping localhost to the gateway IP"
    );
    assert!(
        !source.contains(r#""127.0.0.1\tlocalhost"#),
        "should NOT write 127.0.0.1 as localhost in /etc/hosts"
    );
}

#[cfg(target_os = "linux")]
mod linux {
    use iii_init::error::InitError;

    #[test]
    fn error_types_display_correctly() {
        let err = InitError::MissingWorkerCmd;
        assert!(
            err.to_string().contains("III_WORKER_CMD"),
            "MissingWorkerCmd should mention the env var"
        );

        let parse_err = "not_a_number".parse::<u64>().unwrap_err();
        let err = InitError::ParseNofile {
            value: "not_a_number".to_string(),
            source: parse_err,
        };
        let msg = err.to_string();
        assert!(msg.contains("not_a_number"));
        assert!(msg.contains("III_INIT_NOFILE"));

        let err = InitError::InvalidAddr {
            var: "III_INIT_IP".into(),
            value: "bad_ip".into(),
        };
        let msg = err.to_string();
        assert!(msg.contains("III_INIT_IP"));
        assert!(msg.contains("bad_ip"));

        let err = InitError::InvalidCidr("abc".into());
        assert!(err.to_string().contains("abc"));

        let io_err = std::io::Error::new(std::io::ErrorKind::NotFound, "test device");
        let err = InitError::Rlimit(io_err);
        assert!(err.to_string().contains("RLIMIT_NOFILE"));

        let io_err = std::io::Error::new(std::io::ErrorKind::PermissionDenied, "cannot mount");
        let err = InitError::WriteFile {
            path: "/etc/resolv.conf".into(),
            source: io_err,
        };
        assert!(err.to_string().contains("/etc/resolv.conf"));
    }

    #[test]
    fn default_nofile_matches_crate_constant() {
        assert_eq!(iii_init::rlimit::DEFAULT_NOFILE, 65536);
        assert!(
            iii_init::rlimit::DEFAULT_NOFILE > 1024,
            "should be higher than typical default"
        );
    }

    #[test]
    fn cidr_to_mask_conversions() {
        use std::net::Ipv4Addr;

        assert_eq!(
            iii_init::network::cidr_to_mask(30),
            Ipv4Addr::new(255, 255, 255, 252)
        );
        assert_eq!(
            iii_init::network::cidr_to_mask(24),
            Ipv4Addr::new(255, 255, 255, 0)
        );
        assert_eq!(
            iii_init::network::cidr_to_mask(16),
            Ipv4Addr::new(255, 255, 0, 0)
        );
        assert_eq!(
            iii_init::network::cidr_to_mask(0),
            Ipv4Addr::new(0, 0, 0, 0)
        );
        assert_eq!(
            iii_init::network::cidr_to_mask(32),
            Ipv4Addr::new(255, 255, 255, 255)
        );
    }

    #[test]
    fn raise_nofile_with_default() {
        let result = iii_init::rlimit::raise_nofile();
        match result {
            Ok(()) => {}
            Err(InitError::Rlimit(_)) => {} // restricted environment (e.g. container)
            Err(e) => panic!("unexpected error: {e}"),
        }
    }

    #[test]
    fn exec_worker_fails_without_env() {
        unsafe { std::env::remove_var("III_WORKER_CMD") };
        let result = iii_init::supervisor::exec_worker();
        assert!(result.is_err());
        assert!(
            matches!(result.unwrap_err(), InitError::MissingWorkerCmd),
            "should fail with MissingWorkerCmd when env var is absent"
        );
    }
}
