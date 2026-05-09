//! Typed "demo as code" DSL.
//!
//! A demo is a `Vec<Step>` produced by the [`Script`] builder. Each
//! variant of [`Step`] is interpreted by [`crate::demo::driver`] in
//! declaration order. The DSL is intentionally a closed enum so a typo
//! in a script (an unknown step name, an invalid regex, an unbalanced
//! `start_capture` / `stop_capture` pair) fails to compile or fails the
//! `build` validation pass - never at recording time, after the
//! developer has already burned a 30-second capture.
//!
//! See [`crate::demo::script::build_canonical_v0`] for the demo we ship.

use std::time::Duration;

use anyhow::{anyhow, bail, Result};
use regex::Regex;

/// Default per-character delay for [`Step::Type`] when a script does not
/// specify one. Slow enough that the recording is legible, fast enough
/// that a multi-line `Type` step does not pad the GIF.
pub const DEFAULT_PER_CHAR_DELAY: Duration = Duration::from_millis(50);

/// Default timeout for [`Step::WaitForWindow`] when a script does not
/// specify one. Generous: window creation includes csshw spawning a
/// fresh `cmd.exe` per host plus its own daemon initialisation.
pub const DEFAULT_WAIT_TIMEOUT: Duration = Duration::from_secs(30);

/// Default "stable for" window for [`Step::WaitForWindow`]. A window
/// counts as ready only when its rect has been unchanged for this long;
/// guards against typing into a freshly-spawned console that is still
/// being repositioned by csshw's daemon-side layout.
pub const DEFAULT_WAIT_STABLE_FOR: Duration = Duration::from_millis(500);

/// A single deterministic action in the demo timeline.
///
/// Steps are interpreted top-down. None of them carry implicit side
/// effects across step boundaries; the driver state machine does.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Step {
    /// Block until a top-level window whose title matches `title_regex`
    /// has been visible with a stable rect for `stable_for`. Fails if
    /// no such window appears within `timeout`.
    WaitForWindow {
        /// Regex applied to each top-level window's title.
        title_regex: String,
        /// Hard deadline for the whole wait.
        timeout: Duration,
        /// How long the window's rect must be unchanged before the
        /// step counts as satisfied. Guards against typing into a
        /// console that csshw is still repositioning.
        stable_for: Duration,
    },
    /// Bring the matching window to the foreground. The driver applies
    /// the standard `AttachThreadInput + SetForegroundWindow` workaround
    /// because Windows blocks `SetForegroundWindow` from background
    /// processes.
    Focus {
        /// Regex applied to each top-level window's title.
        title_regex: String,
    },
    /// Type `text` into the foreground window, one character at a time
    /// via `SendInput(KEYEVENTF_UNICODE)`. Newlines are translated to
    /// VK_RETURN so they actually submit a command in cmd.exe.
    Type {
        /// The literal text to type.
        text: String,
        /// Delay between successive characters.
        per_char_delay: Duration,
    },
    /// Static pause. Use sparingly; prefer [`Step::WaitForWindow`].
    Sleep(Duration),
    /// Start ffmpeg's gdigrab capture. Must appear exactly once and
    /// before [`Step::StopCapture`].
    StartCapture,
    /// Stop ffmpeg's gdigrab capture and run the post-encode pipeline
    /// (frame extraction + gifski). Must appear exactly once and after
    /// [`Step::StartCapture`].
    StopCapture,
    /// Free-form annotation emitted to the run trace. No side effects.
    Marker(String),
}

/// Validation error returned by [`Script::build`].
///
/// The error carries a human-readable message describing the problem.
/// We rely on `anyhow::Error` to bubble these up to `main.rs`.
pub type ValidationError = anyhow::Error;

/// Builder for a [`Vec<Step>`].
///
/// Methods take `&mut self` and return `&mut Self` so script files read
/// top-to-bottom. Defaults for delays come from the `DEFAULT_*`
/// constants in this module; the `*_with` variants accept an explicit
/// override.
pub struct Script {
    name: String,
    steps: Vec<Step>,
}

