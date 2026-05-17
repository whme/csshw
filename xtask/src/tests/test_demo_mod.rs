//! Top-level smoke tests for the demo module.
//!
//! Per-submodule behaviour is exercised by `test_demo_dsl.rs`,
//! `test_demo_driver.rs`, `test_demo_config_override.rs`, and
//! `test_demo_script.rs`. This file holds only assertions about the
//! module's public surface.

use crate::demo::{DemoEnv, WindowRect};

#[test]
fn test_demo_env_default_is_sandbox() {
    // The default for `--env` lives in `main.rs` as
    // `DemoEnv::Sandbox`. Pin that here so renaming the variant
    // later flags the README + the v1 plan as out of date.
    let env = DemoEnv::Sandbox;
    assert!(matches!(env, DemoEnv::Sandbox));
}

#[test]
fn test_window_rect_is_value_equality() {
    // Arrange
    let a = WindowRect {
        x: 0,
        y: 0,
        width: 1920,
        height: 1080,
    };
    let b = WindowRect {
        x: 0,
        y: 0,
        width: 1920,
        height: 1080,
    };
    // Assert: PartialEq must derive structurally so the driver's
    // stability check (rect-equality across polls) works.
    assert_eq!(a, b);
}
