//! xtask — developer automation tasks for csshw.
//!
//! Invoke via `cargo xtask <subcommand>`.
//! See each subcommand's module for details.

#![cfg_attr(coverage_nightly, feature(coverage_attribute))]

mod changelog;
mod coverage;
mod readme;
mod release;
mod social_preview;

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
    /// Regenerate res/social-preview.png with the current GitHub star count.
    GenerateSocialPreview,
    /// Run coverage analysis using a pinned nightly toolchain.
    Coverage,
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
        Command::GenerateSocialPreview => {
            social_preview::generate_social_preview(&social_preview::RealSystem)?;
        }
        Command::Coverage => {
            coverage::run_coverage(&coverage::RealSystem)?;
        }
    }
    Ok(())
}