impl Script {
    /// Start a new script with the given human-readable name.
    ///
    /// The name is included in the validation error messages so a
    /// failing build pinpoints which script is broken.
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            steps: Vec::new(),
        }
    }

    /// Append a [`Step::WaitForWindow`] using the default timeout and
    /// stability window.
    pub fn wait_for(&mut self, title_regex: &str) -> &mut Self {
        self.wait_for_with(title_regex, DEFAULT_WAIT_TIMEOUT, DEFAULT_WAIT_STABLE_FOR)
    }

    /// Append a [`Step::WaitForWindow`] with explicit timeouts.
    pub fn wait_for_with(
        &mut self,
        title_regex: &str,
        timeout: Duration,
        stable_for: Duration,
    ) -> &mut Self {
        self.steps.push(Step::WaitForWindow {
            title_regex: title_regex.to_string(),
            timeout,
            stable_for,
        });
        self
    }

    /// Append a [`Step::Focus`].
    pub fn focus(&mut self, title_regex: &str) -> &mut Self {
        self.steps.push(Step::Focus {
            title_regex: title_regex.to_string(),
        });
        self
    }

    /// Append a [`Step::Type`] using the default per-character delay.
    pub fn type_text(&mut self, text: &str) -> &mut Self {
        self.type_text_with(text, DEFAULT_PER_CHAR_DELAY)
    }

    /// Append a [`Step::Type`] with an explicit per-character delay.
    pub fn type_text_with(&mut self, text: &str, per_char_delay: Duration) -> &mut Self {
        self.steps.push(Step::Type {
            text: text.to_string(),
            per_char_delay,
        });
        self
    }

    /// Append a [`Step::Sleep`] expressed in milliseconds.
    pub fn sleep_ms(&mut self, ms: u64) -> &mut Self {
        self.steps.push(Step::Sleep(Duration::from_millis(ms)));
        self
    }

    /// Append [`Step::StartCapture`].
    pub fn start_capture(&mut self) -> &mut Self {
        self.steps.push(Step::StartCapture);
        self
    }

    /// Append [`Step::StopCapture`].
    pub fn stop_capture(&mut self) -> &mut Self {
        self.steps.push(Step::StopCapture);
        self
    }

    /// Append a [`Step::Marker`].
    pub fn marker(&mut self, m: impl Into<String>) -> &mut Self {
        self.steps.push(Step::Marker(m.into()));
        self
    }

    /// Validate and finalise the script.
    ///
    /// # Errors
    ///
    /// Returns an error when:
    /// - any `title_regex` is not a valid regex,
    /// - `StartCapture` and `StopCapture` are not each present exactly
    ///   once,
    /// - `StopCapture` precedes `StartCapture`.
    pub fn build(self) -> Result<Vec<Step>, ValidationError> {
        let mut start_idx: Option<usize> = None;
        let mut stop_idx: Option<usize> = None;
        for (i, step) in self.steps.iter().enumerate() {
            match step {
                Step::WaitForWindow { title_regex, .. } | Step::Focus { title_regex } => {
                    Regex::new(title_regex).map_err(|e| {
                        anyhow!("step {i}: invalid title_regex {:?} - {e}", title_regex)
                    })?;
                }
                Step::StartCapture => {
                    if start_idx.is_some() {
                        bail!("StartCapture appears more than once (second at step {i})");
                    }
                    start_idx = Some(i);
                }
                Step::StopCapture => {
                    if stop_idx.is_some() {
                        bail!("StopCapture appears more than once (second at step {i})");
                    }
                    stop_idx = Some(i);
                }
                _ => {}
            }
        }
        match (start_idx, stop_idx) {
            (None, _) => bail!("script {:?} is missing StartCapture", self.name),
            (_, None) => bail!("script {:?} is missing StopCapture", self.name),
            (Some(s), Some(t)) if t <= s => {
                bail!("StopCapture (step {t}) precedes StartCapture (step {s})")
            }
            _ => {}
        }
        Ok(self.steps)
    }
}

#[cfg(test)]
#[path = "../tests/test_demo_dsl.rs"]
mod tests;
