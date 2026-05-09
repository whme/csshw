//! Step-by-step interpreter for a built demo script.
//!
//! Takes a `&[Step]` plus a [`DemoSystem`] and walks the steps in order,
//! delegating every side effect to the system trait. The driver has a
//! tiny amount of internal state (where to write the raw capture file,
//! how many `StartCapture` we've seen) so unit tests can assert
//! capture pairing.
//!
//! # Errors
//!
//! Returns the first error encountered. Capture is best-effort cleaned
//! up: if a [`Step::StartCapture`] succeeded and a later step fails,
//! the driver still attempts [`DemoSystem::stop_recording`] before
//! returning to avoid leaving an ffmpeg child orphaned. The cleanup
//! error, if any, is logged via [`DemoSystem::print_debug`] and the
//! original error is propagated.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use anyhow::{bail, Context, Result};
use regex::Regex;

use super::{dsl::Step, DemoSystem};

/// Polling interval used by [`Step::WaitForWindow`] between
/// `enum_windows` calls. Short enough to be responsive, long enough not
/// to spin the CPU.
const POLL_INTERVAL: Duration = Duration::from_millis(100);

/// Run `steps` against `system`.
///
/// # Arguments
///
/// * `system` - implementation of [`DemoSystem`].
/// * `steps` - the script's pre-validated steps.
/// * `out_gif` - final GIF path; the raw `.mkv` is derived by replacing
///   the extension with `.mkv` (so both files live alongside each
///   other under `target/demo/`).
/// * `no_record` - when true, [`Step::StartCapture`] /
///   [`Step::StopCapture`] are logged and skipped. Useful for
///   iterating on the script without spawning ffmpeg.
pub fn run<S: DemoSystem>(
    system: &S,
    steps: &[Step],
    out_gif: &Path,
    no_record: bool,
) -> Result<()> {
    let raw_path = derive_raw_path(out_gif);
    let mut state = DriverState::new(raw_path, no_record);
    let mut deferred: Option<anyhow::Error> = None;
    for (i, step) in steps.iter().enumerate() {
        if let Err(e) = run_step(system, &mut state, i, step, out_gif) {
            deferred = Some(e);
            break;
        }
    }
    // Best-effort cleanup if a capture was left running. Mark
    // `capturing` false up front so a failing `stop_recording` here
    // can't be re-entered (e.g. by a future caller looping over
    // `run`); the production `RealSystem::stop_recording` consumes
    // the in-flight ffmpeg child handle, so a second call would error
    // with "no active capture".
    if state.capturing {
        state.capturing = false;
        if let Err(e) = system.stop_recording(&state.raw_path, out_gif) {
            system.print_debug(&format!("cleanup stop_recording failed: {e}"));
        }
    }
    if let Some(e) = deferred {
        return Err(e);
    }
    Ok(())
}

/// Driver-internal state.
struct DriverState {
    raw_path: PathBuf,
    no_record: bool,
    capturing: bool,
}

impl DriverState {
    fn new(raw_path: PathBuf, no_record: bool) -> Self {
        Self {
            raw_path,
            no_record,
            capturing: false,
        }
    }
}

/// Replace the extension of `gif_path` with `.mkv` to get the raw
/// capture path. If `gif_path` has no extension, append `.mkv`.
fn derive_raw_path(gif_path: &Path) -> PathBuf {
    let mut p = gif_path.to_path_buf();
    p.set_extension("mkv");
    p
}

