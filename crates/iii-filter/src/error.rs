use std::fmt;

/// The error codes a filter can be rejected with (`protocol.md` §4).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ErrorCode {
    /// Syntax, operator, operand or limit violation.
    InvalidFilter,
    /// A path that the function's `request_format` / `response_format` rules out.
    InvalidFilterPath,
}

impl ErrorCode {
    /// The wire spelling: `INVALID_FILTER` or `INVALID_FILTER_PATH`.
    pub fn as_str(&self) -> &'static str {
        match self {
            ErrorCode::InvalidFilter => "INVALID_FILTER",
            ErrorCode::InvalidFilterPath => "INVALID_FILTER_PATH",
        }
    }
}

impl fmt::Display for ErrorCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A rejected filter: the wire error code and a human-readable message.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FilterError {
    pub code: ErrorCode,
    pub message: String,
}

impl FilterError {
    pub(crate) fn invalid(message: impl Into<String>) -> Self {
        FilterError {
            code: ErrorCode::InvalidFilter,
            message: message.into(),
        }
    }

    pub(crate) fn invalid_path(message: impl Into<String>) -> Self {
        FilterError {
            code: ErrorCode::InvalidFilterPath,
            message: message.into(),
        }
    }
}

impl fmt::Display for FilterError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for FilterError {}
