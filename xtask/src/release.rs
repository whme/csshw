//! Release preparation and git tag creation.
//!
//! [`prepare_release`] bumps the version, optionally creates a maintenance
//! branch, updates `Cargo.toml` and `Cargo.lock`, generates the changelog,
//! commits, and pushes.
//!
//! [`create_release_tag`] validates the current state and creates an annotated
//! git tag that triggers the GitHub Actions release workflow.

use anyhow::{bail, Context, Result};
use semver::Version;

/// Type of version increment for a release.
#[derive(Debug, PartialEq)]
pub enum ReleaseType {
    /// Increment the major component (X.0.0).
    Major,
    /// Increment the minor component (0.X.0).
    Minor,
    /// Increment the patch component (0.0.X).
    Patch,
}

impl std::fmt::Display for ReleaseType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ReleaseType::Major => write!(f, "major"),
            ReleaseType::Minor => write!(f, "minor"),
            ReleaseType::Patch => write!(f, "patch"),
        }
    }
}

/// All side-effecting operations required by this module.
///
/// Each method maps to exactly one external operation, making every step
/// independently mockable in tests.
pub trait ReleaseSystem {
    /// Run `git status --porcelain` and return its stdout.
    ///
    /// # Errors
    ///
    /// Returns an error if the process fails.
    fn git_status_porcelain(&self) -> Result<String>;

    /// Return the current git branch name.
    ///
    /// # Errors
    ///
    /// Returns an error if the process fails.
    fn git_current_branch(&self) -> Result<String>;

    /// Create and switch to a new branch with `git checkout -b <name>`.
    ///
    /// # Arguments
    ///
    /// * `name` - Branch name to create.
    ///
    /// # Errors
    ///
    /// Returns an error if the process fails.
    fn git_checkout_new_branch(&self, name: &str) -> Result<()>;

    /// Stage the given files with `git add`.
    ///
    /// # Arguments
    ///
    /// * `files` - Paths to stage.
    ///
    /// # Errors
    ///
    /// Returns an error if the process fails.
    fn git_add(&self, files: &[String]) -> Result<()>;

    /// Commit staged changes with the given message.
    ///
    /// # Arguments
    ///
    /// * `message` - Commit message.
    ///
    /// # Errors
    ///
    /// Returns an error if the process fails.
    fn git_commit(&self, message: &str) -> Result<()>;

    /// Run `git push` with the given extra arguments.
    ///
    /// # Arguments
    ///
    /// * `args` - Extra arguments appended to `git push`.
    ///
    /// # Errors
    ///
    /// Returns an error if the process fails.
    fn git_push(&self, args: &[String]) -> Result<()>;

    /// Return `git tag -l <tag>` stdout for the given tag name.
    ///
    /// # Arguments
    ///
    /// * `tag` - Tag name to check.
    ///
    /// # Errors
    ///
    /// Returns an error if the process fails.
    fn git_tag_list(&self, tag: &str) -> Result<String>;

    /// Return the subject of the latest commit (`git log -1 --pretty=format:%s`).
    ///
    /// # Errors
    ///
    /// Returns an error if the process fails.
    fn git_log_latest_subject(&self) -> Result<String>;

    /// Run `git fetch`.
    ///
    /// # Errors
    ///
    /// Returns an error if the process fails (non-fatal; callers may continue).
    fn git_fetch(&self) -> Result<()>;

    /// Return the number of commits the local branch is behind `<branch>` on
    /// the remote.
    ///
    /// # Arguments
    ///
    /// * `branch` - Remote branch to compare against.
    ///
    /// # Errors
    ///
    /// Returns an error if the process fails.
    fn git_rev_list_count_behind(&self, branch: &str) -> Result<u32>;

    /// Create an annotated git tag.
    ///
    /// # Arguments
    ///
    /// * `tag` - Tag name.
    /// * `message` - Annotation message.
    ///
    /// # Errors
    ///
    /// Returns an error if the process fails.
    fn git_create_annotated_tag(&self, tag: &str, message: &str) -> Result<()>;

