//! README help-section verification and update logic.
//!
//! The README embeds the `--help` output between two HTML comment delimiters:
//!
//! ```text
//! <!-- HELP_OUTPUT_START -->
//! ```cmd
//! csshw.exe --help
//! <help content>
//! ```
//! <!-- HELP_OUTPUT_END -->
//! ```
//!
//! [`check_readme_help`] fails when the embedded text differs from the live
//! output. [`update_readme_help`] rewrites the README when they differ and
//! signals the change to the caller so a pre-commit hook can abort.

use anyhow::{bail, Context, Result};

const START_MARKER: &str = "<!-- HELP_OUTPUT_START -->";
const END_MARKER: &str = "<!-- HELP_OUTPUT_END -->";
const PREAMBLE: &str = "\r\n```cmd\r\ncsshw.exe --help\r\n";
const POSTAMBLE: &str = "\r\n```\r\n";

/// All side-effecting operations required by this module.
///
/// Implement with mocks in tests to achieve zero filesystem and process
/// side-effects.
pub trait ReadmeSystem {
    /// Run `cargo run --package csshw -- --help` and return the captured output.
    ///
    /// # Errors
    ///
    /// Returns an error if the process cannot be started or exits non-zero.
    fn get_help_output(&self) -> Result<String>;

    /// Read the full contents of `README.md`.
    ///
    /// # Errors
    ///
    /// Returns an error if the file cannot be read.
    fn read_readme(&self) -> Result<String>;

    /// Write `content` to `README.md`.
    ///
    /// # Errors
    ///
    /// Returns an error if the write fails.
    fn write_readme(&self, content: &str) -> Result<()>;
}

/// Production implementation of [`ReadmeSystem`].
pub struct RealSystem;

#[cfg_attr(coverage_nightly, coverage(off))]
impl ReadmeSystem for RealSystem {
    fn get_help_output(&self) -> Result<String> {
        let output = std::process::Command::new("cargo")
            .args(["run", "--package", "csshw", "--", "--help"])
            .output()
            .context("failed to run `cargo run --package csshw -- --help`")?;
        let raw = String::from_utf8_lossy(&output.stdout).into_owned();
        Ok(raw)
    }

    fn read_readme(&self) -> Result<String> {
        std::fs::read_to_string("README.md").context("failed to read README.md")
    }

    fn write_readme(&self, content: &str) -> Result<()> {
        std::fs::write("README.md", content).context("failed to write README.md")
    }
}

/// Normalize raw `--help` output for comparison with the README section.
///
/// Replaces lines that contain only whitespace with empty lines, normalizes
/// all line endings to `\r\n`, and trims leading and trailing whitespace.
///
/// # Arguments
///
/// * `raw` - Raw output from `--help`, possibly with mixed line endings.
///
/// # Returns
///
/// Normalized string ready for comparison with the README section.
///
pub fn normalize_help_output(raw: &str) -> String {
    let normalized: Vec<&str> = raw
        .lines()
        .map(|line| if line.trim().is_empty() { "" } else { line })
        .collect();
    let joined = normalized.join("\r\n");
    joined.trim().to_owned()
}

/// Extract the help text embedded in the README between the delimiters.
///
/// # Arguments
///
/// * `readme` - Full README contents.
///
/// # Returns
///
/// The help content string (trimmed), or an error if either delimiter is missing.
///
/// # Errors
///
/// Returns an error if `<!-- HELP_OUTPUT_START -->` or `<!-- HELP_OUTPUT_END -->`
/// is absent, or if the expected preamble/postamble structure is not found.
pub fn extract_readme_help_section(readme: &str) -> Result<&str> {
    let start_marker_pos = readme
        .find(START_MARKER)
        .context("could not find <!-- HELP_OUTPUT_START --> in README.md")?;
    let end_marker_pos = readme
        .find(END_MARKER)
        .context("could not find <!-- HELP_OUTPUT_END --> in README.md")?;

    let content_start = start_marker_pos + START_MARKER.len() + PREAMBLE.len();
    let content_end = end_marker_pos - POSTAMBLE.len();

    if content_start > content_end {
        bail!("README help section delimiters are malformed or out of order");
    }

    Ok(readme[content_start..content_end].trim())
}

/// Rebuild the README with the help section replaced by `new_help`.
///
/// All content outside the delimiters and the fixed preamble/postamble is
/// preserved byte-for-byte.
///
/// # Arguments
///
/// * `readme` - Full README contents.
/// * `new_help` - Normalized help text to embed.
///
/// # Returns
///
/// New full README string.
///
/// # Errors
///
/// Returns an error if the delimiters are not found.
pub fn replace_readme_help_section(readme: &str, new_help: &str) -> Result<String> {
    let start_marker_pos = readme
        .find(START_MARKER)
        .context("could not find <!-- HELP_OUTPUT_START --> in README.md")?;
    let end_marker_pos = readme
        .find(END_MARKER)
        .context("could not find <!-- HELP_OUTPUT_END --> in README.md")?;

    let content_start = start_marker_pos + START_MARKER.len() + PREAMBLE.len();
    let content_end = end_marker_pos - POSTAMBLE.len();

    let before = &readme[..content_start];
    let after = &readme[content_end..];

    Ok(format!("{before}{new_help}{after}"))
}

/// Compare the live `--help` output against the README's embedded help section.
///
/// Prints a colored diff to stdout when they differ.
///
/// # Arguments
///
/// * `system` - Injected I/O provider.
///
/// # Returns
///
/// `Ok(())` if they match; an error describing the mismatch otherwise.
///
/// # Errors
///
/// Returns an error when the sections differ or when any I/O operation fails.
pub fn check_readme_help<S: ReadmeSystem>(system: &S) -> Result<()> {
    let raw_help = system.get_help_output()?;
    let actual_help = normalize_help_output(&raw_help);

    let readme = system.read_readme()?;
    let readme_help = extract_readme_help_section(&readme)?;

    if actual_help == readme_help {
        println!("INFO - README.md help output is up to date.");
        return Ok(());
    }

    eprintln!("ERROR - README.md help output is outdated!");
    eprintln!();
    eprintln!("Differences found:");
    eprintln!("==================");
    eprintln!("README has:");
    eprintln!("{readme_help}");
    eprintln!();
    eprintln!("Current --help output:");
    eprintln!("{actual_help}");
    eprintln!();
    eprintln!("==> Run `cargo xtask update-readme-help` to fix this.");

    bail!("README.md help output is outdated")
}

/// Ensure the README's embedded help section matches the live `--help` output,
/// writing an updated README when they differ.
///
/// # Arguments
///
/// * `system` - Injected I/O provider.
///
/// # Returns
///
/// `Ok(true)` when the README was modified (the caller should exit with code 1
/// to abort a pre-commit hook); `Ok(false)` when already up to date.
///
/// # Errors
///
/// Returns an error when any I/O operation fails.
pub fn update_readme_help<S: ReadmeSystem>(system: &S) -> Result<bool> {
    let raw_help = system.get_help_output()?;
    let actual_help = normalize_help_output(&raw_help);

    let readme = system.read_readme()?;
    let readme_help = extract_readme_help_section(&readme)?;

    if actual_help == readme_help {
        println!("INFO - README.md help section is up to date, nothing to be done.");
        return Ok(false);
    }

    println!("WARNING - README.md help section is outdated — fixing it.");
    let new_readme = replace_readme_help_section(&readme, &actual_help)?;
    system.write_readme(&new_readme)?;
    println!("INFO - README.md help section has been updated with current --help output.");

    Ok(true)
}

#[cfg(test)]
#[path = "tests/test_readme.rs"]
mod tests;
