//! Social preview image generation.
//!
//! Orchestrates `docker run` against the pinned Playwright image to render
//! `templates/social-preview.html` into a 1280x640 PNG with live data
//! fetched from the GitHub API. The Rust side is a thin shell: all HTTP,
//! template substitution, and screenshotting live in
//! `xtask/social-preview/generate.mjs`, which runs inside the container.
//!
//! The host only needs Rust, Cargo, and Docker. No host-side Node.js, npm,
//! or Playwright installation is required.

use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};

/// Pinned Playwright Docker image tag.
///
/// The numeric portion (e.g. `v1.59.1`) must match `@playwright/test` in
/// `xtask/social-preview/package.json`. Playwright refuses to run when
/// these versions diverge, so bump both in the same commit. See
/// `xtask/social-preview/README.md` for details.
const PLAYWRIGHT_IMAGE: &str = "mcr.microsoft.com/playwright:v1.59.1-noble";

/// Default output path for the generated PNG, relative to the workspace
/// root. Lives under `target/` so it shares Cargo's build-artifact
/// directory and inherits its `.gitignore` entry.
const DEFAULT_OUT: &str = "target/social-preview/social-preview.png";

/// Container-side mount point for the workspace.
const CONTAINER_WORKSPACE: &str = "/workspace";

/// All side-effecting operations required by this module.
///
/// Implement with mocks in tests to achieve zero docker, filesystem,
/// process, and network side-effects.
pub trait SocialPreviewSystem {
    /// Return the absolute path to the workspace root (parent of `xtask/`).
    ///
    /// # Errors
    ///
    /// Returns an error if the workspace root cannot be resolved.
    fn workspace_root(&self) -> Result<PathBuf>;

    /// Read an environment variable, returning `None` when unset or empty.
    fn env_var(&self, key: &str) -> Option<String>;

    /// Ensure the parent directory of `path` exists, creating it (and any
    /// missing ancestors) if necessary.
    ///
    /// # Arguments
    ///
    /// * `path` - File path whose parent directory must exist.
    ///
    /// # Errors
    ///
    /// Returns an error if the directory cannot be created.
    fn ensure_parent_dir(&self, path: &Path) -> Result<()>;

    /// Verify that `docker` is installed on `PATH` and that its daemon is
    /// reachable. Called before any `docker run` invocation so the user
    /// gets a helpful message instead of a raw pipe/socket error.
    ///
    /// # Errors
    ///
    /// Returns an error describing whether the binary is missing or the
    /// daemon is not running.
    fn check_docker_ready(&self) -> Result<()>;

    /// Return `true` when `image` is already present in the local image
    /// cache (i.e. `docker image inspect <image>` succeeds).
    fn docker_image_exists(&self, image: &str) -> bool;

    /// Run `docker pull <image>` with inherited stdio so the user sees
    /// layer-download progress.
    ///
    /// # Errors
    ///
    /// Returns an error if `docker pull` exits with a non-zero status.
    fn docker_pull(&self, image: &str) -> Result<()>;

    /// Invoke `docker` with the given argument list and environment.
    ///
    /// # Arguments
    ///
    /// * `args` - Arguments passed to `docker` (starting with the
    ///   subcommand, e.g. `run`).
    /// * `envs` - Additional `(key, value)` environment variables applied
    ///   to the spawned `docker` process; these are forwarded to the
    ///   container via explicit `-e` flags built into `args`.
    ///
    /// # Errors
    ///
    /// Returns an error if the process cannot be started or exits with a
    /// non-zero status.
    fn run_docker(&self, args: &[String], envs: &[(String, String)]) -> Result<()>;

    /// Print an informational message to stdout.
    fn print_info(&self, message: &str);

    /// Print a debug-level message. Intended for low-level command
    /// traces (e.g. the exact `docker` invocation) that would be noisy by
    /// default but useful when troubleshooting. The production
    /// implementation only emits the message when `CSSHW_XTASK_VERBOSE`
    /// is set to a non-empty value.
    fn print_debug(&self, message: &str);
}

/// Production implementation of [`SocialPreviewSystem`].
pub struct RealSystem;

#[cfg_attr(coverage_nightly, coverage(off))]
impl SocialPreviewSystem for RealSystem {
    fn workspace_root(&self) -> Result<PathBuf> {
        // CARGO_MANIFEST_DIR is set by Cargo when building this binary; it
        // points at xtask/, whose parent is the workspace root.
        let manifest_dir = env!("CARGO_MANIFEST_DIR");
        let root = Path::new(manifest_dir)
            .parent()
            .context("failed to resolve workspace root from CARGO_MANIFEST_DIR")?
            .to_path_buf();
        Ok(root)
    }

