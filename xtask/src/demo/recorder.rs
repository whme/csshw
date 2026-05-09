//! ffmpeg + gifski subprocess orchestration for the demo recorder.
//!
//! Two-stage pipeline (matches industry practice for high-quality GIFs):
//!
//! 1. `ffmpeg -f gdigrab` -> lossless `.mkv` (writing during the run)
//! 2. `ffmpeg -i raw.mkv -vf "fps=20,scale=1280:-1:flags=lanczos"`
//!    -> PNG frames in `target/demo/frames/`
//! 3. `gifski` -> the final `.gif`
//!
//! v0 expects `ffmpeg` and `gifski` on `PATH`. v1 will SHA-pin
//! vendored binaries downloaded into `target/demo/bin/`.
//!
//! These free functions are called from [`crate::demo::RealSystem`].
//! They are kept out of the [`crate::demo::DemoSystem`] trait so the
//! trait can be mocked without dragging in `std::process::Child`.

use std::io::Write;
use std::path::Path;
use std::process::{Child, Command, Stdio};

use anyhow::{bail, Context, Result};

/// Capture resolution and framerate. Pinned to keep recordings
/// identical across developer machines and CI runners.
const CAPTURE_FRAMERATE: &str = "30";
const CAPTURE_VIDEO_SIZE: &str = "1920x1080";

/// Encode parameters for the GIF. Re-used in the retry ladder if the
/// output exceeds the size budget (deferred to v3).
const ENCODE_FPS: &str = "20";
const ENCODE_WIDTH: &str = "1280";
const ENCODE_QUALITY: &str = "90";

/// Spawn the long-running ffmpeg gdigrab capture writing to `out_raw`.
///
/// Returns the child process so [`stop_ffmpeg_and_encode`] can shut it
/// down cleanly via `q\n` on stdin.
pub fn spawn_ffmpeg_gdigrab(out_raw: &Path) -> Result<Child> {
    if let Some(parent) = out_raw.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }
    let child = Command::new("ffmpeg")
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
        .context(
            "failed to spawn `ffmpeg`. v0 requires ffmpeg on PATH; \
             install via winget (`winget install Gyan.FFmpeg`) or chocolatey",
        )?;
    Ok(child)
}

/// Stop the in-flight ffmpeg, run the frame-extract step, then gifski.
///
/// `out_raw` is the lossless `.mkv` ffmpeg has been writing.
/// `out_gif` is the final GIF the caller asked for.
pub fn stop_ffmpeg_and_encode(mut child: Child, out_raw: &Path, out_gif: &Path) -> Result<()> {
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

    // Frame extraction.
    let extract_status = Command::new("ffmpeg")
        .args(["-y", "-i"])
        .arg(out_raw)
        .args([
            "-vf",
            &format!("fps={ENCODE_FPS},scale={ENCODE_WIDTH}:-1:flags=lanczos"),
        ])
        .arg(frames_dir.join("%05d.png"))
        .status()
        .context("failed to spawn `ffmpeg` for frame extraction")?;
    if !extract_status.success() {
        bail!("ffmpeg frame extraction failed with {extract_status}");
    }

    // gifski encode.
    if let Some(parent) = out_gif.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }
    let frame_glob = frames_dir.join("*.png");
    let gifski_status = Command::new("gifski")
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
        .context(
            "failed to spawn `gifski`. v0 requires gifski on PATH; \
             install via `cargo install gifski` or download from gif.ski",
        )?;
    if !gifski_status.success() {
        bail!("gifski exited with {gifski_status}");
    }
    Ok(())
}
