//! Sandbox environment provider: run the demo inside a fresh
//! Windows Sandbox VM with a normalised desktop and an optional
//! Carnac keystroke overlay.
//!
//! v1's hermetic recording path. The host:
//!
//! 1. Ensures `target/demo/bin/` is populated (vendored ffmpeg,
//!    gifski, Carnac with SHA verification) via
//!    [`crate::demo::bin::ensure_bins`].
//! 2. Builds `target/demo/csshw-demo.wsb` from a string template
//!    that mounts the workspace (read-only), the bin cache
//!    (read-only), `xtask/demo-assets/` (read-only), and a
//!    writable output folder (`target/demo/out/`) into known paths
//!    inside the sandbox.
//! 3. Launches the sandbox via
//!    [`DemoSystem::spawn_sandbox`](crate::demo::DemoSystem::spawn_sandbox).
//!    The `LogonCommand` runs `sandbox-bootstrap.ps1`, which sources
//!    `setup-desktop.ps1`, optionally launches Carnac, builds csshw,
//!    invokes `xtask record-demo --env local`, copies the resulting
//!    GIF to `C:\demo\out\csshw.gif`, and writes a sentinel
//!    `C:\demo\out\done.flag` with the exit status before shutting
//!    the sandbox VM down.
//! 4. Polls the host-side mount for `done.flag`, copies the GIF
//!    back to the user-requested path, and tears the sandbox down.
//!
//! Windows Sandbox is unavailable on GitHub-hosted runners (no
//! nested virtualisation), so this provider is the local-iteration
//! path. The `ci_runner` provider in v2 will own the canonical
//! recording path on `windows-2022`.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use anyhow::{bail, Context, Result};

use crate::demo::{bin, DemoSystem};

/// Sandbox-side root for everything we mount.
const SANDBOX_ROOT: &str = "C:\\demo";

/// Sandbox-side mount points. Hard-coded so the bootstrap script
/// (PowerShell, no command-line plumbing) can reference them.
const SANDBOX_REPO: &str = "C:\\demo\\repo";
const SANDBOX_BIN: &str = "C:\\demo\\bin";
const SANDBOX_ASSETS: &str = "C:\\demo\\assets";
const SANDBOX_OUT: &str = "C:\\demo\\out";

/// Sentinel file the in-sandbox bootstrap writes once it has
/// finished (successfully or otherwise). Its content is the literal
/// text `ok` on success, or `error: <message>` on failure.
const SENTINEL_NAME: &str = "done.flag";

/// File name the bootstrap copies the recorded GIF to. Decoupled
/// from the host-side `out_gif` argument so callers can choose any
/// destination without leaking that path into the sandbox.
const SANDBOX_GIF_NAME: &str = "csshw.gif";

/// Hard ceiling on how long we wait for the sentinel to appear.
/// Sandbox boot + cargo build + 5-second capture + gifski encode
/// fits comfortably in 8 minutes even on a cold cache; longer than
/// that suggests the bootstrap itself wedged.
const SENTINEL_TIMEOUT: Duration = Duration::from_secs(8 * 60);

/// Poll interval for [`wait_for_sentinel`]. Quick enough that the
/// host loop wakes up promptly when the sandbox writes the file;
/// slow enough not to hammer NTFS.
const SENTINEL_POLL: Duration = Duration::from_millis(500);

/// Resolved layout of the demo working tree on the host. Returned
/// by [`prepare_layout`] so [`run`] and the unit tests share the
/// path-building code.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SandboxLayout {
    /// Absolute workspace root.
    pub workspace: PathBuf,
    /// `<workspace>/target/demo/`.
    pub demo_root: PathBuf,
    /// `<workspace>/target/demo/bin/`.
    pub bin_dir: PathBuf,
    /// `<workspace>/xtask/demo-assets/`.
    pub assets_dir: PathBuf,
    /// `<workspace>/target/demo/out/`.
    pub out_dir: PathBuf,
    /// `<workspace>/target/demo/csshw-demo.wsb`.
    pub wsb_path: PathBuf,
    /// Host path of the sentinel file the bootstrap writes.
    pub sentinel: PathBuf,
    /// Host path the bootstrap copies the recorded GIF to (under
    /// the writable mount).
    pub sandbox_gif: PathBuf,
}