    /// Push a tag to `origin`.
    ///
    /// # Arguments
    ///
    /// * `tag` - Tag name to push.
    ///
    /// # Errors
    ///
    /// Returns an error if the process fails.
    fn git_push_tag(&self, tag: &str) -> Result<()>;

    /// Read the contents of `Cargo.toml`.
    ///
    /// # Errors
    ///
    /// Returns an error if the file cannot be read.
    fn read_cargo_toml(&self) -> Result<String>;

    /// Write `content` to `Cargo.toml`.
    ///
    /// # Errors
    ///
    /// Returns an error if the write fails.
    fn write_cargo_toml(&self, content: &str) -> Result<()>;

    /// Run `cargo update --workspace` to refresh `Cargo.lock`.
    ///
    /// # Errors
    ///
    /// Returns an error if the process fails.
    fn cargo_update_workspace(&self) -> Result<()>;

    /// Generate the changelog for the current version.
    ///
    /// # Errors
    ///
    /// Returns an error if changelog generation fails.
    fn generate_changelog(&self) -> Result<()>;

    /// Display `message` and read a line of user input.
    ///
    /// # Arguments
    ///
    /// * `message` - Prompt text.
    ///
    /// # Returns
    ///
    /// The trimmed response string.
    ///
    /// # Errors
    ///
    /// Returns an error if stdin cannot be read.
    fn prompt_user(&self, message: &str) -> Result<String>;
}

/// Production implementation of [`ReleaseSystem`].
pub struct RealSystem;

impl ReleaseSystem for RealSystem {
    fn git_status_porcelain(&self) -> Result<String> {
        let output = std::process::Command::new("git")
            .args(["status", "--porcelain"])
            .output()
            .context("failed to run `git status --porcelain`")?;
        Ok(String::from_utf8_lossy(&output.stdout).into_owned())
    }

    fn git_current_branch(&self) -> Result<String> {
        let output = std::process::Command::new("git")
            .args(["branch", "--show-current"])
            .output()
            .context("failed to run `git branch --show-current`")?;
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
    }

    fn git_checkout_new_branch(&self, name: &str) -> Result<()> {
        let status = std::process::Command::new("git")
            .args(["checkout", "-b", name])
            .status()
            .context("failed to run `git checkout -b`")?;
        if !status.success() {
            bail!("`git checkout -b {name}` failed with status {status}");
        }
        Ok(())
    }

    fn git_add(&self, files: &[String]) -> Result<()> {
        let status = std::process::Command::new("git")
            .arg("add")
            .args(files)
            .status()
            .context("failed to run `git add`")?;
        if !status.success() {
            bail!("`git add` failed with status {status}");
        }
        Ok(())
    }

    fn git_commit(&self, message: &str) -> Result<()> {
        let status = std::process::Command::new("git")
            .args(["commit", "-m", message])
            .status()
            .context("failed to run `git commit`")?;
        if !status.success() {
            bail!("`git commit` failed with status {status}");
        }
        Ok(())
    }

    fn git_push(&self, args: &[String]) -> Result<()> {
        let status = std::process::Command::new("git")
            .arg("push")
            .args(args)
            .status()
            .context("failed to run `git push`")?;
        if !status.success() {
            bail!("`git push` failed with status {status}");
        }
        Ok(())
    }

    fn git_tag_list(&self, tag: &str) -> Result<String> {
        let output = std::process::Command::new("git")
            .args(["tag", "-l", tag])
            .output()
            .context("failed to run `git tag -l`")?;
        Ok(String::from_utf8_lossy(&output.stdout).into_owned())
    }

    fn git_log_latest_subject(&self) -> Result<String> {
        let output = std::process::Command::new("git")
            .args(["log", "-1", "--pretty=format:%s"])
            .output()
            .context("failed to run `git log`")?;
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
    }

