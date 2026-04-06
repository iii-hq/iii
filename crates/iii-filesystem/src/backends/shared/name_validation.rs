//! Name validation for filesystem operations.
//!
//! Every operation that accepts a guest-provided directory entry name must
//! call [`validate_name`] to prevent path traversal attacks.

use std::{ffi::CStr, io};

use super::platform;

//--------------------------------------------------------------------------------------------------
// Functions
//--------------------------------------------------------------------------------------------------

/// Validate a directory entry name, blocking traversal attacks.
///
/// Rejects: empty names, `..`, and names containing `/`.
///
/// Backslash is intentionally allowed -- it is a valid filename character on
/// Linux. The filesystem operates on raw bytes, not path-separator-aware
/// strings.
pub(crate) fn validate_name(name: &CStr) -> io::Result<()> {
    let bytes = name.to_bytes();

    if bytes.is_empty() {
        return Err(platform::einval());
    }
    if bytes == b".." {
        return Err(platform::eperm());
    }
    if bytes.contains(&b'/') {
        return Err(platform::eperm());
    }

    Ok(())
}

//--------------------------------------------------------------------------------------------------
// Tests
//--------------------------------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::CString;

    fn cstr(s: &[u8]) -> CString {
        CString::new(s.to_vec()).unwrap()
    }

    #[test]
    fn validate_name_accepts_normal() {
        assert!(validate_name(&cstr(b"hello.txt")).is_ok());
        assert!(validate_name(&cstr(b".hidden")).is_ok());
        assert!(validate_name(&cstr(b".")).is_ok()); // validate_name allows "."
    }

    #[test]
    fn validate_name_rejects_empty() {
        let name = unsafe { CStr::from_bytes_with_nul_unchecked(b"\0") };
        assert!(validate_name(name).is_err());
    }

    #[test]
    fn validate_name_rejects_dotdot() {
        assert!(validate_name(&cstr(b"..")).is_err());
    }

    #[test]
    fn validate_name_rejects_slash() {
        assert!(validate_name(&cstr(b"a/b")).is_err());
    }

    #[test]
    fn validate_name_allows_backslash() {
        assert!(validate_name(&cstr(b"a\\b")).is_ok());
    }
}