    fn env_var(&self, key: &str) -> Option<String> {
        std::env::var(key).ok().filter(|v| !v.is_empty())
    }

    fn ensure_parent_dir(&self, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("failed to create directory {}", parent.display()))?;
        }
        Ok(())
    }

    fn check_docker_ready(&self) -> Result<()> {
        // `docker info` is cheap and exercises both the CLI resolution
        // path and a round-trip to the daemon socket.
        let output = match std::process::Command::new("docker")
            .args(["info", "--format", "{{.ServerVersion}}"])
            .output()
        {
            Ok(o) => o,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                bail!(
                    "`docker` was not found on PATH. Install Docker Desktop (or the Docker Engine) and ensure `docker` is on your PATH."
                );
            }
            Err(e) => {
                return Err(e).context("failed to spawn `docker info`");
            }
        };
        if output.status.success() && !output.stdout.is_empty() {
            return Ok(());
        }
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!(
            "Docker is installed but its daemon is not reachable. Start Docker Desktop (or your Docker daemon) and try again.\n  docker info stderr: {}",
            stderr.trim()
        );
    }

    fn docker_image_exists(&self, image: &str) -> bool {
        std::process::Command::new("docker")
            .args(["image", "inspect", image])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    }

    fn docker_pull(&self, image: &str) -> Result<()> {
        let status = std::process::Command::new("docker")
            .args(["pull", image])
            .status()
            .with_context(|| format!("failed to spawn `docker pull {image}`"))?;
        if !status.success() {
            bail!("`docker pull {image}` failed with status {status}");
        }
        Ok(())
    }

    fn run_docker(&self, args: &[String], envs: &[(String, String)]) -> Result<()> {
        let mut command = std::process::Command::new("docker");
        command.args(args);
        for (key, value) in envs {
            command.env(key, value);
        }
        let status = command
            .status()
            .context("failed to spawn `docker`; is Docker installed and on PATH?")?;
        if !status.success() {
            bail!("`docker {}` failed with status {status}", args.join(" "));
        }
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

/// Split the caller-supplied `--out` into (host-absolute path, workspace-
/// relative path with forward slashes).
///
/// Accepts any path. Relative paths resolve against the workspace root;
/// absolute paths are used as-is. Lexical `..` components are normalised
/// so inputs like `sub/../preview.png` are supported. The final resolved
/// path must still live under `workspace_root` so the container bind mount
/// can reach it at `/workspace/<rel>`; paths outside the workspace are
/// rejected with a clear error.
fn resolve_out_paths(workspace_root: &Path, out: Option<PathBuf>) -> Result<(PathBuf, String)> {
    let raw = out.unwrap_or_else(|| PathBuf::from(DEFAULT_OUT));
    let joined = if raw.is_absolute() {
        raw.clone()
    } else {
        workspace_root.join(&raw)
    };
    let normalised = normalise_path(&joined);
    let rel = normalised.strip_prefix(workspace_root).map_err(|_| {
        anyhow::anyhow!(
            "--out must resolve to a path inside the workspace root ({}); got {}",
            workspace_root.display(),
            raw.display()
        )
    })?;
    let rel_str = rel.to_string_lossy().replace('\\', "/");
    Ok((normalised.clone(), rel_str))
}

/// Lexically normalise a path by collapsing `.` and `..` components
/// without touching the filesystem. Behaves like `Path::canonicalize`
/// minus the requirement that the path exist. `..` at the root is
/// dropped (matching POSIX semantics).
fn normalise_path(path: &Path) -> PathBuf {
    use std::path::Component;
    let mut out = PathBuf::new();
    for comp in path.components() {
        match comp {
            Component::ParentDir => {
                // Only pop if the last pushed component is a regular
                // segment; otherwise drop (root `..`) or keep (leading
                // `..` on a relative path).
                let popped = match out.components().next_back() {
                    Some(Component::Normal(_)) => {
                        out.pop();
                        true
                    }
                    _ => false,
                };
                if !popped && !path.is_absolute() {
                    out.push("..");
                }
            }
            Component::CurDir => {}
            other => out.push(other.as_os_str()),
        }
    }
    out
}

/// Render a `docker` argument list as a single shell-quoted string, purely
/// for diagnostic logging. Arguments containing whitespace or shell
/// metacharacters are wrapped in single quotes; inner single quotes are
/// escaped as `'\''`. This is never re-parsed - it's only printed to
/// stdout so a user can copy-paste the exact invocation.
fn shell_quote_args(args: &[String]) -> String {
    args.iter()
        .map(|a| {
            if a.is_empty()
                || a.chars().any(|c| {
                    c.is_whitespace()
                        || matches!(
                            c,
                            '\'' | '"'
                                | '$'
                                | '`'
                                | '\\'
                                | '&'
                                | '|'
                                | ';'
                                | '<'
                                | '>'
                                | '('
                                | ')'
                                | '{'
                                | '}'
                                | '*'
                                | '?'
                                | '#'
                                | '!'
                                | '['
                                | ']'
                        )
                })
            {
                format!("'{}'", a.replace('\'', "'\\''"))
            } else {
                a.clone()
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Build the `docker run` argument list for the generator script.
fn build_docker_args(workspace_root: &Path, container_out: &str, has_token: bool) -> Vec<String> {
    let mount = format!(
        "{}:{CONTAINER_WORKSPACE}",
        workspace_root.to_string_lossy().replace('\\', "/")
    );
    let mut args: Vec<String> = vec![
        "run".into(),
        "--rm".into(),
        "-v".into(),
        mount,
        "-w".into(),
        CONTAINER_WORKSPACE.into(),
        "-e".into(),
        format!("OUT_PATH={container_out}"),
    ];
    if has_token {
        args.push("-e".into());
        args.push("GITHUB_TOKEN".into());
    }
    args.push(PLAYWRIGHT_IMAGE.into());
    args.push("sh".into());
    args.push("-c".into());
    // Install node_modules on first run, then invoke the generator. We
    // use `npm ci` (not `npm install`) so the install is strictly driven
    // by the committed `package-lock.json`; this keeps runs reproducible
    // and prevents the bind-mounted workspace from picking up lockfile
    // mutations. Subsequent runs skip the install entirely and stay
    // offline.
    //
    // The install runs in a subshell so it does not alter the CWD of the
    // subsequent `node` invocation. `generate.mjs` resolves its inputs
    // (template, logo, font, linguist colors) as workspace-relative paths,
    // so it must run from `/workspace` - not from
    // `/workspace/xtask/social-preview`.
    args.push(
        "( cd xtask/social-preview && { [ -d node_modules ] || npm ci; } ) && node xtask/social-preview/generate.mjs"
            .into(),
    );
    args
}

/// Generate the social preview PNG.
///
/// Resolves the output path, ensures the host-side output directory
/// exists, and invokes the Playwright Docker container which runs
/// `xtask/social-preview/generate.mjs` to fetch live GitHub data and
/// render `templates/social-preview.html` to PNG.
///
/// # Arguments
///
/// * `system` - Injected I/O provider.
/// * `out` - Optional output path override. Relative paths resolve against
///   the workspace root.
/// * `token` - Optional GitHub token override. Falls back to the
///   `GITHUB_TOKEN` environment variable, then unauthenticated access.
///
/// # Errors
///
/// Returns an error if the workspace root cannot be resolved, the output
/// directory cannot be created, or the `docker run` invocation fails.
pub fn generate_social_preview<S: SocialPreviewSystem>(
    system: &S,
    out: Option<PathBuf>,
    token: Option<String>,
) -> Result<()> {
    let workspace_root = system.workspace_root()?;
    let (host_out, relative_out) = resolve_out_paths(&workspace_root, out)?;
    system.print_info(&format!(
        "Generating social preview -> {}",
        host_out.display()
    ));
    system.check_docker_ready()?;
    if !system.docker_image_exists(PLAYWRIGHT_IMAGE) {
        system.print_info(&format!(
            "Pulling Playwright image {PLAYWRIGHT_IMAGE} (first run only)"
        ));
        system.docker_pull(PLAYWRIGHT_IMAGE)?;
    }
    system.ensure_parent_dir(&host_out)?;

    let container_out = format!("{CONTAINER_WORKSPACE}/{relative_out}");
    let resolved_token = token.or_else(|| system.env_var("GITHUB_TOKEN"));
    let has_token = resolved_token.is_some();

    let args = build_docker_args(&workspace_root, &container_out, has_token);
    let envs: Vec<(String, String)> = resolved_token
        .into_iter()
        .map(|t| ("GITHUB_TOKEN".to_owned(), t))
        .collect();

    system.print_info(&format!("Starting Playwright container {PLAYWRIGHT_IMAGE}"));
    system.print_debug(&format!("+ docker {}", shell_quote_args(&args)));
    system.run_docker(&args, &envs)?;
    system.print_info(&format!("Wrote {}", host_out.display()));
    Ok(())
}

#[cfg(test)]
#[path = "tests/test_social_preview.rs"]
mod tests;