/// Dispatch a single step.
fn run_step<S: DemoSystem>(
    system: &S,
    state: &mut DriverState,
    index: usize,
    step: &Step,
    out_gif: &Path,
) -> Result<()> {
    system.print_debug(&format!("step {index}: {step:?}"));
    match step {
        Step::WaitForWindow {
            title_regex,
            timeout,
            stable_for,
        } => wait_for_window(system, title_regex, *timeout, *stable_for)
            .with_context(|| format!("step {index}: WaitForWindow {title_regex:?}")),
        Step::Focus { title_regex } => focus(system, title_regex)
            .with_context(|| format!("step {index}: Focus {title_regex:?}")),
        Step::Type {
            text,
            per_char_delay,
        } => {
            type_text(system, text, *per_char_delay).with_context(|| format!("step {index}: Type"))
        }
        Step::Sleep(d) => {
            system.sleep(*d);
            Ok(())
        }
        Step::StartCapture => {
            if state.no_record {
                system.print_info(&format!("step {index}: StartCapture skipped (--no-record)"));
                return Ok(());
            }
            system.start_recording(&state.raw_path)?;
            state.capturing = true;
            Ok(())
        }
        Step::StopCapture => {
            if state.no_record {
                system.print_info(&format!("step {index}: StopCapture skipped (--no-record)"));
                return Ok(());
            }
            // Mark the capture as stopped up front so a failing
            // `stop_recording` cannot be retried by the cleanup path
            // in `run` (which would double-consume the ffmpeg child
            // and report "no active capture").
            state.capturing = false;
            system.stop_recording(&state.raw_path, out_gif)?;
            Ok(())
        }
        Step::Marker(m) => {
            system.print_info(&format!("marker: {m}"));
            Ok(())
        }
    }
}

/// Block until a window matching `title_regex` has been visible with
/// the same rect for at least `stable_for`. Polls every
/// [`POLL_INTERVAL`].
fn wait_for_window<S: DemoSystem>(
    system: &S,
    title_regex: &str,
    timeout: Duration,
    stable_for: Duration,
) -> Result<()> {
    let re =
        Regex::new(title_regex).with_context(|| format!("invalid title_regex {title_regex:?}"))?;
    let deadline = Instant::now() + timeout;
    let mut stable_since: Option<(u64, super::WindowRect, Instant)> = None;
    loop {
        let windows = system.enum_windows()?;
        if let Some(w) = windows.into_iter().find(|w| re.is_match(&w.title)) {
            match stable_since {
                Some((hwnd, rect, since))
                    if hwnd == w.hwnd && rect == w.rect && since.elapsed() >= stable_for =>
                {
                    return Ok(());
                }
                Some((hwnd, rect, _)) if hwnd == w.hwnd && rect == w.rect => {
                    // Still stabilising; fall through to sleep.
                }
                _ => {
                    stable_since = Some((w.hwnd, w.rect, Instant::now()));
                }
            }
        } else {
            stable_since = None;
        }
        if Instant::now() >= deadline {
            bail!("no window matching {title_regex:?} stabilised within {timeout:?}");
        }
        system.sleep(POLL_INTERVAL);
    }
}

/// Bring the first window matching `title_regex` to the foreground.
fn focus<S: DemoSystem>(system: &S, title_regex: &str) -> Result<()> {
    let re =
        Regex::new(title_regex).with_context(|| format!("invalid title_regex {title_regex:?}"))?;
    let windows = system.enum_windows()?;
    let target = windows
        .into_iter()
        .find(|w| re.is_match(&w.title))
        .ok_or_else(|| anyhow::anyhow!("no window matching {title_regex:?}"))?;
    system.set_foreground(target.hwnd)
}

/// Type `text` one character at a time. Newlines (`\n`, `\r`) are sent
/// as VK_RETURN so they actually submit a command in cmd.exe instead
/// of inserting a literal control character.
fn type_text<S: DemoSystem>(system: &S, text: &str, per_char_delay: Duration) -> Result<()> {
    /// Windows `VK_RETURN` virtual-key code.
    const VK_RETURN: u16 = 0x0D;
    for c in text.chars() {
        match c {
            '\n' | '\r' => system.send_vk(VK_RETURN)?,
            other => system.send_unicode_char(other)?,
        }
        system.sleep(per_char_delay);
    }
    Ok(())
}

#[cfg(test)]
#[path = "../tests/test_demo_driver.rs"]
mod tests;
