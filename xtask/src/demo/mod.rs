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
//! v1 scope: two `--env` providers (`local` and `sandbox`) sharing
//! the v0 hard-coded canonical script that launches `csshw alpha
//! bravo`, types a broadcast command, and stops. The recorder uses
//! SHA-pinned vendored ffmpeg + gifski + Carnac (downloaded once
//! into `target/demo/bin/` and verified by [`bin::ensure_bins`]),
//! so a developer no longer needs ffmpeg, gifski, or Carnac on
//! `PATH`. The sandbox provider boots the demo inside a fresh
//! Windows Sandbox VM with a normalised desktop (wallpaper, console
//! font, DPI) and an optional Carnac keystroke overlay; Sandbox
//! cannot run on GitHub-hosted runners (no nested virtualisation),
//! so v1 is the local-iteration path. CI workflows and the
//! orphan-branch publish flow arrive in v2; the full control-mode +
//! vim + ping scene arrives in v3.

#![cfg_attr(coverage_nightly, coverage(off))]

pub mod bin;
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
    /// Run on the caller's own interactive desktop session. No
    /// isolation - the caller is expected to step away while the
    /// demo records. The only provider that works in CI: GitHub-
    /// hosted runners lack the nested virtualisation that Windows
    /// Sandbox requires, so CI workflows must pass `--env local`
    /// explicitly.
    Local,
    /// Run inside a fresh Windows Sandbox VM. Default since v1 so
    /// `cargo xtask record-demo` is hermetic on a developer
    /// workstation. Mounts the workspace read-only, mounts a
    /// writable output folder for the GIF, mounts the cached
    /// vendored binaries, and runs the demo via a `LogonCommand`
    /// that boots `xtask/demo-assets/sandbox-bootstrap.ps1`.
    Sandbox,
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

    /// Return `true` when `path` exists on the host filesystem. Used
    /// for cache hits in [`bin`] and for the sandbox sentinel poll in
    /// [`env::sandbox`].
    fn path_exists(&self, path: &Path) -> bool;

    /// Return the size of `path` in bytes. Used by the recorder to
    /// poll until ffmpeg has written its first capture frames.
    fn file_size(&self, path: &Path) -> Result<u64>;

    /// Download `url` to `dest`, replacing any existing file. Failure
    /// to fetch (HTTP error, redirect loop, transport error) returns
    /// an error.
    fn http_download(&self, url: &str, dest: &Path) -> Result<()>;

    /// Compute the lower-case hex SHA-256 digest of `path`.
    fn sha256_file(&self, path: &Path) -> Result<String>;

    /// Extract `archive` into `dest_dir`. Supports `.zip` and
    /// `.tar.xz` based on the file's extension. The destination is
    /// created if it does not exist; existing contents are not
    /// removed (callers are expected to extract into a clean
    /// directory).
    fn extract_archive(&self, archive: &Path, dest_dir: &Path) -> Result<()>;

    /// Launch `WindowsSandbox.exe` against `wsb_path`. Production
    /// impl tracks the child internally so
    /// [`terminate_sandbox`](Self::terminate_sandbox) can shut it
    /// down on cleanup.
    fn spawn_sandbox(&self, wsb_path: &Path) -> Result<()>;

    /// Best-effort terminate the in-flight Windows Sandbox process.
    /// Idempotent.
    fn terminate_sandbox(&self) -> Result<()>;

    /// Print an informational message to stdout.
    fn print_info(&self, message: &str);

    /// Print a verbose message to stderr (gated on `CSSHW_XTASK_VERBOSE`).
    fn print_debug(&self, message: &str);
}

/// Production implementation of [`DemoSystem`].
///
/// Holds three long-lived child processes between method calls:
/// the in-flight ffmpeg gdigrab capture, the spawned csshw daemon,
/// and (for `--env sandbox`) the WindowsSandbox.exe host. All
/// Windows-API calls live in the `windows_input` private module
/// behind `cfg(target_os = "windows")`.
pub struct RealSystem {
    capture: std::sync::Mutex<Option<std::process::Child>>,
    csshw: std::sync::Mutex<Option<std::process::Child>>,
    sandbox: std::sync::Mutex<Option<std::process::Child>>,
}

