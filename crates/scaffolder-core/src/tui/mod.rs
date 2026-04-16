//! CLI prompts using cliclack (Charm-style inline prompts)
//!
//! This module is optional and only available when the `tui` feature is enabled.

#[cfg(feature = "tui")]
mod prompts;

#[cfg(feature = "tui")]
pub use prompts::{run, CreateArgs};
