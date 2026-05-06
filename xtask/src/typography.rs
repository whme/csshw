//! Typography linter that blocks decorative or "smart" Unicode
//! punctuation from sneaking into the repository.
//!
//! Agents tend to introduce em-dashes, en-dashes, smart quotes,
//! ellipsis, arrows, and similar non-ASCII glyphs in comments and
//! prose. They look similar to their ASCII equivalents but are not
//! what a Windows developer types and not what `cargo fmt` produces.
//!
//! [`check_typography`] enumerates tracked text files via
//! `git ls-files`, scans each for a curated blocklist of code points,
//! prints any violations as `path:line:col U+XXXX 'glyph'`, and
//! returns an error when at least one violation is found so the
//! pre-commit hook and CI both abort.
//!
//! Performance: the scan runs inside the pre-commit hook, so the
//! hot path reads bytes, exits early on pure-ASCII input, and only
//! decodes UTF-8 for files that actually contain non-ASCII bytes.

use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};

/// File extensions whose contents are scanned.
///
/// All matching is done in lowercase. Files with no extension are
/// scanned only when their path matches [`SCAN_EXTRA_PATHS`].
const SCAN_EXTENSIONS: &[&str] = &[
    "rs", "md", "toml", "yml", "yaml", "json", "html", "txt", "cfg", "sh", "ps1",
];

/// Tracked paths without a recognised extension that should still be
/// scanned (shell scripts, hooks, etc.). Compared against the
/// `git ls-files` output verbatim (forward slashes).
const SCAN_EXTRA_PATHS: &[&str] = &[".githooks/pre-commit"];

/// Tracked paths that are explicitly excluded from scanning. Used for
/// generated artefacts such as `Cargo.lock` and for files (such as
/// the `CHANGELOG.md`) that may legitimately preserve historical
/// typography from prior releases.
///
/// Keep this list short -- the goal is to fix offending content, not
/// to allowlist around it. Compared against the `git ls-files` output
/// verbatim (forward slashes).
const ALLOWED_PATHS: &[&str] = &["Cargo.lock"];

/// Hard cap on file size accepted by the scanner. Anything larger is
/// skipped with a warning -- the repo has nothing close to this size,
/// and a pathological large file should not block a commit.
const MAX_FILE_BYTES: u64 = 5 * 1024 * 1024;

/// All side-effecting operations performed by the typography scanner.
///
/// Implement with mocks in tests to achieve zero filesystem and
/// process side-effects.
pub trait TypographySystem {
    /// Return the list of tracked files reported by `git ls-files`.
    ///
    /// Paths are returned with forward slashes (the format `git`
    /// emits on every platform).
    ///
    /// # Errors
    ///
    /// Returns an error if the `git` process cannot be started or
    /// exits non-zero.
    fn list_tracked_files(&self) -> Result<Vec<String>>;

    /// Return the size in bytes of the file at `path`.
    ///
    /// # Errors
    ///
    /// Returns an error if the file cannot be stat-ed.
    fn file_size(&self, path: &Path) -> Result<u64>;

    /// Read the full contents of the file at `path` as raw bytes.
    ///
    /// # Errors
    ///
    /// Returns an error if the file cannot be read.
    fn read_file(&self, path: &Path) -> Result<Vec<u8>>;

    /// Emit a message to the user (informational or warning).
    ///
    /// # Arguments
    ///
    /// * `msg` - Message to display.
    fn log(&self, msg: &str);
}

/// Production implementation of [`TypographySystem`].
pub struct RealSystem;

