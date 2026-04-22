//! Local coverage report generation.
//!
//! Reads the pinned nightly toolchain from `nightly-toolchain.version`,
//! ensures it is installed, then runs `cargo-llvm-cov` to produce both
//! Cobertura XML and HTML coverage reports.
//!
//! [`run_coverage`] orchestrates the full workflow: toolchain check,
//! instrumented test run, and report generation.

use anyhow::{bail, Context, Result};

/// Filename regex passed to `--ignore-filename-regex` to exclude files
/// that cannot meaningfully be tested (entry point, debug helpers).
///
/// Keep in sync with `.github/workflows/_shared-ci.yml` `IGNORE_COVERAGE`.
const IGNORE_COVERAGE_REGEX: &str = r"((src[/\\]main\.rs$)|(src[/\\]utils[/\\]debug\.rs$))";

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
            IGNORE_COVERAGE_REGEX,
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
            IGNORE_COVERAGE_REGEX,
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
