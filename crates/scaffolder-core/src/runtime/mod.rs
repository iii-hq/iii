//! Runtime detection and tool management
//!
//! This module provides:
//! - Language runtime detection (Node.js, Bun, Python)
//! - Generic tool management for CLI tools like iii

pub mod check;
pub mod tool;

pub use check::{
    check_bun, check_node, check_python, check_runtimes, check_runtimes_with_advisory, Language,
    RuntimeInfo,
};
pub use tool::ToolManager;
