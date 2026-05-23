//! Paseo agent GitHub auth injection.
//!
//! A paseo-spawned agent would otherwise inherit the user's full `gh`
//! login - including classic scopes like `repo` that allow deleting
//! repositories or force-pushing to `main`. This module is the
//! counterpart of that risk: on worktree creation, it writes a
//! per-worktree `.claude/settings.local.json` whose `env` map carries
//! a fine-grained PAT supplied by the contributor. Claude Code
//! injects that `env` into the agent process, and `gh` honors
//! `GH_TOKEN` over the keyring, so the agent ends up acting as the
//! scoped PAT while the contributor's own `gh` session outside paseo
//! is unaffected.
//!
//! The token source is `<source-checkout>/.paseo/gh-token` - a
//! gitignored file the contributor creates once per clone. The
//! source checkout path is taken from the `PASEO_SOURCE_CHECKOUT_PATH`
//! environment variable paseo sets when running setup steps; if that
//! variable is absent, the current directory is used instead, which
//! covers manual `cargo xtask inject-agent-token` invocations from
//! the repo root.
//!
//! If the token file is missing the subcommand is a silent no-op
//! (with an informational log line). Fine-grained PATs
//! (`github_pat_...`) are recommended because they can be restricted
//! to specific repositories and to a subset of repository permissions.
//! Classic (`ghp_...`) and OAuth (`gho_...`) tokens are accepted to
//! avoid hard-blocking contributors who only have those, but each
//! triggers a warning log line since they cannot be scoped tightly
//! enough to preserve the least-privilege property. Any other content
//! is rejected so we never inject arbitrary text as a token.

use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};

/// Prefix for a fine-grained personal access token. This is the
/// recommended token shape because it can be restricted to specific
/// repositories and to a subset of repository permissions.
const FINE_GRAINED_PREFIX: &str = "github_pat_";

/// Prefix for a classic personal access token. Accepted to avoid
/// hard-blocking contributors who only have a classic token, but
/// flagged with a warning since classic tokens cannot be scoped to
/// specific repositories or to a subset of repository permissions.
const CLASSIC_PREFIX: &str = "ghp_";

/// Prefix for an OAuth user-to-server token. Accepted with the same
/// caveat as [`CLASSIC_PREFIX`].
const OAUTH_PREFIX: &str = "gho_";

/// Relative path inside the source checkout where the contributor
/// stores their GitHub token.
const TOKEN_FILE_REL_PATH: &str = ".paseo/gh-token";

/// Relative path inside the worktree where Claude Code reads local,
/// uncommitted per-project settings.
const SETTINGS_FILE_REL_PATH: &str = ".claude/settings.local.json";

/// All side-effecting operations performed by this subcommand.
///
/// Implement with mocks in tests to achieve zero filesystem,
/// environment, or process side-effects.
pub trait InjectAgentTokenSystem {
    /// Look up an environment variable.
    ///
    /// # Arguments
    ///
    /// * `key` - Environment variable name.
    ///
    /// # Returns
    ///
    /// `Some(value)` when the variable is set and non-empty,
    /// `None` otherwise.
    fn env_var(&self, key: &str) -> Option<String>;

    /// Return the current working directory.
    ///
    /// # Errors
    ///
    /// Returns an error if the current directory cannot be
    /// determined.
    fn current_dir(&self) -> Result<PathBuf>;

    /// Read the token file at `path`.
    ///
    /// # Arguments
    ///
    /// * `path` - Absolute or worktree-relative path to the token
    ///   file.
    ///
    /// # Returns
    ///
    /// `Ok(Some(contents))` when the file exists and is readable,
    /// `Ok(None)` when it does not exist (the subcommand treats
    /// this as a no-op).
    ///
    /// # Errors
    ///
    /// Returns an error for filesystem failures other than
    /// "not found" (for example, permission denied).
    fn read_token_file(&self, path: &Path) -> Result<Option<String>>;

    /// Write `contents` to the settings file at `path`, creating
    /// any missing parent directories.
    ///
    /// # Arguments
    ///
    /// * `path` - Target path for the settings file.
    /// * `contents` - Full file contents to write.
    ///
    /// # Errors
    ///
    /// Returns an error if directory creation or the write fails.
    fn write_settings(&self, path: &Path, contents: &str) -> Result<()>;