/// Resolve every host-side path the sandbox provider needs.
///
/// Pure path arithmetic: no I/O, no trait calls. Kept separate so
/// the unit tests assert mount layout without setting up filesystem
/// mocks.
pub fn prepare_layout(workspace: &Path) -> SandboxLayout {
    let demo_root = workspace.join("target").join("demo");
    let out_dir = demo_root.join("out");
    SandboxLayout {
        workspace: workspace.to_path_buf(),
        demo_root: demo_root.clone(),
        bin_dir: demo_root.join("bin"),
        assets_dir: workspace.join("xtask").join("demo-assets"),
        out_dir: out_dir.clone(),
        wsb_path: demo_root.join("csshw-demo.wsb"),
        sentinel: out_dir.join(SENTINEL_NAME),
        sandbox_gif: out_dir.join(SANDBOX_GIF_NAME),
    }
}

/// Build the `.wsb` XML body that boots the demo.
///
/// Five mount points are pinned to fixed sandbox-side paths so the
/// bootstrap PowerShell script can hard-code them without command-
/// line plumbing:
///
/// | Host path                              | Sandbox path     | RO  |
/// |----------------------------------------|------------------|-----|
/// | `<workspace>`                          | [`SANDBOX_REPO`] | yes |
/// | `<workspace>/target/demo/bin`          | [`SANDBOX_BIN`]  | yes |
/// | `<workspace>/xtask/demo-assets`        | [`SANDBOX_ASSETS`]| yes |
/// | `<workspace>/target/demo/out`          | [`SANDBOX_OUT`]  | no  |
///
/// `<Resolution>` is intentionally not set: as of Windows 11 23H2
/// the sandbox config schema does not expose a stable resolution
/// element. The bootstrap script normalises the desktop (1920x1080,
/// 100 % scale, wallpaper, console font) by sourcing
/// `setup-desktop.ps1` after first sign-in, which is the only place
/// these settings reliably apply.
///
/// `no_overlay` is forwarded to the bootstrap via a positional
/// argument so the same `.wsb` template covers both code paths.
pub fn render_wsb(layout: &SandboxLayout, no_overlay: bool) -> String {
    let overlay_arg = if no_overlay { "-NoOverlay" } else { "" };
    // The bootstrap is run via `cmd /c powershell ...` because
    // Windows Sandbox's `<Command>` runs in a non-interactive shell
    // where `powershell.exe` direct invocation occasionally races
    // the user-profile mount.
    let bootstrap = format!(
        "cmd.exe /c \"powershell -NoProfile -ExecutionPolicy Bypass \
         -File {SANDBOX_ASSETS}\\sandbox-bootstrap.ps1 {overlay_arg}\""
    );
    format!(
        "<Configuration>\r\n\
         \x20\x20<VGpu>Disable</VGpu>\r\n\
         \x20\x20<Networking>Default</Networking>\r\n\
         \x20\x20<AudioInput>Disable</AudioInput>\r\n\
         \x20\x20<VideoInput>Disable</VideoInput>\r\n\
         \x20\x20<ProtectedClient>Enable</ProtectedClient>\r\n\
         \x20\x20<MappedFolders>\r\n\
         {repo}\
         {bins}\
         {assets}\
         {out}\
         \x20\x20</MappedFolders>\r\n\
         \x20\x20<LogonCommand>\r\n\
         \x20\x20\x20\x20<Command>{bootstrap}</Command>\r\n\
         \x20\x20</LogonCommand>\r\n\
         </Configuration>\r\n",
        repo = mapped_folder(&layout.workspace, SANDBOX_REPO, true),
        bins = mapped_folder(&layout.bin_dir, SANDBOX_BIN, true),
        assets = mapped_folder(&layout.assets_dir, SANDBOX_ASSETS, true),
        out = mapped_folder(&layout.out_dir, SANDBOX_OUT, false),
    )
}

/// Render one `<MappedFolder>` block.
///
/// The host path is emitted via `Display`, which on Windows uses
/// backslashes. XML escaping is intentionally minimal: paths cannot
/// contain `<`, `>`, `&`, or `"` on Windows, so we sidestep those
/// cases entirely.
fn mapped_folder(host: &Path, sandbox: &str, read_only: bool) -> String {
    let ro = if read_only { "true" } else { "false" };
    format!(
        "\x20\x20\x20\x20<MappedFolder>\r\n\
         \x20\x20\x20\x20\x20\x20<HostFolder>{}</HostFolder>\r\n\
         \x20\x20\x20\x20\x20\x20<SandboxFolder>{sandbox}</SandboxFolder>\r\n\
         \x20\x20\x20\x20\x20\x20<ReadOnly>{ro}</ReadOnly>\r\n\
         \x20\x20\x20\x20</MappedFolder>\r\n",
        host.display()
    )
}

