//! Tests for the demo DSL.
//!
//! Pure data manipulation - no `DemoSystem` mock needed.

use std::time::Duration;

use crate::demo::dsl::{
    Script, Step, DEFAULT_PER_CHAR_DELAY, DEFAULT_WAIT_STABLE_FOR, DEFAULT_WAIT_TIMEOUT,
};

#[test]
fn test_script_records_steps_in_order() {
    // Arrange
    let mut s = Script::new("ordering");
    // Act
    s.start_capture()
        .wait_for("daemon")
        .focus("daemon")
        .type_text("hi\r")
        .sleep_ms(500)
        .stop_capture();
    // Assert
    let steps = s.build().unwrap();
    assert_eq!(steps.len(), 6);
    assert!(matches!(steps[0], Step::StartCapture));
    assert!(matches!(steps[1], Step::WaitForWindow { .. }));
    assert!(matches!(steps[2], Step::Focus { .. }));
    assert!(matches!(steps[3], Step::Type { .. }));
    assert!(matches!(steps[4], Step::Sleep(_)));
    assert!(matches!(steps[5], Step::StopCapture));
}

#[test]
fn test_wait_for_uses_defaults() {
    // Arrange
    let mut s = Script::new("defaults");
    s.start_capture().wait_for("d").stop_capture();
    // Act
    let steps = s.build().unwrap();
    // Assert
    let Step::WaitForWindow {
        timeout,
        stable_for,
        ..
    } = &steps[1]
    else {
        panic!("expected WaitForWindow");
    };
    assert_eq!(*timeout, DEFAULT_WAIT_TIMEOUT);
    assert_eq!(*stable_for, DEFAULT_WAIT_STABLE_FOR);
}

#[test]
fn test_type_text_uses_default_per_char_delay() {
    // Arrange
    let mut s = Script::new("defaults");
    s.start_capture().type_text("ab").stop_capture();
    // Act
    let steps = s.build().unwrap();
    // Assert
    let Step::Type { per_char_delay, .. } = &steps[1] else {
        panic!("expected Type");
    };
    assert_eq!(*per_char_delay, DEFAULT_PER_CHAR_DELAY);
}

#[test]
fn test_build_rejects_invalid_regex() {
    // Arrange
    let mut s = Script::new("bad-regex");
    s.start_capture().wait_for("(unclosed").stop_capture();
    // Act
    let err = s.build().unwrap_err().to_string();
    // Assert
    assert!(err.contains("invalid title_regex"), "got: {err}");
}

#[test]
fn test_build_rejects_missing_start_capture() {
    // Arrange
    let mut s = Script::new("no-start");
    s.wait_for("daemon").stop_capture();
    // Act
    let err = s.build().unwrap_err().to_string();
    // Assert
    assert!(err.contains("missing StartCapture"), "got: {err}");
}

#[test]
fn test_build_rejects_missing_stop_capture() {
    // Arrange
    let mut s = Script::new("no-stop");
    s.start_capture().wait_for("daemon");
    // Act
    let err = s.build().unwrap_err().to_string();
    // Assert
    assert!(err.contains("missing StopCapture"), "got: {err}");
}

#[test]
fn test_build_rejects_duplicate_capture() {
    // Arrange
    let mut s = Script::new("dup");
    s.start_capture().start_capture().stop_capture();
    // Act
    let err = s.build().unwrap_err().to_string();
    // Assert
    assert!(
        err.contains("StartCapture appears more than once"),
        "got: {err}"
    );
}

#[test]
fn test_build_rejects_stop_before_start() {
    // Arrange
    let mut s = Script::new("inverted");
    s.stop_capture().start_capture();
    // Act
    let err = s.build().unwrap_err().to_string();
    // Assert
    assert!(err.contains("precedes StartCapture"), "got: {err}");
}

#[test]
fn test_wait_for_with_overrides_durations() {
    // Arrange
    let custom_timeout = Duration::from_secs(7);
    let custom_stable = Duration::from_millis(123);
    let mut s = Script::new("custom");
    s.start_capture()
        .wait_for_with("d", custom_timeout, custom_stable)
        .stop_capture();
    // Act
    let steps = s.build().unwrap();
    // Assert
    let Step::WaitForWindow {
        timeout,
        stable_for,
        ..
    } = &steps[1]
    else {
        panic!("expected WaitForWindow");
    };
    assert_eq!(*timeout, custom_timeout);
    assert_eq!(*stable_for, custom_stable);
}