    /// Emit an informational or warning message to the user.
    ///
    /// # Arguments
    ///
    /// * `msg` - Message to display.
    fn log(&self, msg: &str);
}

/// Production implementation of [`InjectAgentTokenSystem`].
pub struct RealSystem;

#[cfg_attr(coverage_nightly, coverage(off))]
impl InjectAgentTokenSystem for RealSystem {
    fn env_var(&self, key: &str) -> Option<String> {
        std::env::var(key).ok().filter(|v| !v.is_empty())
    }

    fn current_dir(&self) -> Result<PathBuf> {
        std::env::current_dir().context("failed to resolve current directory")
    }

    fn read_token_file(&self, path: &Path) -> Result<Option<String>> {
        match std::fs::read_to_string(path) {
            Ok(contents) => Ok(Some(contents)),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(err) => Err(err).with_context(|| format!("failed to read {}", path.display())),
        }
    }

    fn write_settings(&self, path: &Path, contents: &str) -> Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("failed to create {}", parent.display()))?;
        }
        std::fs::write(path, contents)
            .with_context(|| format!("failed to write {}", path.display()))?;
        Ok(())
    }

    fn log(&self, msg: &str) {
        println!("{msg}");
    }
}

/// Build the JSON body written to `.claude/settings.local.json`.
///
/// Caller-enforced invariant: `token` contains only bytes in
/// `[A-Za-z0-9_]`. That alphabet has no characters that require JSON
/// escaping, which is what lets this function skip a general-purpose
/// JSON encoder without risking injection. The invariant is enforced
/// by [`is_in_token_alphabet`] inside [`inject_agent_token`].
///
/// # Arguments
///
/// * `token` - GitHub token, already validated and trimmed.
///
/// # Returns
///
/// A pretty-printed JSON document terminated with a newline.
fn build_settings_body(token: &str) -> String {
    format!(
        "{{\n  \"env\": {{\n    \"GH_TOKEN\": \"{token}\",\n    \"GH_HOST\": \"github.com\"\n  }}\n}}\n"
    )
}

/// Return `true` when every byte of `token` is in the GitHub token
/// alphabet `[A-Za-z0-9_]`.
///
/// Enforcing this invariant is what lets [`build_settings_body`]
/// embed the token directly into a JSON template without escaping -
/// none of the characters in this alphabet need JSON escaping, so a
/// token that passes this check cannot break out of its string
/// literal nor inject additional keys. Fine-grained PATs, classic
/// PATs, and OAuth tokens all share the same alphabet, so the same
/// check applies to every accepted token shape.
///
/// # Arguments
///
/// * `token` - Trimmed token to validate.
///
/// # Returns
///
/// `true` when `token` is non-empty and contains only the allowed
/// characters; `false` otherwise.
fn is_in_token_alphabet(token: &str) -> bool {
    !token.is_empty()
        && token
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_')
}

/// Recognized GitHub token shapes.
#[derive(Clone, Copy)]
enum TokenKind {
    FineGrained,
    Classic,
    OAuth,
}

impl TokenKind {
    /// Identify the token shape from its prefix.
    ///
    /// # Arguments
    ///
    /// * `token` - Trimmed token contents.
    ///
    /// # Returns
    ///
    /// `Some(kind)` when the token starts with a recognized prefix,
    /// `None` otherwise.
    fn classify(token: &str) -> Option<Self> {
        if token.starts_with(FINE_GRAINED_PREFIX) {
            Some(Self::FineGrained)
        } else if token.starts_with(CLASSIC_PREFIX) {
            Some(Self::Classic)
        } else if token.starts_with(OAUTH_PREFIX) {
            Some(Self::OAuth)
        } else {
            None
        }
    }
}

/// Resolve the source checkout directory.
///
/// Paseo passes `PASEO_SOURCE_CHECKOUT_PATH` into `worktree.setup`
/// subprocesses. When the variable is missing - for example when the
/// subcommand is invoked manually - fall back to the current
/// directory so running it from the repo root behaves intuitively.
///
/// # Arguments
///
/// * `system` - Injected I/O provider.
///
/// # Returns
///
/// The source checkout path.
///
/// # Errors
///
/// Returns an error only when the fallback `current_dir` lookup
/// fails.
fn resolve_source_checkout<S: InjectAgentTokenSystem>(system: &S) -> Result<PathBuf> {
    if let Some(path) = system.env_var("PASEO_SOURCE_CHECKOUT_PATH") {
        return Ok(PathBuf::from(path));
    }
    system.current_dir()
}

