//! ffmpeg + gifski subprocess orchestration for the demo recorder.
//!
//! Two-stage pipeline (matches industry practice for high-quality GIFs):
//!
//! 1. `ffmpeg -f gdigrab` -> lossless `.mkv` (writing during the run)
//! 2. `ffmpeg -i raw.mkv -vf "fps=20,scale=1280:-1:flags=lanczos"`
//!    -> PNG frames in `target/demo/frames/`
//! 3. `gifski` -> the final `.gif`
//!
//! v1 invokes the SHA-pinned vendored binaries cached under
//! `target/demo/bin/` by [`crate::demo::bin::ensure_bins`]. The exe
//! paths are passed in by [`crate::demo::RealSystem`] so `recorder`
//! itself stays a side-effect-free orchestrator from the tests'
//! perspective (the actual `Command::status()` calls are mock-free
//! because `RealSystem` is the only caller).
//!
//! # Capture readiness
//!
//! ffmpeg's gdigrab takes a non-trivial amount of time to bring up
//! the screen-grabber the first time and to write the .mkv header.
//! Sending input before the header is written produces a recording
//! whose first frames are missing the action that just happened.
//! [`wait_for_capture_baseline`] polls
//! [`DemoSystem::file_size`](crate::demo::DemoSystem::file_size)
//! until ffmpeg has written enough bytes to guarantee the capture
//! pipeline is live, with a generous timeout. The DSL stays unaware
//! of this readiness contract: the script just emits `StartCapture`
//! and trusts the recorder.

use std::io::Write;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use anyhow::{bail, Context, Result};

use super::DemoSystem;

/// Capture resolution and framerate. Pinned to keep recordings
/// identical across developer machines and CI runners.
const CAPTURE_FRAMERATE: &str = "30";
const CAPTURE_VIDEO_SIZE: &str = "1920x1080";

/// Encode parameters for the GIF. Re-used in the retry ladder if the
/// output exceeds the size budget (deferred to v3).
const ENCODE_FPS: &str = "20";
const ENCODE_WIDTH: &str = "1280";
const ENCODE_QUALITY: &str = "90";

/// Bytes the .mkv must reach before the capture is considered live.
/// gdigrab writes a Matroska header (~600-800 bytes) plus at least
/// one frame's worth of huffyuv-encoded data before flushing. 8 KiB
/// gives us comfortable margin without being so high that we wait
/// for several frames on a slow machine.
const CAPTURE_BASELINE_BYTES: u64 = 8 * 1024;

/// Hard ceiling on the readiness wait. ffmpeg gdigrab on a clean
/// Windows Sandbox boots in ~1-2 seconds; 15 seconds covers slow
/// disks and the Carnac overlay's first foreground steal.
const CAPTURE_BASELINE_TIMEOUT: Duration = Duration::from_secs(15);

/// Poll interval for [`wait_for_capture_baseline`].
const CAPTURE_BASELINE_POLL: Duration = Duration::from_millis(100);

/// Spawn the long-running ffmpeg gdigrab capture writing to `out_raw`.
///
/// Returns the child process so [`stop_ffmpeg_and_encode`] can shut it
/// down cleanly via `q\n` on stdin.
///
/// # Arguments
///
/// * `ffmpeg_exe` - absolute path to the vendored ffmpeg.exe (see
///   [`crate::demo::bin`]).
/// * `out_raw` - destination `.mkv`; parent directories are created.
pub fn spawn_ffmpeg_gdigrab(ffmpeg_exe: &Path, out_raw: &Path) -> Result<Child> {
    if let Some(parent) = out_raw.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }
    let child = Command::new(ffmpeg_exe)
        .args([
            "-y",
            "-f",
            "gdigrab",
            "-framerate",
            CAPTURE_FRAMERATE,
            "-video_size",
            CAPTURE_VIDEO_SIZE,
            "-i",
            "desktop",
            "-c:v",
            "ffvhuff",
        ])
        .arg(out_raw)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .with_context(|| {
            format!(
                "failed to spawn vendored ffmpeg at {}",
                ffmpeg_exe.display()
            )
        })?;
    Ok(child)
}

