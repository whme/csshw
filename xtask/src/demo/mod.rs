//! Automated demo recording (`record-demo` xtask subcommand).
//!
//! This module turns the README's `demo/csshw.gif` from a hand-recorded
//! artifact into a reproducible build output. A typed Rust DSL
//! ([`dsl::Step`]) describes the demo as an ordered list of actions
//! (launch, wait-for-window, focus, type, sleep, start/stop capture);
//! the [`driver`] interprets it against a [`DemoSystem`] that abstracts
//! every side effect (Windows input synthesis, filesystem writes,
//! subprocess spawning, sleeps). Tests mock [`DemoSystem`] to assert
//! step semantics with zero real-system effects.
//!
//! v0 scope: a single `--env local` provider that runs on the caller's
//! own desktop (no isolation) and a hard-coded canonical script that
//! launches `csshw alpha bravo`, types a broadcast command, and stops.
//! Sandbox + Carnac + visual normalisation arrive in v1; CI workflows
//! and the orphan-branch publish flow arrive in v2; the full
//! control-mode + vim + ping scene arrives in v3.

#![cfg_attr(coverage_nightly, coverage(off))]

pub mod config_override;
pub mod driver;
pub mod dsl;
pub mod env;
pub mod recorder;
pub mod script;

use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::Result;
use clap::ValueEnum;

/// Supported environment providers for `record-demo`.
///
/// Each variant maps to a module under [`env`] that is responsible for
/// preparing the recording environment (writing csshw config, building
/// a fake-host home tree, optionally normalising the desktop) and then
/// invoking the shared [`driver`].
#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum DemoEnv {
    /// Run on the caller's own interactive desktop session. v0 default.
    /// No isolation - the caller is expected to step away while the
    /// demo records.
    Local,
}

/// One top-level window snapshot returned by [`DemoSystem::enum_windows`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WindowInfo {
    /// Opaque handle. We model `HWND` as `u64` so the trait stays
    /// portable across platforms; the production impl casts back.
    pub hwnd: u64,
    /// Title text as returned by `GetWindowTextW` and lossily decoded.
    pub title: String,
    /// Window rect in screen coordinates.
    pub rect: WindowRect,
}

/// Window bounds in screen pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WindowRect {
    /// Left edge.
    pub x: i32,
    /// Top edge.
    pub y: i32,
    /// Width in pixels.
    pub width: i32,
    /// Height in pixels.
    pub height: i32,
}

/// All side effects the demo subcommand needs.
///
/// Implemented for production by [`RealSystem`] and mocked in tests via
/// `mockall`. Following the pattern of `xtask/src/social_preview.rs`,
/// every concrete I/O call lives behind one of these methods so unit
/// tests assert behaviour without touching the real system.
///
/// `hosts` is `&[String]` rather than `&[&str]` because mockall does
/// not handle the implicit lifetime in the latter.
pub trait DemoSystem {
    /// Absolute path to the workspace root (parent of `xtask/`).
    fn workspace_root(&self) -> Result<PathBuf>;

    /// Create `path` and any missing ancestors. No-op if it exists.
    fn ensure_dir(&self, path: &Path) -> Result<()>;

    /// Write `content` to `path`, creating ancestor directories.
    fn write_file(&self, path: &Path, content: &str) -> Result<()>;

    /// Copy `from` to `to`, replacing any existing file.
    fn copy_file(&self, from: &Path, to: &Path) -> Result<()>;

    /// Return whether `path` exists on disk. Routed through the
    /// trait (rather than `Path::exists`) so the env layer can be
    /// unit-tested with a pure mock.
    fn file_exists(&self, path: &Path) -> bool;

    /// Enumerate visible top-level windows.
    fn enum_windows(&self) -> Result<Vec<WindowInfo>>;

    /// Bring the window identified by `hwnd` to the foreground.
    /// Production impl applies the `AttachThreadInput` workaround.
    fn set_foreground(&self, hwnd: u64) -> Result<()>;

    /// Synthesise a Unicode keypress for the given codepoint via
    /// `SendInput(KEYEVENTF_UNICODE)`. The character lands in the
    /// foreground window.
    fn send_unicode_char(&self, c: char) -> Result<()>;

    /// Synthesise a virtual-key keypress (e.g. VK_RETURN). Used for
    /// keys Unicode injection can't carry as text (Enter, Esc, F-keys).
    fn send_vk(&self, vk: u16) -> Result<()>;