/// Block until `sentinel` exists, then return its contents.
///
/// Polls [`DemoSystem::path_exists`] every [`SENTINEL_POLL`] until
/// either the file appears or [`SENTINEL_TIMEOUT`] elapses. Uses
/// [`DemoSystem::sleep`] so unit tests can short-circuit the wait.
///
/// # Errors
///
/// Returns an error on timeout, including the elapsed duration so
/// the user can distinguish "sandbox never booted" from "demo took
/// too long" by checking the host-side log.
pub fn wait_for_sentinel<S: DemoSystem>(system: &S, sentinel: &Path) -> Result<()> {
    let start = Instant::now();
    let deadline = start + SENTINEL_TIMEOUT;
    loop {
        if system.path_exists(sentinel) {
            return Ok(());
        }
        if Instant::now() >= deadline {
            bail!(
                "sandbox sentinel {} did not appear within {:?}; \
                 the in-sandbox bootstrap likely wedged",
                sentinel.display(),
                SENTINEL_TIMEOUT
            );
        }
        system.sleep(SENTINEL_POLL);
    }
}

/// Prepare and run the demo inside a fresh Windows Sandbox VM.
///
/// # Arguments
///
/// * `system` - the [`DemoSystem`].
/// * `out_gif` - host-side destination GIF; the bootstrap always
///   writes its GIF to the sandbox-mounted out folder, so this
///   function copies the result to `out_gif` after the sandbox
///   exits.
/// * `no_record` - currently forwarded only to the host-side log.
///   The in-sandbox xtask call is what actually skips capture; v1
///   keeps that wiring local to the bootstrap script for simplicity.
/// * `no_overlay` - skip the Carnac overlay inside the sandbox.
///
/// # Errors
///
/// Returns an error when the bin cache cannot be populated, the
/// `.wsb` cannot be written, the sandbox fails to launch, the
/// sentinel times out, or the bootstrap reports a non-`ok`
/// completion status.
pub fn run<S: DemoSystem>(
    system: &S,
    out_gif: &Path,
    no_record: bool,
    no_overlay: bool,
) -> Result<()> {
    let workspace = system.workspace_root()?;
    let layout = prepare_layout(&workspace);
    system.print_info(&format!(
        "sandbox env: workspace={} no_record={no_record} no_overlay={no_overlay}",
        layout.workspace.display(),
    ));

    // Ensure the vendored binaries are present on the host before
    // we mount them read-only into the sandbox. The sandbox cannot
    // populate this cache itself: its network is sandboxed and the
    // download would have to repeat on every run.
    bin::ensure_bins(system, &layout.bin_dir)
        .with_context(|| "preparing target/demo/bin/ for sandbox mount")?;

    // Wipe leftover sentinels and GIFs from previous runs so the
    // poll loop can use plain "exists" without a timestamp check.
    system.ensure_dir(&layout.out_dir)?;
    if system.path_exists(&layout.sentinel) {
        system.print_debug(&format!(
            "sandbox env: removing stale sentinel {}",
            layout.sentinel.display()
        ));
        std::fs::remove_file(&layout.sentinel).with_context(|| {
            format!(
                "failed to clear stale sentinel {}",
                layout.sentinel.display()
            )
        })?;
    }
    if system.path_exists(&layout.sandbox_gif) {
        std::fs::remove_file(&layout.sandbox_gif).with_context(|| {
            format!(
                "failed to clear stale sandbox-side gif {}",
                layout.sandbox_gif.display()
            )
        })?;
    }

    let wsb = render_wsb(&layout, no_overlay);
    system.write_file(&layout.wsb_path, &wsb)?;
    system.print_info(&format!(
        "sandbox env: wrote {} (mount root {SANDBOX_ROOT})",
        layout.wsb_path.display()
    ));

    system.spawn_sandbox(&layout.wsb_path)?;
    let result = (|| -> Result<()> {
        wait_for_sentinel(system, &layout.sentinel)?;
        let status = std::fs::read_to_string(&layout.sentinel)
            .with_context(|| format!("reading sentinel {}", layout.sentinel.display()))?;
        let status_trim = status.trim();
        if status_trim != "ok" {
            bail!("sandbox bootstrap reported non-ok status: {}", status_trim);
        }
        if !system.path_exists(&layout.sandbox_gif) {
            bail!(
                "sandbox reported success but {} is missing",
                layout.sandbox_gif.display()
            );
        }
        system.copy_file(&layout.sandbox_gif, out_gif)?;
        system.print_info(&format!(
            "sandbox env: copied recorded GIF to {}",
            out_gif.display()
        ));
        Ok(())
    })();

    if let Err(e) = system.terminate_sandbox() {
        system.print_debug(&format!("terminate_sandbox failed: {e}"));
    }
    result
}

#[cfg(test)]
#[path = "../../tests/test_demo_env_sandbox.rs"]
mod tests;
