//! Unit tests for the utils mod module.

#![deny(clippy::implicit_return)]
#![allow(clippy::needless_return, clippy::doc_overindented_list_items)]

// Import the windows tests module
#[path = "test_windows.rs"]
mod test_windows;

// Note: All Windows API related tests have been moved to test_windows.rs
// This file now serves as the main test module entry point for utils tests
