//! Changelog generation via the external `changelogging` tool.
//!
//! Reads the current version from `Cargo.toml`, synchronises it into
//! `changelogging.toml`, then invokes `changelogging build --remove` to
//! consume news fragments and append an entry to `CHANGELOG.md`.

use anyhow::{Context, Result};

/// All side-effecting operations required by this module.
///
/// Implement with mocks in tests to achieve zero filesystem and process
/// side-effects.
pub trait ChangelogSystem {
    /// Read the contents of `Cargo.toml`.
    ///
    /// # Errors
    ///
    /// Returns an error if the file cannot be read.
    fn read_cargo_toml(&self) -> Result<String>;

    /// Read the contents of `changelogging.toml`.
    ///
    /// # Errors
    ///
    /// Returns an error if the file cannot be read.
    fn read_changelogging_toml(&self) -> Result<String>;

    /// Write `content` to `changelogging.toml`.
    ///
    /// # Errors
    ///
    /// Returns an error if the write fails.
    fn write_changelogging_toml(&self, content: &str) -> Result<()>;

    /// Run `changelogging build --remove` to generate `CHANGELOG.md`.
    ///
    /// # Errors
    ///
    /// Returns an error if the process cannot be started or exits non-zero.
    fn run_changelogging_build(&self) -> Result<()>;
}

/// Production implementation of [`ChangelogSystem`].
pub struct RealSystem;

impl ChangelogSystem for RealSystem {
    fn read_cargo_toml(&self) -> Result<String> {
        std::fs::read_to_string("Cargo.toml").context("failed to read Cargo.toml")
    }

    fn read_changelogging_toml(&self) -> Result<String> {
        std::fs::read_to_string("changelogging.toml").context("failed to read changelogging.toml")
    }

    fn write_changelogging_toml(&self, content: &str) -> Result<()> {
        std::fs::write("changelogging.toml", content).context("failed to write changelogging.toml")
    }

    fn run_changelogging_build(&self) -> Result<()> {
        let status = std::process::Command::new("changelogging")
            .args(["build", "--remove"])
            .status()
            .context("failed to run `changelogging build --remove`")?;
        if !status.success() {
            anyhow::bail!("`changelogging build --remove` exited with status {status}");
        }
        Ok(())
    }
}

/// Extract the `[package].version` value from a `Cargo.toml` string.
///
/// # Arguments
///
/// * `cargo_toml_content` - Raw TOML text of `Cargo.toml`.
///
/// # Returns
///
/// The version string (e.g. `"0.18.1"`).
///
/// # Errors
///
/// Returns an error if the content cannot be parsed as TOML or the
/// `[package].version` key is absent.
pub fn extract_version_from_cargo_toml(cargo_toml_content: &str) -> Result<String> {
    let doc: toml_edit::Document = cargo_toml_content
        .parse()
        .context("failed to parse Cargo.toml")?;
    let version = doc
        .get("package")
        .and_then(|p| p.as_table())
        .and_then(|t| t.get("version"))
        .and_then(|v| v.as_str())
        .context("missing [package].version in Cargo.toml")?;
    Ok(version.to_owned())
}

/// Set `context.version` in a `changelogging.toml` document to `version`.
///
/// All other keys and formatting are preserved via `toml_edit`.
///
/// # Arguments
///
/// * `changelogging_content` - Raw TOML text of `changelogging.toml`.
/// * `version` - Version string to write.
///
/// # Returns
///
/// Updated TOML text.
///
/// # Errors
///
/// Returns an error if `changelogging_content` cannot be parsed as TOML.
pub fn set_changelogging_version(changelogging_content: &str, version: &str) -> Result<String> {
    let mut doc: toml_edit::Document = changelogging_content
        .parse()
        .context("failed to parse changelogging.toml")?;
    doc["context"]["version"] = toml_edit::value(version);
    Ok(doc.to_string())
}

/// Generate the changelog for the version currently recorded in `Cargo.toml`.
///
/// 1. Reads the version from `Cargo.toml`.
/// 2. Rewrites `changelogging.toml` with the new version.
/// 3. Runs `changelogging build --remove`.
///
/// # Arguments
///
/// * `system` - Injected I/O provider.
///
/// # Errors
///
/// Returns an error if any step fails.
pub fn generate_changelog<S: ChangelogSystem>(system: &S) -> Result<()> {
    let cargo_toml = system.read_cargo_toml()?;
    let version = extract_version_from_cargo_toml(&cargo_toml)?;
    println!("Generating changelog for version {version}");

    let changelogging_toml = system.read_changelogging_toml()?;
    let updated = set_changelogging_version(&changelogging_toml, &version)?;
    system.write_changelogging_toml(&updated)?;

    system.run_changelogging_build()?;
    Ok(())
}

#[cfg(test)]
#[path = "tests/test_changelog.rs"]
mod tests;