/// Block until ffmpeg has written at least
/// [`CAPTURE_BASELINE_BYTES`] to `out_raw`, indicating the capture
/// pipeline is live and subsequent input will be recorded.
///
/// Polls [`DemoSystem::file_size`] at [`CAPTURE_BASELINE_POLL`]; the
/// `system.sleep` is used between polls so unit tests can short-
/// circuit the wait.
///
/// # Errors
///
/// Returns an error when the file does not reach the baseline within
/// [`CAPTURE_BASELINE_TIMEOUT`]. The caller (the trait method
/// `start_recording`) is responsible for any teardown.
pub fn wait_for_capture_baseline<S: DemoSystem>(system: &S, out_raw: &Path) -> Result<()> {
    let deadline = Instant::now() + CAPTURE_BASELINE_TIMEOUT;
    loop {
        if system.path_exists(out_raw) {
            // file_size can transiently fail on Windows while ffmpeg
            // holds an exclusive write handle; treat that as "not
            // yet" and keep polling.
            if let Ok(size) = system.file_size(out_raw) {
                if size >= CAPTURE_BASELINE_BYTES {
                    system.print_debug(&format!(
                        "recorder: capture baseline reached ({size} bytes)"
                    ));
                    return Ok(());
                }
            }
        }
        if Instant::now() >= deadline {
            bail!(
                "ffmpeg did not reach capture baseline ({} bytes) within {:?}; \
                 was the gdigrab device available?",
                CAPTURE_BASELINE_BYTES,
                CAPTURE_BASELINE_TIMEOUT
            );
        }
        system.sleep(CAPTURE_BASELINE_POLL);
    }
}

/// Stop the in-flight ffmpeg, run the frame-extract step, then gifski.
///
/// `out_raw` is the lossless `.mkv` ffmpeg has been writing.
/// `out_gif` is the final GIF the caller asked for.
///
/// # Arguments
///
/// * `child` - the running ffmpeg gdigrab process.
/// * `ffmpeg_exe` - absolute path to the vendored ffmpeg.exe (used
///   again for the frame-extract step).
/// * `gifski_exe` - absolute path to the vendored gifski.exe.
/// * `out_raw` - the lossless `.mkv` written by `child`.
/// * `out_gif` - destination GIF path.
pub fn stop_ffmpeg_and_encode(
    mut child: Child,
    ffmpeg_exe: &Path,
    gifski_exe: &Path,
    out_raw: &Path,
    out_gif: &Path,
) -> Result<()> {
    // Politely ask ffmpeg to flush + exit by sending `q\n` on stdin;
    // it converts the partial buffer into a valid container.
    if let Some(stdin) = child.stdin.as_mut() {
        let _ = stdin.write_all(b"q\n");
    }
    let status = child.wait().context("waiting for ffmpeg gdigrab")?;
    if !status.success() {
        // Non-zero on graceful `q` is rare but documented; do not
        // bail unconditionally because the .mkv may still be valid.
        eprintln!("ffmpeg gdigrab exited with {status}; continuing to encode");
    }
    if !out_raw.exists() {
        bail!(
            "ffmpeg did not produce {}: cannot continue to gifski",
            out_raw.display()
        );
    }

    let frames_dir = out_raw
        .parent()
        .map(|p| p.join("frames"))
        .unwrap_or_else(|| Path::new("frames").to_path_buf());
    if frames_dir.exists() {
        std::fs::remove_dir_all(&frames_dir)
            .with_context(|| format!("failed to clear {}", frames_dir.display()))?;
    }
    std::fs::create_dir_all(&frames_dir)
        .with_context(|| format!("failed to create {}", frames_dir.display()))?;

    // Frame extraction (vendored ffmpeg).
    let extract_status = Command::new(ffmpeg_exe)
        .args(["-y", "-i"])
        .arg(out_raw)
        .args([
            "-vf",
            &format!("fps={ENCODE_FPS},scale={ENCODE_WIDTH}:-1:flags=lanczos"),
        ])
        .arg(frames_dir.join("%05d.png"))
        .status()
        .with_context(|| {
            format!(
                "failed to spawn vendored ffmpeg at {}",
                ffmpeg_exe.display()
            )
        })?;
    if !extract_status.success() {
        bail!("ffmpeg frame extraction failed with {extract_status}");
    }

    // gifski encode (vendored gifski).
    if let Some(parent) = out_gif.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }
    let frame_glob = frames_dir.join("*.png");
    let gifski_status = Command::new(gifski_exe)
        .args([
            "--fps",
            ENCODE_FPS,
            "--width",
            ENCODE_WIDTH,
            "--quality",
            ENCODE_QUALITY,
            "-o",
        ])
        .arg(out_gif)
        .arg(frame_glob)
        .status()
        .with_context(|| {
            format!(
                "failed to spawn vendored gifski at {}",
                gifski_exe.display()
            )
        })?;
    if !gifski_status.success() {
        bail!("gifski exited with {gifski_status}");
    }
    Ok(())
}

#[cfg(test)]
#[path = "../tests/test_demo_recorder.rs"]
mod tests;
