//! Vendored binary management for the `record-demo` recorder.
//!
//! v0 expected `ffmpeg` and `gifski` on `PATH`. v1 ships SHA-pinned
//! download URLs for ffmpeg, gifski, and Carnac, fetches them once
//! into `target/demo/bin/<name>/`, verifies the SHA-256 of every
//! download against the constants in this module, and extracts the
//! archive into a deterministic on-disk layout that
//! [`crate::demo::recorder`] and the sandbox bootstrap can rely on.
//!
//! # Cache layout
//!
//! ```text
//! target/demo/bin/
//!   ffmpeg/<top>/bin/ffmpeg.exe        # Gyan ffmpeg essentials zip
//!   gifski/win/gifski.exe              # gifski release tar.xz
//!   carnac/lib/net45/Carnac.exe        # Carnac release zip (nested)
//! ```
//!
//! Where `<top>` is `ffmpeg-<version>-essentials_build`. The expected
//! relative paths inside each install are encoded in [`Pin::exe_rel`]
//! so a refresh that changes the upstream archive layout shows up as
//! a clear "binary missing after extract" error.
//!
//! # Pin refresh process
//!
//! 1. Download the new archive from the candidate URL.
//! 2. `Get-FileHash -Algorithm SHA256 <archive>` (PowerShell) and
//!    paste the lower-case hex digest into [`FFMPEG`], [`GIFSKI`],
//!    or [`CARNAC`].
//! 3. Bump the cached top-level directory name in [`Pin::cache_dir`]
//!    and (if the upstream layout changed) [`Pin::exe_rel`].
//! 4. Run `cargo xtask record-demo --env local --no-record` once on
//!    a clean checkout to confirm the cache populates without a
//!    SHA-mismatch error, then commit.
//!
//! All side effects (download, sha256, extract, fs) flow through the
//! [`DemoSystem`](crate::demo::DemoSystem) trait so unit tests
//! exercise this module against `mockall`-generated mocks with zero
//! network or filesystem effects.

use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};

use super::DemoSystem;

/// Pinned upstream archive plus the on-disk layout it expands into.
///
/// The pin set is hard-coded so a `cargo xtask record-demo` run that
/// hits a tampered network or stale CDN entry fails loudly with a
/// SHA mismatch instead of silently using a different binary.
pub struct Pin {
    /// Human-readable name used for the `target/demo/bin/<name>/`
    /// cache subdirectory and in log messages.
    pub name: &'static str,
    /// Direct download URL for the upstream release archive.
    pub url: &'static str,
    /// Expected lower-case hex SHA-256 digest of the archive.
    pub sha256: &'static str,
    /// File name to use for the downloaded archive (preserves the
    /// extension so [`DemoSystem::extract_archive`] can dispatch).
    pub archive_name: &'static str,
    /// Path to the extracted entry binary, expressed relative to
    /// `target/demo/bin/<name>/`. After
    /// [`ensure_pin`] returns, this path is guaranteed to exist.
    pub exe_rel: &'static str,
    /// Optional inner archive that must be extracted from the outer
    /// download to expose the entry binary. Used for Carnac, whose
    /// release zip wraps a NuGet package.
    pub inner_archive: Option<&'static str>,
}

impl Pin {
    /// Cache directory for this pin under
    /// `target/demo/bin/<name>/`.
    fn cache_dir(&self, bin_root: &Path) -> PathBuf {
        bin_root.join(self.name)
    }
}

/// FFmpeg pin. Gyan's "essentials" build is the standard Windows
/// distribution: a static build with the codecs we need
/// (ffvhuff for lossless capture; libx264 not used) and no shared
/// runtime DLL dependencies.
pub const FFMPEG: Pin = Pin {
    name: "ffmpeg",
    url: "https://github.com/GyanD/codexffmpeg/releases/download/8.1.1/ffmpeg-8.1.1-essentials_build.zip",
    sha256: "6f58ce889f59c311410f7d2b18895b33c03456463486f3b1ebc93d97a0f54541",
    archive_name: "ffmpeg-8.1.1-essentials_build.zip",
    exe_rel: "ffmpeg-8.1.1-essentials_build/bin/ffmpeg.exe",
    inner_archive: None,
};

/// gifski pin. Upstream ships a single tar.xz containing static
/// per-platform binaries; the Windows binary lives at `win/gifski.exe`
/// inside the archive.
pub const GIFSKI: Pin = Pin {
    name: "gifski",
    url: "https://github.com/ImageOptim/gifski/releases/download/1.34.0/gifski-1.34.0.tar.xz",
    sha256: "b9b6591aa163123d737353d9c8581efdf3234d28eeaa45329b31da905cd5a996",
    archive_name: "gifski-1.34.0.tar.xz",
    exe_rel: "win/gifski.exe",
    inner_archive: None,
};

