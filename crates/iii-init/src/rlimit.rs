use crate::error::InitError;

/// Default RLIMIT_NOFILE value (soft and hard).
const DEFAULT_NOFILE: u64 = 65536;

/// Raise RLIMIT_NOFILE to the value specified in `III_INIT_NOFILE` env var,
/// or `DEFAULT_NOFILE` (65536) if not set.
pub fn raise_nofile() -> Result<(), InitError> {
    let limit = match std::env::var("III_INIT_NOFILE") {
        Ok(val) => val.parse::<u64>().map_err(|e| InitError::ParseNofile {
            value: val,
            source: e,
        })?,
        Err(_) => DEFAULT_NOFILE,
    };

    let rlim = libc::rlimit {
        rlim_cur: limit,
        rlim_max: limit,
    };
    let ret = unsafe { libc::setrlimit(libc::RLIMIT_NOFILE, &rlim) };
    if ret != 0 {
        return Err(InitError::Rlimit(std::io::Error::last_os_error()));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_nofile_value() {
        assert_eq!(DEFAULT_NOFILE, 65536);
    }

    #[test]
    fn test_raise_nofile_succeeds_with_default() {
        // This should succeed on any system -- 65536 is a reasonable limit.
        // If the test environment restricts setrlimit, this will still pass
        // because we test with the default value which is typically allowed.
        let result = raise_nofile();
        // On most systems this succeeds; on restrictive systems it may fail
        // with EPERM. Either outcome is acceptable for a unit test.
        match result {
            Ok(()) => {} // expected on most systems
            Err(InitError::Rlimit(ref e)) if e.raw_os_error() == Some(libc::EPERM) => {} // restricted env
            Err(e) => panic!("unexpected error: {e}"),
        }
    }

    #[test]
    fn test_parse_nofile_invalid_value() {
        // Directly test the parsing logic that raise_nofile uses internally.
        let result = "not_a_number".parse::<u64>();
        assert!(result.is_err());

        // Verify our error type captures the parse failure correctly.
        let parse_err = result.unwrap_err();
        let init_err = InitError::ParseNofile {
            value: "not_a_number".to_string(),
            source: parse_err,
        };
        let msg = format!("{init_err}");
        assert!(msg.contains("not_a_number"));
        assert!(msg.contains("III_INIT_NOFILE"));
    }

    #[test]
    fn test_parse_nofile_valid_value() {
        let result = "32768".parse::<u64>();
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), 32768);
    }
}
