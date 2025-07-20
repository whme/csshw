//! Unit tests for the constants module.

#![deny(clippy::implicit_return)]
#![allow(clippy::needless_return, clippy::doc_overindented_list_items)]

use crate::utils::constants::*;

/// Test module for constants validation.
mod constants_test {
    use super::*;

    #[test]
    fn test_pipe_name_format() {
        // Test that PIPE_NAME follows the expected Windows named pipe format
        assert!(PIPE_NAME.starts_with(r"\\.\pipe\"));
        // Test that PIPE_NAME incorporates the package name
        assert!(PIPE_NAME.contains(PKG_NAME));
    }

}
