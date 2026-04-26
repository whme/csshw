//! xtask — developer automation tasks for csshw.
//!
//! Invoke via `cargo xtask <subcommand>`.
//! See each subcommand's module for details.

#![cfg_attr(coverage_nightly, feature(coverage_attribute))]

mod changelog;
mod coverage;
mod inject_agent_token;
mod readme;
mod release;
mod social_preview;

use std::path::PathBuf;

use anyhow::Result;
use clap::{Parser, Subcommand};

/// Developer automation tasks for csshw.
#[derive(Parser)]
#[clap(name = "xtask")]
struct Args {
    #[clap(subcommand)]
    command: Command,
}

/// Available xtask subcommands.
#[derive(Subcommand)]
enum Command {
    /// Verify the README help section matches `cargo run --package csshw -- --help`.
    CheckReadmeHelp,
    /// Update the README help section to match `cargo run --package csshw -- --help`.
    UpdateReadmeHelp,
    /// Generate changelog for the current version from news fragments.
    GenerateChangelog,
    /// Prepare a new release: bump version, create maintenance branch, commit, push.
    PrepareRelease,
    /// Create and push an annotated git release tag for the current version.
    CreateReleaseTag,
    /// Render the 1280x640 social preview PNG from templates/social-preview.html
    /// using headless Chromium via the pinned Playwright Docker image.
    GenerateSocialPreview {
        /// Output path for the generated PNG. Relative paths resolve
        /// against the workspace root; absolute paths are accepted as
        /// long as they live under the workspace root so the container
        /// bind mount can reach them. Defaults to
        /// `target/social-preview/social-preview.png`.
        #[arg(long)]
        out: Option<PathBuf>,
        /// GitHub token used for authenticated API requests. Falls back to
        /// the `GITHUB_TOKEN` environment variable, then to unauthenticated
        /// access (rate-limited to 60 requests/hour).
        #[arg(long)]
        token: Option<String>,
    },
    /// Run coverage analysis using a pinned nightly toolchain.
    Coverage,
    /// Inject a contributor-supplied fine-grained GitHub PAT into the
    /// current worktree's `.claude/settings.local.json` so paseo-spawned
    /// agents act with a least-privilege token instead of the user's
    /// full `gh` login. A no-op when `.paseo/gh-token` is absent.
    InjectAgentToken,
}

fn main() -> Result<()> {
    let args = Args::parse();
    match args.command {
        Command::CheckReadmeHelp => {
            readme::check_readme_help(&readme::RealSystem)?;
        }
        Command::UpdateReadmeHelp => {
            let changed = readme::update_readme_help(&readme::RealSystem)?;
            if changed {
                // Exit 1 to abort the pre-commit hook when the README was modified.
                std::process::exit(1);
            }
        }
        Command::GenerateChangelog => {
            changelog::generate_changelog(&changelog::RealSystem)?;
        }
        Command::PrepareRelease => {
            release::prepare_release(&release::RealSystem)?;
        }
        Command::CreateReleaseTag => {
            release::create_release_tag(&release::RealSystem)?;
        }
        Command::GenerateSocialPreview { out, token } => {
            social_preview::generate_social_preview(&social_preview::RealSystem, out, token)?;
        }
        Command::Coverage => {
            coverage::run_coverage(&coverage::RealSystem)?;
        }
        Command::InjectAgentToken => {
            inject_agent_token::inject_agent_token(&inject_agent_token::RealSystem)?;
        }
    }
    Ok(())
}