/// Carnac pin. The MIT-licensed keystroke overlay used inside the
/// sandbox so the recording shows what keys the demo is sending.
/// The release zip wraps a NuGet package (Squirrel installer payload);
/// [`ensure_pin`] extracts the outer zip then the inner nupkg so the
/// final layout exposes `lib/net45/Carnac.exe` directly.
pub const CARNAC: Pin = Pin {
    name: "carnac",
    url: "https://github.com/Code52/carnac/releases/download/2.3.13/carnac.2.3.13.zip",
    sha256: "989819ac562c2d3dd717eca2fe41f264c23a929d4ab29a9777e9512811089117",
    archive_name: "carnac.2.3.13.zip",
    exe_rel: "lib/net45/Carnac.exe",
    inner_archive: Some("carnac-2.3.13-full.nupkg"),
};

/// Resolved paths to the cached vendored binaries used by the
/// recorder.
///
/// The Carnac binary is also downloaded by [`ensure_bins`] (it is
/// the keystroke overlay the sandbox bootstrap launches), but its
/// host-side path never crosses back into Rust: the sandbox
/// bootstrap script references it via the canonical sandbox-side
/// mount path. Same for the bin-root directory itself, which is
/// passed to the sandbox via `xtask/src/demo/env/sandbox.rs`'s
/// own layout struct.
#[derive(Debug, Clone)]
pub struct BinSet {
    /// Absolute path to ffmpeg.exe.
    pub ffmpeg: PathBuf,
    /// Absolute path to gifski.exe.
    pub gifski: PathBuf,
}

/// Ensure ffmpeg, gifski, and Carnac are present and SHA-verified
/// under `bin_root`.
///
/// On a cold cache the function downloads each archive, verifies its
/// SHA-256, and extracts it into the per-pin cache directory. On a
/// warm cache (entry binary already present) it returns immediately.
///
/// # Arguments
///
/// * `system` - injected I/O provider; mocked in tests.
/// * `bin_root` - cache root, normally
///   `<workspace>/target/demo/bin/`.
///
/// # Errors
///
/// Returns an error when a download fails, a SHA mismatches, an
/// archive cannot be extracted, or the expected entry binary is
/// missing after extraction.
pub fn ensure_bins<S: DemoSystem>(system: &S, bin_root: &Path) -> Result<BinSet> {
    system.ensure_dir(bin_root)?;
    let ffmpeg = ensure_pin(system, &FFMPEG, bin_root)?;
    let gifski = ensure_pin(system, &GIFSKI, bin_root)?;
    // Carnac is downloaded for the sandbox overlay but never
    // referenced from Rust; the bootstrap script uses its
    // canonical sandbox-side mount path.
    ensure_pin(system, &CARNAC, bin_root)?;
    Ok(BinSet { ffmpeg, gifski })
}

/// Materialise a single pin and return the absolute path to its
/// entry binary.
///
/// The fast path: if the entry binary already exists, return it
/// without contacting the network. The slow path: download to a
/// temporary `.archive` file alongside the cache dir, verify the
/// SHA, extract, then (for [`Pin::inner_archive`]) extract the inner
/// archive over the same destination.
pub fn ensure_pin<S: DemoSystem>(system: &S, pin: &Pin, bin_root: &Path) -> Result<PathBuf> {
    let cache = pin.cache_dir(bin_root);
    let exe = cache.join(pin.exe_rel);
    if system.path_exists(&exe) {
        system.print_debug(&format!("bin: {} cache hit at {}", pin.name, exe.display()));
        return Ok(exe);
    }
    system.ensure_dir(&cache)?;
    let archive = cache.join(pin.archive_name);
    system.print_info(&format!("bin: downloading {} from {}", pin.name, pin.url));
    system.http_download(pin.url, &archive)?;
    let actual = system
        .sha256_file(&archive)
        .with_context(|| format!("hashing {}", archive.display()))?;
    if !sha256_eq(&actual, pin.sha256) {
        bail!(
            "bin: SHA-256 mismatch for {} ({}): expected {}, got {}",
            pin.name,
            archive.display(),
            pin.sha256,
            actual
        );
    }
    system.print_debug(&format!(
        "bin: {} sha256 verified ({})",
        pin.name, pin.sha256
    ));
    system.extract_archive(&archive, &cache)?;
    if let Some(inner) = pin.inner_archive {
        let inner_path = cache.join(inner);
        if !system.path_exists(&inner_path) {
            bail!(
                "bin: inner archive {} missing after extracting {}",
                inner_path.display(),
                archive.display()
            );
        }
        system.extract_archive(&inner_path, &cache)?;
    }
    if !system.path_exists(&exe) {
        bail!(
            "bin: expected entry binary {} missing after extracting {}",
            exe.display(),
            pin.name
        );
    }
    Ok(exe)
}

/// Case-insensitive SHA-256 hex comparison. Pin constants are
/// committed lower-case but PowerShell's `Get-FileHash` returns
/// upper-case digests; tolerating either avoids a class of "wrong
/// case in the pin" foot-guns when refreshing the constants.
fn sha256_eq(a: &str, b: &str) -> bool {
    a.len() == b.len()
        && a.bytes()
            .zip(b.bytes())
            .all(|(x, y)| x.eq_ignore_ascii_case(&y))
}

#[cfg(test)]
#[path = "../tests/test_demo_bin.rs"]
mod tests;