    fn git_fetch(&self) -> Result<()> {
        let status = std::process::Command::new("git")
            .arg("fetch")
            .status()
            .context("failed to run `git fetch`")?;
        if !status.success() {
            bail!("`git fetch` failed with status {status}");
        }
        Ok(())
    }

    fn git_rev_list_count_behind(&self, branch: &str) -> Result<u32> {
        let output = std::process::Command::new("git")
            .args(["rev-list", "--count", &format!("HEAD..origin/{branch}")])
            .output()
            .context("failed to run `git rev-list`")?;
        let count = String::from_utf8_lossy(&output.stdout)
            .trim()
            .parse::<u32>()
            .unwrap_or(0);
        Ok(count)
    }

    fn git_create_annotated_tag(&self, tag: &str, message: &str) -> Result<()> {
        let status = std::process::Command::new("git")
            .args(["tag", "-a", tag, "-m", message])
            .status()
            .context("failed to run `git tag -a`")?;
        if !status.success() {
            bail!("`git tag -a {tag}` failed with status {status}");
        }
        Ok(())
    }

    fn git_push_tag(&self, tag: &str) -> Result<()> {
        let status = std::process::Command::new("git")
            .args(["push", "origin", tag])
            .status()
            .context("failed to run `git push origin <tag>`")?;
        if !status.success() {
            bail!("`git push origin {tag}` failed with status {status}");
        }
        Ok(())
    }

    fn read_cargo_toml(&self) -> Result<String> {
        std::fs::read_to_string("Cargo.toml").context("failed to read Cargo.toml")
    }

    fn write_cargo_toml(&self, content: &str) -> Result<()> {
        std::fs::write("Cargo.toml", content).context("failed to write Cargo.toml")
    }

    fn cargo_update_workspace(&self) -> Result<()> {
        let status = std::process::Command::new("cargo")
            .args(["update", "--workspace"])
            .status()
            .context("failed to run `cargo update --workspace`")?;
        if !status.success() {
            bail!("`cargo update --workspace` failed with status {status}");
        }
        Ok(())
    }

    fn generate_changelog(&self) -> Result<()> {
        crate::changelog::generate_changelog(&crate::changelog::RealSystem)
    }

    fn prompt_user(&self, message: &str) -> Result<String> {
        use std::io::Write;
        print!("{message}");
        std::io::stdout()
            .flush()
            .context("failed to flush stdout")?;
        let mut input = String::new();
        std::io::stdin()
            .read_line(&mut input)
            .context("failed to read user input")?;
        Ok(input.trim().to_owned())
    }
}

/// Determine the suggested next version and release type from the current branch.
///
/// `main` → minor bump; `*-maintenance` → patch bump.
///
/// # Arguments
///
/// * `current` - Current version from `Cargo.toml`.
/// * `branch` - Current git branch name.
///
/// # Returns
///
/// `(ReleaseType, next_version)`.
///
/// # Errors
///
/// Returns an error when `branch` is neither `main` nor ends with
/// `-maintenance`.
pub fn suggest_next_version(current: &Version, branch: &str) -> Result<(ReleaseType, Version)> {
    if branch == "main" {
        let mut next = current.clone();
        next.minor += 1;
        next.patch = 0;
        Ok((ReleaseType::Minor, next))
    } else if branch.ends_with("-maintenance") {
        let mut next = current.clone();
        next.patch += 1;
        Ok((ReleaseType::Patch, next))
    } else {
        bail!(
            "must be on 'main' or a '*-maintenance' branch to prepare a release \
             (current branch: {branch})"
        )
    }
}

/// Determine the release type by comparing two versions.
///
/// # Arguments
///
/// * `current` - The version before the release.
/// * `next` - The version after the release.
///
/// # Returns
///
/// The most significant component that changed.
pub fn determine_release_type(current: &Version, next: &Version) -> ReleaseType {
    if next.major > current.major {
        ReleaseType::Major
    } else if next.minor > current.minor {
        ReleaseType::Minor
    } else {
        ReleaseType::Patch
    }
}