/// Inject the contributor's GitHub token into the current worktree's
/// Claude Code settings.
///
/// The token is read from `<source-checkout>/.paseo/gh-token`. A
/// missing token file is treated as an opt-out: the function logs a
/// notice and returns `Ok(())` so worktree creation is not blocked
/// for contributors who have not set a token up yet. Fine-grained
/// PATs are written silently; classic and OAuth tokens are written
/// but trigger a warning log line recommending fine-grained PATs.
///
/// # Arguments
///
/// * `system` - Injected I/O provider.
///
/// # Returns
///
/// `Ok(())` on success or when the token file is absent.
///
/// # Errors
///
/// Returns an error when a token file exists but does not start with
/// one of the recognized prefixes ([`FINE_GRAINED_PREFIX`],
/// [`CLASSIC_PREFIX`], [`OAUTH_PREFIX`]), when its trimmed contents
/// fall outside the token alphabet (see [`is_in_token_alphabet`]),
/// or when the settings file cannot be written.
pub fn inject_agent_token<S: InjectAgentTokenSystem>(system: &S) -> Result<()> {
    let source = resolve_source_checkout(system)?;
    let token_file = source.join(TOKEN_FILE_REL_PATH);

    let Some(raw) = system.read_token_file(&token_file)? else {
        system.log(&format!(
            "INFO - paseo agent GitHub auth: no {} found; agents will use your existing gh login. See CONTRIBUTING.md.",
            token_file.display()
        ));
        return Ok(());
    };

    let token = raw.trim();
    if token.is_empty() {
        bail!(
            "{} is empty; expected a GitHub token starting with `{}` (recommended), `{}`, or `{}`. See CONTRIBUTING.md.",
            token_file.display(),
            FINE_GRAINED_PREFIX,
            CLASSIC_PREFIX,
            OAUTH_PREFIX,
        );
    }
    let Some(kind) = TokenKind::classify(token) else {
        bail!(
            "{} must contain a GitHub token starting with `{}` (recommended), `{}`, or `{}`. See CONTRIBUTING.md.",
            token_file.display(),
            FINE_GRAINED_PREFIX,
            CLASSIC_PREFIX,
            OAUTH_PREFIX,
        );
    };
    if !is_in_token_alphabet(token) {
        bail!(
            "{} contains characters outside the GitHub token alphabet ([A-Za-z0-9_]); refusing to embed it in settings. See CONTRIBUTING.md.",
            token_file.display()
        );
    }

    let cwd = system.current_dir()?;
    let settings_path = cwd.join(SETTINGS_FILE_REL_PATH);
    let body = build_settings_body(token);
    system.write_settings(&settings_path, &body)?;

    match kind {
        TokenKind::FineGrained => {
            system.log(&format!(
                "INFO - paseo agent GitHub auth: wrote {} from {} (scoped PAT)",
                settings_path.display(),
                token_file.display()
            ));
        }
        TokenKind::Classic => {
            system.log(&format!(
                "WARN - paseo agent GitHub auth: detected a classic token in {}; wrote {} but fine-grained PATs (prefix `{}`) are recommended because they can be restricted to specific repositories and permissions, while classic tokens cannot. See CONTRIBUTING.md.",
                token_file.display(),
                settings_path.display(),
                FINE_GRAINED_PREFIX,
            ));
        }
        TokenKind::OAuth => {
            system.log(&format!(
                "WARN - paseo agent GitHub auth: detected an OAuth token in {}; wrote {} but fine-grained PATs (prefix `{}`) are recommended because they can be restricted to specific repositories and permissions, while OAuth tokens cannot. See CONTRIBUTING.md.",
                token_file.display(),
                settings_path.display(),
                FINE_GRAINED_PREFIX,
            ));
        }
    }

    Ok(())
}

#[cfg(test)]
#[path = "tests/test_inject_agent_token.rs"]
mod tests;
