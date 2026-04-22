//! Local coverage report generation.
//!
//! The nightly toolchain is required because `#[coverage(off)]` — used to
//! exclude untestable code such as Windows API wrappers and production I/O
//! implementations from coverage — relies on the `coverage_attribute` feature,
//! which is only available on nightly Rust. Without it the `cfg(coverage_nightly)`
//! guards would not activate, causing those impls to be counted as missed lines
//! and distorting the report.
//!
//! The pinned toolchain version is read from `nightly-toolchain.version` and
//! the filename exclusion regex is read from `.coverage-ignore-regex`; both
//! files are shared with the CI workflow to keep the environments in sync.
//!
//! [`run_coverage`] orchestrates the full workflow: toolchain check,
//! instrumented test run, and report generation.

use anyhow::{bail, Context, Result};

/// All side-effecting operations required by this module.
///
/// Implement with mocks in tests to achieve zero filesystem, process,
/// and toolchain side-effects.
pub trait CoverageSystem {
    /// Read the contents of `nightly-toolchain.version` and return the
    /// trimmed toolchain identifier (e.g. `nightly-2026-04-20`).
    ///
    /// # Errors
    ///
    /// Returns an error if the file cannot be read.
    fn read_nightly_version_file(&self) -> Result<String>;

    /// Run `rustup toolchain list` and return its stdout.
    ///
    /// # Errors
    ///
    /// Returns an error if the process cannot be started.
    fn list_installed_toolchains(&self) -> Result<String>;

    /// Run `rustup toolchain install <toolchain> --component llvm-tools`.
    ///
    /// # Errors
    ///
    /// Returns an error if the install fails.
    fn install_toolchain(&self, toolchain: &str) -> Result<()>;

    /// Run a `cargo +<toolchain> llvm-cov` subcommand with the given arguments.
    ///
    /// # Errors
    ///
    /// Returns an error if the command fails.
    fn run_cargo_llvm_cov(&self, toolchain: &str, args: &[String]) -> Result<()>;

    /// Read the contents of `.coverage-ignore-regex` and return the trimmed
    /// filename regex pattern passed to `--ignore-filename-regex`.
    ///
    /// # Errors
    ///
    /// Returns an error if the file cannot be read.
    fn read_ignore_regex_file(&self) -> Result<String>;

    /// Print an informational message to stdout.
    fn print_info(&self, message: &str);
}

/// Production implementation of [`CoverageSystem`].
pub struct RealSystem;

#[cfg_attr(coverage_nightly, coverage(off))]
impl CoverageSystem for RealSystem {
    fn read_nightly_version_file(&self) -> Result<String> {
        std::fs::read_to_string("nightly-toolchain.version")
            .context("failed to read nightly-toolchain.version")
            .map(|s| s.trim().to_owned())
    }

    fn list_installed_toolchains(&self) -> Result<String> {
        let output = std::process::Command::new("rustup")
            .args(["toolchain", "list"])
            .output()
            .context("failed to run `rustup toolchain list`")?;
        Ok(String::from_utf8_lossy(&output.stdout).into_owned())
    }

    fn install_toolchain(&self, toolchain: &str) -> Result<()> {
        let status = std::process::Command::new("rustup")
            .args([
                "toolchain",
                "install",
                toolchain,
                "--component",
                "llvm-tools",
            ])
            .status()
            .context("failed to run `rustup toolchain install`")?;
        if !status.success() {
            bail!("`rustup toolchain install {toolchain}` failed with status {status}");
        }
        Ok(())
    }

    fn run_cargo_llvm_cov(&self, toolchain: &str, args: &[String]) -> Result<()> {
        let toolchain_arg = format!("+{toolchain}");
        let status = std::process::Command::new("cargo")
            .arg(&toolchain_arg)
            .arg("llvm-cov")
            .args(args)
            .status()
            .with_context(|| {
                format!(
                    "failed to run `cargo {toolchain_arg} llvm-cov {}`",
                    args.join(" ")
                )
            })?;
        if !status.success() {
            bail!(
                "`cargo {toolchain_arg} llvm-cov {}` failed with status {status}",
                args.join(" ")
            );
        }
        Ok(())
    }

    fn read_ignore_regex_file(&self) -> Result<String> {
        std::fs::read_to_string(".coverage-ignore-regex")
            .context("failed to read .coverage-ignore-regex")
            .map(|s| s.trim().to_owned())
    }

    fn print_info(&self, message: &str) {
        println!("INFO - {message}");
    }
}

/// Convert a slice of string literals to a `Vec<String>`.
fn args(values: &[&str]) -> Vec<String> {
    values.iter().map(|s| (*s).to_owned()).collect()
}

/// Generate coverage reports using the pinned nightly toolchain.
///
/// Reads the toolchain identifier from `nightly-toolchain.version`, ensures
/// it is installed, cleans stale coverage data, runs the test suite with
/// instrumentation, and produces Cobertura XML and HTML reports.
///
/// # Arguments
///
/// * `system` - Injected I/O provider.
///
/// # Errors
///
/// Returns an error if any step fails (missing version file, toolchain
/// install failure, test failure, or report generation failure).
pub fn run_coverage<S: CoverageSystem>(system: &S) -> Result<()> {
    let toolchain = system.read_nightly_version_file()?;
    system.print_info(&format!("Using nightly toolchain: {toolchain}"));
    let ignore_regex = system.read_ignore_regex_file()?;

    // Ensure toolchain is installed.
    let installed = system.list_installed_toolchains()?;
    if installed.lines().any(|line| line.starts_with(&toolchain)) {
        system.print_info("Toolchain already installed");
    } else {
        system.print_info(&format!("Installing toolchain: {toolchain}"));
        system.install_toolchain(&toolchain)?;
    }

    // Clean previous coverage data.
    system.print_info("Cleaning previous coverage data");
    system.run_cargo_llvm_cov(&toolchain, &args(&["clean", "--workspace"]))?;

    // Run tests with coverage instrumentation.
    system.print_info("Running tests with coverage");
    system.run_cargo_llvm_cov(
        &toolchain,
        &args(&[
            "--all-features",
            "--workspace",
            "--no-report",
            "--",
            "--no-capture",
        ]),
    )?;

    // Generate Cobertura XML report.
    system.print_info("Generating Cobertura XML report");
    system.run_cargo_llvm_cov(
        &toolchain,
        &args(&[
            "report",
            "--cobertura",
            "--output-path",
            "coverage.xml",
            "--ignore-filename-regex",
            &ignore_regex,
        ]),
    )?;

    // Generate HTML report.
    system.print_info("Generating HTML report");
    system.run_cargo_llvm_cov(
        &toolchain,
        &args(&[
            "report",
            "--html",
            "--output-dir",
            "coverage_html",
            "--ignore-filename-regex",
            &ignore_regex,
        ]),
    )?;

    system.print_info("Coverage reports generated:");
    system.print_info("  XML:  coverage.xml");
    system.print_info("  HTML: coverage_html/index.html");
    Ok(())
}

#[cfg(test)]
#[path = "tests/test_coverage.rs"]
mod tests;