/// Rewrite the `[package].version` field in a `Cargo.toml` string.
///
/// Uses `toml_edit` to preserve all existing formatting.
///
/// # Arguments
///
/// * `cargo_toml_content` - Raw TOML text of `Cargo.toml`.
/// * `new_version` - Version string to set.
///
/// # Returns
///
/// Updated TOML text.
///
/// # Errors
///
/// Returns an error if `cargo_toml_content` cannot be parsed as TOML.
pub fn set_cargo_toml_version(cargo_toml_content: &str, new_version: &str) -> Result<String> {
    let mut doc: toml_edit::Document = cargo_toml_content
        .parse()
        .context("failed to parse Cargo.toml")?;
    doc["package"]["version"] = toml_edit::value(new_version);
    Ok(doc.to_string())
}

/// Prepare a new release.
///
/// Full workflow:
/// 1. Verify working tree is clean.
/// 2. Detect branch and suggest release type / next version.
/// 3. Prompt user (accepts custom version input).
/// 4. Create maintenance branch if on `main`.
/// 5. Update `Cargo.toml` version.
/// 6. Run `cargo update --workspace`.
/// 7. Generate changelog.
/// 8. Commit and push.
///
/// # Arguments
///
/// * `system` - Injected I/O provider.
///
/// # Errors
///
/// Returns an error if any step fails.
pub fn prepare_release<S: ReleaseSystem>(system: &S) -> Result<()> {
    let status = system.git_status_porcelain()?;
    if !status.trim().is_empty() {
        bail!("git working directory is not clean — commit or stash changes first:\n{status}");
    }

    let current_branch = system.git_current_branch()?;
    let cargo_toml = system.read_cargo_toml()?;
    let current_version: Version = crate::changelog::extract_version_from_cargo_toml(&cargo_toml)?
        .parse()
        .context("failed to parse current version as semver")?;

    println!("INFO - Current branch: {current_branch}");
    println!("INFO - Current version: {current_version}");

    let (suggested_type, suggested_version) =
        suggest_next_version(&current_version, &current_branch)?;

    let prompt = format!(
        "Preparing {suggested_type} release: {current_version} -> {suggested_version}. Continue? [Y/n]: "
    );
    let answer = system.prompt_user(&prompt)?;

    let (next_version, actual_type) =
        if answer.eq_ignore_ascii_case("n") || answer.eq_ignore_ascii_case("no") {
            let custom_str = system.prompt_user(&format!(
                "Enter custom version (current: {current_version}): "
            ))?;
            if custom_str.is_empty() {
                bail!("version cannot be empty");
            }
            let custom: Version = custom_str
                .parse()
                .context("invalid version format — use semantic versioning (e.g. 1.2.3)")?;
            let release_type = determine_release_type(&current_version, &custom);
            (custom, release_type)
        } else if answer.is_empty()
            || answer.eq_ignore_ascii_case("y")
            || answer.eq_ignore_ascii_case("yes")
        {
            (suggested_version, suggested_type)
        } else {
            bail!("invalid input — please enter Y or n");
        };

    let target_branch = if current_branch == "main" {
        format!("{}.{}-maintenance", next_version.major, next_version.minor)
    } else {
        current_branch.clone()
    };

    println!("INFO - Preparing {actual_type} release: {current_version} -> {next_version}");
    println!("INFO - Target branch: {target_branch}");

    if current_branch == "main" {
        println!("INFO - Creating maintenance branch: {target_branch}");
        system.git_checkout_new_branch(&target_branch)?;
    }

    println!("INFO - Updating Cargo.toml version to {next_version}");
    let updated_cargo = set_cargo_toml_version(&cargo_toml, &next_version.to_string())?;
    system.write_cargo_toml(&updated_cargo)?;

    println!("INFO - Updating Cargo.lock");
    system.cargo_update_workspace()?;

    println!("INFO - Generating changelog");
    system.generate_changelog()?;

    let commit_message = format!("Version {next_version}");
    println!("INFO - Committing: {commit_message}");
    system.git_add(&[
        "Cargo.toml".to_owned(),
        "Cargo.lock".to_owned(),
        "CHANGELOG.md".to_owned(),
        "changelogging.toml".to_owned(),
    ])?;
    system.git_commit(&commit_message)?;

    println!("INFO - Pushing to remote");
    if current_branch == "main" {
        system.git_push(&["-u".to_owned(), "origin".to_owned(), target_branch.clone()])?;
    } else {
        system.git_push(&[])?;
    }

    println!("INFO - Release {next_version} prepared on branch {target_branch}");
    println!("INFO - Run `cargo xtask create-release-tag` to tag the release");
    Ok(())
}

