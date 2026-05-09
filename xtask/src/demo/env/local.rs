//! Local environment provider: run the demo on the caller's own
//! interactive desktop session.
//!
//! v0's smallest reviewable provider. There is no isolation, no
//! wallpaper normalisation, and no Carnac. The caller is expected to
//! launch the command and step away while the demo records.
//! Sandbox-based isolation arrives in v1.

use std::path::{Path, PathBuf};

use anyhow::Result;

use crate::demo::{config_override, driver, dsl::Step, DemoSystem};

/// Hosts the v0 canonical script launches csshw with. Kept here (not
/// in `script.rs`) because `config_override::generate` needs them too,
/// and it is the env layer that owns the demo-tree on disk.
pub const V0_HOSTS: &[&str] = &["alpha", "bravo"];

/// Prepare and run the demo on the local desktop.
///
/// Sets up `target/demo/` (config, dispatcher, fake homes), copies the
/// pre-built `csshw.exe` into it (so csshw's startup
/// `set_current_dir(exe_dir)` lands on our config rather than the
/// developer's real one), launches csshw, runs the driver, and
/// terminates csshw on exit.
///
/// # Arguments
///
/// * `system` - the [`DemoSystem`].
/// * `steps` - validated steps from [`crate::demo::dsl::Script::build`].
/// * `out_gif` - desired GIF path.
/// * `no_record` - forwarded to the driver; skips capture for fast
///   script iteration.
pub fn run<S: DemoSystem>(
    system: &S,
    steps: &[Step],
    out_gif: &Path,
    no_record: bool,
) -> Result<()> {
    let workspace = system.workspace_root()?;
    let demo_root = workspace.join("target").join("demo");
    system.ensure_dir(&demo_root)?;
    let layout = config_override::generate(system, &demo_root, V0_HOSTS)?;
    system.print_info(&format!(
        "local env: prepared {} fake hosts under {}",
        V0_HOSTS.len(),
        layout.csshw_cwd.display(),
    ));

    // Copy csshw.exe into the demo directory. csshw rebases its cwd
    // to its own exe_dir on startup (src/cli.rs:548), so the config
    // we just wrote is only picked up if csshw runs from there.
    let source_exe = locate_csshw_exe(system, &workspace)?;
    let demo_exe = layout.csshw_cwd.join("csshw.exe");
    system.copy_file(&source_exe, &demo_exe)?;

    let host_args: Vec<String> = V0_HOSTS.iter().map(|h| (*h).to_string()).collect();
    system.print_info(&format!(
        "local env: launching {} {}",
        demo_exe.display(),
        host_args.join(" "),
    ));
    system.spawn_csshw(&demo_exe, &host_args, &layout.csshw_cwd)?;

    let driver_result = driver::run(system, steps, out_gif, no_record);

    // Always attempt cleanup, regardless of driver outcome.
    if let Err(e) = system.terminate_csshw() {
        system.print_debug(&format!("terminate_csshw failed: {e}"));
    }

    driver_result
}

/// Locate a built csshw.exe under the workspace's `target/` directory.
///
/// Prefers a release build (smaller, no debug overhead) and falls back
/// to debug. v0 fails loudly if neither exists; v1 will offer to build
/// it for the caller. The existence check goes through
/// [`DemoSystem::file_exists`] so this function is unit-testable with
/// a pure mock.
fn locate_csshw_exe<S: DemoSystem>(system: &S, workspace: &Path) -> Result<PathBuf> {
    for profile in ["release", "debug"] {
        let candidate = workspace.join("target").join(profile).join("csshw.exe");
        if system.file_exists(&candidate) {
            return Ok(candidate);
        }
    }
    anyhow::bail!(
        "could not find csshw.exe under target/release or target/debug. \
         Run `cargo build --release` first."
    )
}