    /// Block the current thread for `duration`. Trait method (rather
    /// than `std::thread::sleep`) so tests can short-circuit waits.
    fn sleep(&self, duration: Duration);

    /// Launch csshw with the given hosts, working directory, and exe
    /// path. Fire-and-forget: returns once the daemon process is
    /// spawned, not when it exits. Production impl tracks the child
    /// internally so [`terminate_csshw`](Self::terminate_csshw) can
    /// kill it on cleanup.
    fn spawn_csshw(&self, exe: &Path, hosts: &[String], cwd: &Path) -> Result<()>;

    /// Kill the in-flight csshw daemon (if any) and best-effort kill
    /// any leaked client `csshw.exe` instances. Idempotent.
    fn terminate_csshw(&self) -> Result<()>;

    /// Start a screen capture writing to `out_raw`. Production impl
    /// spawns ffmpeg gdigrab and stores the child handle internally so
    /// [`stop_recording`](Self::stop_recording) can terminate it.
    fn start_recording(&self, out_raw: &Path) -> Result<()>;

    /// Terminate the in-flight capture, run the post-encode pipeline
    /// (frame extraction + gifski), and produce `out_gif`.
    fn stop_recording(&self, out_raw: &Path, out_gif: &Path) -> Result<()>;

    /// Print an informational message to stdout.
    fn print_info(&self, message: &str);

    /// Print a verbose message to stderr (gated on `CSSHW_XTASK_VERBOSE`).
    fn print_debug(&self, message: &str);
}

/// Production implementation of [`DemoSystem`].
///
/// Holds two long-lived child processes between method calls:
/// the in-flight ffmpeg gdigrab capture, and the spawned csshw
/// daemon. All Windows-API calls live in the `windows_input` private
/// module behind `cfg(target_os = "windows")`.
pub struct RealSystem {
    capture: std::sync::Mutex<Option<std::process::Child>>,
    csshw: std::sync::Mutex<Option<std::process::Child>>,
}

impl RealSystem {
    /// Construct a [`RealSystem`] with no in-flight children.
    pub fn new() -> Self {
        Self {
            capture: std::sync::Mutex::new(None),
            csshw: std::sync::Mutex::new(None),
        }
    }
}

impl Default for RealSystem {
    fn default() -> Self {
        Self::new()
    }
}

mod windows_input;

impl DemoSystem for RealSystem {
    fn workspace_root(&self) -> Result<PathBuf> {
        let manifest_dir = env!("CARGO_MANIFEST_DIR");
        Path::new(manifest_dir)
            .parent()
            .map(Path::to_path_buf)
            .ok_or_else(|| anyhow::anyhow!("failed to resolve workspace root"))
    }

    fn ensure_dir(&self, path: &Path) -> Result<()> {
        std::fs::create_dir_all(path)
            .map_err(|e| anyhow::anyhow!("failed to create {}: {e}", path.display()))
    }

    fn write_file(&self, path: &Path, content: &str) -> Result<()> {
        if let Some(parent) = path.parent() {
            self.ensure_dir(parent)?;
        }
        std::fs::write(path, content)
            .map_err(|e| anyhow::anyhow!("failed to write {}: {e}", path.display()))
    }

    fn copy_file(&self, from: &Path, to: &Path) -> Result<()> {
        if let Some(parent) = to.parent() {
            self.ensure_dir(parent)?;
        }
        std::fs::copy(from, to).map(|_| ()).map_err(|e| {
            anyhow::anyhow!("failed to copy {} -> {}: {e}", from.display(), to.display())
        })
    }

    fn file_exists(&self, path: &Path) -> bool {
        path.exists()
    }

    fn enum_windows(&self) -> Result<Vec<WindowInfo>> {
        windows_input::enum_windows()
    }

    fn set_foreground(&self, hwnd: u64) -> Result<()> {
        windows_input::set_foreground(hwnd)
    }

    fn send_unicode_char(&self, c: char) -> Result<()> {
        windows_input::send_unicode_char(c)
    }

    fn send_vk(&self, vk: u16) -> Result<()> {
        windows_input::send_vk(vk)
    }

    fn sleep(&self, duration: Duration) {
        std::thread::sleep(duration);
    }