#[cfg_attr(coverage_nightly, coverage(off))]
impl TypographySystem for RealSystem {
    fn list_tracked_files(&self) -> Result<Vec<String>> {
        let output = std::process::Command::new("git")
            .args(["ls-files"])
            .output()
            .context("failed to run `git ls-files`")?;
        if !output.status.success() {
            bail!(
                "`git ls-files` exited non-zero: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }
        let stdout =
            String::from_utf8(output.stdout).context("`git ls-files` produced non-UTF-8 output")?;
        Ok(stdout
            .lines()
            .filter(|line| !line.is_empty())
            .map(|line| line.to_owned())
            .collect())
    }

    fn file_size(&self, path: &Path) -> Result<u64> {
        let meta = std::fs::metadata(path)
            .with_context(|| format!("failed to stat {}", path.display()))?;
        Ok(meta.len())
    }

    fn read_file(&self, path: &Path) -> Result<Vec<u8>> {
        std::fs::read(path).with_context(|| format!("failed to read {}", path.display()))
    }

    fn log(&self, msg: &str) {
        eprintln!("{msg}");
    }
}

/// A single offending code point found in a scanned file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Violation {
    /// Repository-relative path with forward slashes.
    pub path: String,
    /// 1-based line number of the offending character.
    pub line: u32,
    /// 1-based column (counted in `char`s, not bytes) of the offending
    /// character.
    pub column: u32,
    /// The offending Unicode scalar value.
    pub character: char,
}

/// Return `true` when `c` should be flagged by the scanner.
///
/// The blocklist is hand-curated to cover the decorative glyphs that
/// LLMs habitually substitute for ASCII punctuation. Emoji and other
/// non-ASCII characters are deliberately not included.
///
/// # Arguments
///
/// * `c` - Character to test.
///
/// # Returns
///
/// `true` when `c` is on the blocklist, `false` otherwise.
pub fn is_blocklisted(c: char) -> bool {
    let cp = c as u32;
    matches!(
        cp,
        // Non-breaking and middle-dot, multiplication, division.
        0x00A0 | 0x00B7 | 0x00D7 | 0x00F7
        // Exotic spaces.
        | 0x2000..=0x200B
        | 0x202F | 0x205F | 0x3000
        // Hyphens, en/em-dashes, horizontal bar, minus sign.
        | 0x2010..=0x2015 | 0x2212
        // Smart single and double quotes.
        | 0x2018..=0x201F
        // Bullet, ellipsis.
        | 0x2022 | 0x2026
        // Arrows block in its entirety.
        | 0x2190..=0x21FF
        // Math comparison glyphs.
        | 0x2248 | 0x2260 | 0x2264 | 0x2265
    )
}

/// Decide whether `path` should be scanned.
///
/// A file is scanned when:
///
/// 1. it is not in [`ALLOWED_PATHS`], and
/// 2. its lowercase extension is in [`SCAN_EXTENSIONS`], or its path
///    appears verbatim in [`SCAN_EXTRA_PATHS`].
///
/// # Arguments
///
/// * `path` - Forward-slash relative path as emitted by
///   `git ls-files`.
///
/// # Returns
///
/// `true` when the file should be scanned, `false` otherwise.
pub fn should_scan(path: &str) -> bool {
    if ALLOWED_PATHS.contains(&path) {
        return false;
    }
    if SCAN_EXTRA_PATHS.contains(&path) {
        return true;
    }
    let Some(dot) = path.rfind('.') else {
        return false;
    };
    let ext = &path[dot + 1..];
    SCAN_EXTENSIONS
        .iter()
        .any(|allowed| allowed.eq_ignore_ascii_case(ext))
}

/// Scan a single file's contents and return any violations.
///
/// Pure function -- no I/O. Files that are pure ASCII return early
/// before allocating or decoding UTF-8, which keeps the common case
/// (almost every `.rs` file in this repo) cheap.
///
/// Files that are not valid UTF-8 are reported via the returned
/// `non_utf8` flag and produce no violations; the caller decides
/// whether to surface that as a warning.
///
/// # Arguments
///
/// * `path` - Display path used when constructing violations.
/// * `bytes` - Raw file contents.
///
/// # Returns
///
/// `(violations, non_utf8)` where `non_utf8` is `true` if the file
/// could not be decoded as UTF-8.
pub fn scan_bytes(path: &str, bytes: &[u8]) -> (Vec<Violation>, bool) {
    // Fast path: pure ASCII -> nothing to flag.
    if bytes.iter().all(|&b| b < 0x80) {
        return (Vec::new(), false);
    }

    let Ok(text) = std::str::from_utf8(bytes) else {
        return (Vec::new(), true);
    };

    let mut violations = Vec::new();
    let mut line: u32 = 1;
    let mut column: u32 = 1;
    for c in text.chars() {
        if c == '\n' {
            line += 1;
            column = 1;
            continue;
        }
        if c == '\r' {
            // CRLF: do not advance the column. The following '\n' resets it.
            continue;
        }
        if is_blocklisted(c) {
            violations.push(Violation {
                path: path.to_owned(),
                line,
                column,
                character: c,
            });
        }
        column += 1;
    }
    (violations, false)
}

/// Scan every tracked text file and report violations.
///
/// Reads the file list via `git ls-files`, filters it through
/// [`should_scan`], and runs [`scan_bytes`] on each remaining file.
/// Violations are printed to stderr as
/// `path:line:col U+XXXX 'glyph'`.
///
/// # Arguments
///
/// * `system` - Injected I/O provider.
///
/// # Returns
///
/// `Ok(())` when no violations are found.
///
/// # Errors
///
/// Returns an error when at least one violation is found, or when an
/// I/O operation fails. Files that are too large or not valid UTF-8
/// are skipped with a warning and do not fail the run.
pub fn check_typography<S: TypographySystem>(system: &S) -> Result<()> {
    let files = system.list_tracked_files()?;
    let mut violations: Vec<Violation> = Vec::new();
    for rel in files {
        if !should_scan(&rel) {
            continue;
        }
        let path = PathBuf::from(&rel);
        let size = system.file_size(&path)?;
        if size > MAX_FILE_BYTES {
            system.log(&format!(
                "WARNING - skipping {rel}: {size} bytes exceeds {MAX_FILE_BYTES} byte cap"
            ));
            continue;
        }
        let bytes = system.read_file(&path)?;
        let (mut found, non_utf8) = scan_bytes(&rel, &bytes);
        if non_utf8 {
            system.log(&format!("WARNING - skipping {rel}: not valid UTF-8"));
            continue;
        }
        violations.append(&mut found);
    }

    if violations.is_empty() {
        println!("INFO - check-typography: no forbidden Unicode found.");
        return Ok(());
    }

    eprintln!(
        "ERROR - check-typography: found {} forbidden Unicode character(s).",
        violations.len()
    );
    eprintln!("        Replace them with their ASCII equivalents (em/en-dashes -> '-',");
    eprintln!("        smart quotes -> ' or \", ellipsis -> ..., arrows -> -> / <-, etc.).");
    eprintln!();
    for v in &violations {
        eprintln!(
            "{}:{}:{} U+{:04X} {:?}",
            v.path, v.line, v.column, v.character as u32, v.character
        );
    }
    bail!("found {} forbidden Unicode character(s)", violations.len())
}

#[cfg(test)]
#[path = "tests/test_typography.rs"]
mod tests;
