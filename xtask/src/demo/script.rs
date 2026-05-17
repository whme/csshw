//! The canonical demo script.
//!
//! This file is the "demo as code" surface: edit it to change what the
//! GIF shows. The DSL ([`crate::demo::dsl`]) is type-checked, so a
//! typo here surfaces as a compile error rather than a recording-time
//! failure.
//!
//! v0 ships [`build_canonical_v0`]: launch csshw with two hosts, wait
//! for both client windows, broadcast a single command, stop. The
//! richer scene (control-mode add-host, vim broadcast, ping/Ctrl+C)
//! arrives in v3 once the chord primitive lands in the DSL.

use crate::demo::dsl::Script;

/// Build the v0 canonical demo: launch + broadcast + stop.
///
/// Returns the unbuilt [`Script`]. Callers (the production
/// `record_demo` entrypoint and unit tests) are expected to call
/// `.build()` to validate and consume into a `Vec<Step>`.
///
/// # Window-title regexes
///
/// The regexes match titles produced by csshw itself when it spawns
/// console windows. csshw uses titles like `daemon [...]` and
/// `<user>@<host>` for clients; we keep the regexes loose (`(?i)` and
/// no anchors) so future title tweaks do not silently break the demo.
pub fn build_canonical_v0() -> Script {
    let mut s = Script::new("csshw-demo-v0");
    s.start_capture()
        .marker("v0: launch + broadcast + stop")
        .wait_for(r"(?i)daemon")
        .wait_for(r"(?i)alpha")
        .wait_for(r"(?i)bravo")
        .focus(r"(?i)daemon")
        .sleep_ms(800)
        .type_text("whoami\r")
        .sleep_ms(2000)
        .stop_capture();
    s
}

#[cfg(test)]
#[path = "../tests/test_demo_script.rs"]
mod tests;
