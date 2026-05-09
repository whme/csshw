//! Sanity test for the canonical v0 script.
//!
//! The point of having a typed DSL is that the script validates at
//! `cargo build` time. This test pins down the contract: the canonical
//! script must build without errors and contain the expected first /
//! last steps. Future scripts (v1+) can clone this pattern.

use crate::demo::dsl::Step;
use crate::demo::script;

#[test]
fn test_canonical_v0_builds() {
    // Act
    let steps = script::build_canonical_v0().build().unwrap();
    // Assert
    assert!(!steps.is_empty());
    assert!(matches!(steps.first(), Some(Step::StartCapture)));
    assert!(matches!(steps.last(), Some(Step::StopCapture)));
}

#[test]
fn test_canonical_v0_contains_a_type_step() {
    // Act
    let steps = script::build_canonical_v0().build().unwrap();
    // Assert
    let typed = steps.iter().any(|s| matches!(s, Step::Type { .. }));
    assert!(typed, "canonical v0 should type at least one command");
}