impl RealSystem {
    /// Construct a [`RealSystem`] with no in-flight children.
    pub fn new() -> Self {
        Self {
            capture: std::sync::Mutex::new(None),
            csshw: std::sync::Mutex::new(None),
            sandbox: std::sync::Mutex::new(None),
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
        let workspace = self.workspace_root()?;
        let bin_dir = workspace.join("target").join("demo").join("bin");
        let bins = bin::ensure_bins(self, &bin_dir)?;
        let mut slot = self.capture.lock().expect("capture mutex poisoned");
        if slot.is_some() {
            anyhow::bail!("start_recording called while a capture is already running");
        }
        let child = recorder::spawn_ffmpeg_gdigrab(&bins.ffmpeg, out_raw)?;
        // Block until ffmpeg has actually started writing frames so
        // the demo's first keystrokes are captured. The trait `sleep`
        // and `file_size` are used so tests can short-circuit.
        recorder::wait_for_capture_baseline(self, out_raw)?;
        *slot = Some(child);
        Ok(())
    }

    fn stop_recording(&self, out_raw: &Path, out_gif: &Path) -> Result<()> {
        let workspace = self.workspace_root()?;
        let bin_dir = workspace.join("target").join("demo").join("bin");
        let bins = bin::ensure_bins(self, &bin_dir)?;
        let child = self
            .capture
            .lock()
            .expect("capture mutex poisoned")
            .take()
            .ok_or_else(|| anyhow::anyhow!("stop_recording called with no active capture"))?;
        recorder::stop_ffmpeg_and_encode(child, &bins.ffmpeg, &bins.gifski, out_raw, out_gif)
    }

    fn path_exists(&self, path: &Path) -> bool {
        path.exists()
    }

    fn file_size(&self, path: &Path) -> Result<u64> {
        std::fs::metadata(path)
            .map(|m| m.len())
            .map_err(|e| anyhow::anyhow!("failed to stat {}: {e}", path.display()))
    }

    fn http_download(&self, url: &str, dest: &Path) -> Result<()> {
        if let Some(parent) = dest.parent() {
            self.ensure_dir(parent)?;
        }
        // Use PowerShell's Invoke-WebRequest so we inherit the OS's
        // TLS root store and avoid pulling a Rust HTTP client into
        // xtask. `-UseBasicParsing` skips the IE engine warm-up; the
        // first run on a fresh sandbox would otherwise prompt for IE
        // first-launch configuration. Single-quoted PS strings keep
        // backslashes in `dest` literal.
        let dest_str = dest.to_string_lossy().replace('\'', "''");
        let url_str = url.replace('\'', "''");
        let script = format!(
            "$ProgressPreference='SilentlyContinue';\
             [Net.ServicePointManager]::SecurityProtocol=\
             [Net.SecurityProtocolType]::Tls12;\
             Invoke-WebRequest -UseBasicParsing -Uri '{url_str}' -OutFile '{dest_str}'"
        );
        let status = std::process::Command::new("powershell")
            .args(["-NoProfile", "-NonInteractive", "-Command", &script])
            .status()
            .map_err(|e| anyhow::anyhow!("failed to spawn powershell for download: {e}"))?;
        if !status.success() {
            anyhow::bail!("powershell Invoke-WebRequest {url} -> {dest:?} failed: {status}");
        }
        Ok(())
    }

    fn sha256_file(&self, path: &Path) -> Result<String> {
        use sha2::{Digest, Sha256};
        let mut file = std::fs::File::open(path)
            .map_err(|e| anyhow::anyhow!("failed to open {} for hashing: {e}", path.display()))?;
        let mut hasher = Sha256::new();
        std::io::copy(&mut file, &mut hasher)
            .map_err(|e| anyhow::anyhow!("failed to read {} for hashing: {e}", path.display()))?;
        let digest = hasher.finalize();
        Ok(digest.iter().map(|b| format!("{b:02x}")).collect())
    }

    fn extract_archive(&self, archive: &Path, dest_dir: &Path) -> Result<()> {
        self.ensure_dir(dest_dir)?;
        let name = archive
            .file_name()
            .map(|s| s.to_string_lossy().to_lowercase())
            .unwrap_or_default();
        if name.ends_with(".tar.xz") {
            // Windows ships BSD tar.exe but no `xz` binary, so the
            // bundled tar shells out and fails. Decompress + untar
            // in-process instead. Only the gifski release uses
            // tar.xz today; .tar.gz / .tar are not currently
            // exercised, so they are deliberately not handled here.
            let f = std::fs::File::open(archive)
                .map_err(|e| anyhow::anyhow!("failed to open {}: {e}", archive.display()))?;
            let mut tar_bytes = Vec::new();
            lzma_rs::xz_decompress(&mut std::io::BufReader::new(f), &mut tar_bytes)
                .map_err(|e| anyhow::anyhow!("xz_decompress {} failed: {e}", archive.display()))?;
            let mut tar_archive = tar::Archive::new(std::io::Cursor::new(tar_bytes));
            tar_archive.unpack(dest_dir).map_err(|e| {
                anyhow::anyhow!(
                    "tar::unpack {} -> {} failed: {e}",
                    archive.display(),
                    dest_dir.display()
                )
            })?;
            return Ok(());
        }
        if name.ends_with(".zip") || name.ends_with(".nupkg") {
            let archive_str = archive.to_string_lossy().replace('\'', "''");
            let dest_str = dest_dir.to_string_lossy().replace('\'', "''");
            // PowerShell's `Expand-Archive` validates by file
            // extension and refuses anything other than `.zip` (the
            // Carnac release ships as `.nupkg`, which is a zip).
            // Drop down to `System.IO.Compression.ZipFile`, which is
            // format-only. The `Add-Type` call is a no-op if the
            // assembly is already loaded.
            let script = format!(
                "$ProgressPreference='SilentlyContinue';\
                 Add-Type -AssemblyName System.IO.Compression.FileSystem;\
                 [System.IO.Compression.ZipFile]::ExtractToDirectory(\
                 '{archive_str}','{dest_str}',$true)"
            );
            let status = std::process::Command::new("powershell")
                .args(["-NoProfile", "-NonInteractive", "-Command", &script])
                .status()
                .map_err(|e| anyhow::anyhow!("failed to spawn powershell for extract: {e}"))?;
            if !status.success() {
                anyhow::bail!(
                    "ZipFile::ExtractToDirectory {} failed: {status}",
                    archive.display()
                );
            }
            return Ok(());
        }
        anyhow::bail!(
            "extract_archive: unsupported archive extension for {}",
            archive.display()
        )
    }

    fn spawn_sandbox(&self, wsb_path: &Path) -> Result<()> {
        let mut slot = self.sandbox.lock().expect("sandbox mutex poisoned");
        if slot.is_some() {
            anyhow::bail!("spawn_sandbox called while a sandbox is already running");
        }
        let child = std::process::Command::new("WindowsSandbox.exe")
            .arg(wsb_path)
            .spawn()
            .map_err(|e| {
                anyhow::anyhow!(
                    "failed to spawn WindowsSandbox.exe. Enable the \
                     \"Windows Sandbox\" optional feature first \
                     (`Enable-WindowsOptionalFeature -Online \
                     -FeatureName Containers-DisposableClientVM`): {e}"
                )
            })?;
        *slot = Some(child);
        Ok(())
    }

    fn terminate_sandbox(&self) -> Result<()> {
        if let Some(mut child) = self.sandbox.lock().expect("sandbox mutex poisoned").take() {
            let _ = child.kill();
            let _ = child.wait();
        }
        // Belt-and-braces: WindowsSandbox.exe is the launcher, but
        // the sandbox VM itself is hosted by `vmcompute` and the
        // user-facing `WindowsSandboxClient.exe`. A stale client
        // can outlive the launcher. Best-effort taskkill mirrors
        // [`Self::terminate_csshw`].
        let _ = std::process::Command::new("taskkill")
            .args(["/IM", "WindowsSandboxClient.exe", "/F"])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status();
        let _ = std::process::Command::new("taskkill")
            .args(["/IM", "WindowsSandbox.exe", "/F"])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status();
        Ok(())
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
        DemoEnv::Sandbox => env::sandbox::run(system, &out, no_record, no_overlay)?,
    }
    Ok(())
}

#[cfg(test)]
#[path = "../tests/test_demo_mod.rs"]
mod tests;