    fn spawn_csshw(&self, exe: &Path, hosts: &[String], cwd: &Path) -> Result<()> {
        let mut slot = self.csshw.lock().expect("csshw mutex poisoned");
        if slot.is_some() {
            anyhow::bail!("spawn_csshw called while a daemon is already running");
        }
        let mut cmd = std::process::Command::new(exe);
        cmd.args(hosts).current_dir(cwd);
        let child = cmd
            .spawn()
            .map_err(|e| anyhow::anyhow!("failed to spawn {}: {e}", exe.display()))?;
        *slot = Some(child);
        Ok(())
    }

    fn terminate_csshw(&self) -> Result<()> {
        // Kill the daemon child we tracked.
        if let Some(mut child) = self.csshw.lock().expect("csshw mutex poisoned").take() {
            let _ = child.kill();
            let _ = child.wait();
        }
        // Belt-and-braces: the daemon spawns clients via
        // CreateProcessW(CREATE_NEW_CONSOLE), which detaches them from
        // the daemon. Kill any lingering csshw.exe by image name.
        // This is acceptable in dev contexts; v1 will switch to a
        // Job Object so cleanup is automatic and safe.
        let _ = std::process::Command::new("taskkill")
            .args(["/IM", "csshw.exe", "/T", "/F"])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status();
        Ok(())
    }

    fn start_recording(&self, out_raw: &Path) -> Result<()> {
        let mut slot = self.capture.lock().expect("capture mutex poisoned");
        if slot.is_some() {
            anyhow::bail!("start_recording called while a capture is already running");
        }
        let child = recorder::spawn_ffmpeg_gdigrab(out_raw)?;
        *slot = Some(child);
        Ok(())
    }

    fn stop_recording(&self, out_raw: &Path, out_gif: &Path) -> Result<()> {
        let child = self
            .capture
            .lock()
            .expect("capture mutex poisoned")
            .take()
            .ok_or_else(|| anyhow::anyhow!("stop_recording called with no active capture"))?;
        recorder::stop_ffmpeg_and_encode(child, out_raw, out_gif)
    }

    fn print_info(&self, message: &str) {
        println!("INFO - {message}");
    }

    fn print_debug(&self, message: &str) {
        if std::env::var("CSSHW_XTASK_VERBOSE")
            .map(|v| !v.is_empty())
            .unwrap_or(false)
        {
            eprintln!("DEBUG - {message}");
        }
    }
}

/// Top-level entry point for `cargo xtask record-demo`.
///
/// Orchestrates: build the canonical [`dsl::Script`], delegate
/// environment preparation to the matching `env::*` module, run the
/// driver, return.
///
/// # Arguments
///
/// * `system` - the [`DemoSystem`] (real or mocked).
/// * `out` - desired GIF path. Defaults to
///   `<workspace>/target/demo/csshw.gif`.
/// * `env` - which environment provider to use.
/// * `no_record` - skip [`dsl::Step::StartCapture`] /
///   [`dsl::Step::StopCapture`]. Useful for iterating on the script
///   without burning capture time.
/// * `no_overlay` - skip the Carnac keystroke overlay. v0 always
///   behaves as if this is true (Carnac arrives in v1).
pub fn record_demo<S: DemoSystem>(
    system: &S,
    out: Option<PathBuf>,
    env: DemoEnv,
    no_record: bool,
    no_overlay: bool,
) -> Result<()> {
    // The demo subcommand drives Windows desktop input via
    // `windows_input` and captures the screen with ffmpeg gdigrab,
    // both of which only exist on Windows. Bail early with a clear
    // message instead of letting the caller hit a misleading
    // "csshw.exe not found" or "gdigrab unavailable" error mid-run.
    if !cfg!(target_os = "windows") {
        anyhow::bail!("record-demo is Windows-only; this is a non-Windows build");
    }
    let workspace = system.workspace_root()?;
    let out = out.unwrap_or_else(|| workspace.join("target/demo/csshw.gif"));
    let script = script::build_canonical_v0().build()?;
    system.print_info(&format!(
        "record-demo: env={env:?} out={} steps={} no_record={no_record} no_overlay={no_overlay}",
        out.display(),
        script.len(),
    ));
    match env {
        DemoEnv::Local => env::local::run(system, &script, &out, no_record)?,
    }
    Ok(())
}

#[cfg(test)]
#[path = "../tests/test_demo_mod.rs"]
mod tests;
