//! Generate a demo-only `csshw-config.toml` and per-host fake homes.
//!
//! csshw rebases its cwd to its own `exe_dir` at startup
//! (`src/cli.rs:548`), so the demo flow copies `csshw.exe` into
//! `<demo_root>/csshw.exe` and writes the config alongside it. With
//! that layout, `[client]` overrides `program = "ssh"` to a local
//! `cmd.exe` that opens a "fake host" - no real sshd, no mutation of
//! the developer's real config.
//!
//! csshw's `username_host_placeholder` substitutes `<user>@<host>`
//! into `[client] arguments`. To avoid pinning our directory layout
//! to that exact format (and to handle the empty-username case
//! gracefully), every host routes through a single
//! `<demo_root>/dispatcher.bat` that strips the optional `<user>@`
//! prefix and dispatches to the per-host `enter.bat`.
//!
//! Each host gets a `<demo_root>/fakehosts/<host>/enter.bat` that sets
//! a readable prompt and `cd`s into a per-host home directory with a
//! curated set of files. Differences between hosts are what makes the
//! demo interesting (e.g. `secret.txt` only on `charlie`).

use std::path::{Path, PathBuf};

use anyhow::Result;

use super::DemoSystem;

/// File contents shared by every fake host, keyed by relative path
/// within the home directory. v0 is intentionally minimal.
const SHARED_HOME_FILES: &[(&str, &str)] = &[(
    "README.txt",
    "csshw demo - shared file present on every host.\r\n",
)];

/// Files unique to a specific host, keyed by host name. The inner
/// tuples are `(relative path, contents)`.
const HOST_SPECIFIC_FILES: &[(&str, &[(&str, &str)])] =
    &[("charlie", &[("secret.txt", "charlie-only payload\r\n")])];

/// Result of [`generate`]: paths the caller passes back into the
/// driver and (for `csshw_cwd`) into [`DemoSystem::spawn_csshw`].
pub struct OverrideLayout {
    /// Directory containing `csshw.exe`, `csshw-config.toml`, the
    /// `dispatcher.bat`, and the `fakehosts/` subtree. csshw is
    /// launched from here (cwd-rebased to here by csshw itself).
    pub csshw_cwd: PathBuf,
}

/// Write the demo's csshw config, dispatcher, and per-host fake
/// homes.
///
/// # Arguments
///
/// * `system` - file IO is delegated through [`DemoSystem`] so unit
///   tests can mock it out.
/// * `demo_root` - parent directory; the function writes
///   `demo_root/csshw-config.toml`, `demo_root/dispatcher.bat`, and
///   `demo_root/fakehosts/<host>/...`.
/// * `hosts` - list of bare host names (no `user@` prefix).
///
/// # Returns
///
/// An [`OverrideLayout`] whose `csshw_cwd` is `demo_root`.
pub fn generate<S: DemoSystem>(
    system: &S,
    demo_root: &Path,
    hosts: &[&str],
) -> Result<OverrideLayout> {
    system.ensure_dir(demo_root)?;
    let fakehosts = demo_root.join("fakehosts");
    for host in hosts {
        let home = fakehosts.join(host);
        system.ensure_dir(&home)?;
        for (rel, content) in SHARED_HOME_FILES {
            system.write_file(&home.join(rel), content)?;
        }
        for (h, files) in HOST_SPECIFIC_FILES {
            if h == host {
                for (rel, content) in *files {
                    system.write_file(&home.join(rel), content)?;
                }
            }
        }
        let bat = home.join("enter.bat");
        system.write_file(&bat, &enter_bat(host, &home))?;
    }
    let dispatcher = demo_root.join("dispatcher.bat");
    system.write_file(&dispatcher, dispatcher_bat())?;
    let toml = render_toml(&dispatcher);
    system.write_file(&demo_root.join("csshw-config.toml"), &toml)?;
    Ok(OverrideLayout {
        csshw_cwd: demo_root.to_path_buf(),
    })
}

/// Build the per-host `enter.bat` that the dispatcher invokes.
///
/// `@echo off` keeps the output free of cmd-echo lines, then we set a
/// readable prompt (`<host>-fake $$`) and `cd` into the host's home
/// directory. The trailing `cls` clears the cmd-launch banner so the
/// recording starts on a clean console.
fn enter_bat(host: &str, home: &Path) -> String {
    format!(
        "@echo off\r\nset PROMPT=$_{host}@{host}-fake $$ \r\ncd /d \"{home}\"\r\ncls\r\n",
        host = host,
        home = home.display(),
    )
}

/// Returns the static `dispatcher.bat` body.
///
/// The dispatcher is invoked by csshw with one argument: the
/// substituted `{{USERNAME_AT_HOST}}`, which is either `user@host`
/// (when csshw's username is set) or just `@host` (when it is not),
/// or just `host` (when the host arg already includes the user
/// prefix or no user is involved). The dispatcher normalises all
/// three to the bare host so we can keep fakehost directories
/// simply named (`alpha`, not `@alpha`).
///
/// Implementation note: we use cmd's `:*@=` substring substitution,
/// not `for /f tokens=2 delims=@`. The `for /f` form skips leading
/// delimiters - it parses `@alpha` as a single token (`alpha`), so
/// `tokens=2` matches nothing and `HOST` keeps its initial
/// `@alpha` value, leading to "the system cannot find the path
/// specified" when `call` falls through to a non-existent
/// `fakehosts\@alpha\enter.bat`. The substring form has no such
/// quirk: it strips through the first `@` if present, otherwise
/// leaves the value unchanged.
fn dispatcher_bat() -> &'static str {
    "@echo off\r\n\
     setlocal enabledelayedexpansion\r\n\
     set ARG=%~1\r\n\
     set HOST=!ARG!\r\n\
     if not \"!HOST:@=!\"==\"!HOST!\" set HOST=!HOST:*@=!\r\n\
     call \"%~dp0fakehosts\\!HOST!\\enter.bat\"\r\n"
}

/// Build the TOML body that overrides `[client]` to spawn cmd.exe via
/// the dispatcher. We leave `[daemon]` and `[clusters]` to csshw's
/// own defaults (the demo passes hosts on the command line).
fn render_toml(dispatcher: &Path) -> String {
    // Backslashes are doubled because TOML basic strings interpret
    // them as escapes. The dispatcher is the single entry point;
    // csshw substitutes `{{USERNAME_AT_HOST}}` as its argument.
    let dispatcher_str = dispatcher.display().to_string().replace('\\', "\\\\");
    format!(
        "# Auto-generated by `cargo xtask record-demo`. Do not commit.\n\
         [client]\n\
         ssh_config_path = \"\"\n\
         program = \"cmd.exe\"\n\
         arguments = [\"/k\", \"{dispatcher_str}\", \"{{{{USERNAME_AT_HOST}}}}\"]\n\
         username_host_placeholder = \"{{{{USERNAME_AT_HOST}}}}\"\n",
    )
}

#[cfg(test)]
#[path = "../tests/test_demo_config_override.rs"]
mod tests;