/// Create and push an annotated git tag for the current release version.
///
/// Full workflow:
/// 1. Verify on a maintenance branch.
/// 2. Read version from `Cargo.toml`.
/// 3. Check the tag does not already exist.
/// 4. Verify the latest commit message is `"Version X.Y.Z"`.
/// 5. Fetch from remote and check not behind.
/// 6. Prompt user for confirmation.
/// 7. Create annotated tag and push.
///
/// # Arguments
///
/// * `system` - Injected I/O provider.
///
/// # Errors
///
/// Returns an error if any validation step fails.
pub fn create_release_tag<S: ReleaseSystem>(system: &S) -> Result<()> {
    let current_branch = system.git_current_branch()?;
    if !current_branch.ends_with("-maintenance") {
        bail!(
            "must be on a maintenance branch to create a release tag \
             (current branch: {current_branch}) — run `cargo xtask prepare-release` first"
        );
    }

    let cargo_toml = system.read_cargo_toml()?;
    let version_str = crate::changelog::extract_version_from_cargo_toml(&cargo_toml)?;
    let version: Version = version_str
        .parse()
        .context("failed to parse version as semver")?;

    println!("INFO - Current branch: {current_branch}");
    println!("INFO - Version to tag: {version}");

    let existing_tag = system.git_tag_list(&version.to_string())?;
    if !existing_tag.trim().is_empty() {
        bail!("tag {version} already exists");
    }

    let commit_msg = system.git_log_latest_subject()?;
    let expected_msg = format!("Version {version}");
    if commit_msg != expected_msg {
        bail!(
            "latest commit message does not match expected version commit\n\
             expected: {expected_msg}\n\
             actual:   {commit_msg}\n\
             run `cargo xtask prepare-release` first"
        );
    }

    println!("INFO - Fetching latest changes from remote");
    if let Err(e) = system.git_fetch() {
        eprintln!("WARN - Failed to fetch from remote, continuing anyway: {e}");
    }

    let behind = system.git_rev_list_count_behind(&current_branch)?;
    if behind > 0 {
        bail!("local branch is {behind} commit(s) behind remote — run `git pull` first");
    }

    let answer = system.prompt_user(&format!(
        "About to create and push tag '{version}'. Continue? [Y/n]: "
    ))?;
    if answer.eq_ignore_ascii_case("n") || answer.eq_ignore_ascii_case("no") {
        println!("INFO - Tag creation cancelled");
        return Ok(());
    }

    let tag_message = format!("Version {version}");
    println!("INFO - Creating annotated tag: {version}");
    system.git_create_annotated_tag(&version.to_string(), &tag_message)?;

    println!("INFO - Pushing tag to remote");
    system.git_push_tag(&version.to_string())?;

    println!("INFO - Tag '{version}' created and pushed");
    println!("INFO - Check: https://github.com/whme/csshw/actions/workflows/release.yml");
    Ok(())
}

#[cfg(test)]
#[path = "tests/test_release.rs"]
mod tests;
