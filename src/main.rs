use std::{os::windows::process::CommandExt, process::Command};

use clap::Parser;
use windows::Win32::System::Threading::CREATE_NEW_CONSOLE;

const PKG_NAME: &str = env!("CARGO_PKG_NAME");

/// Simple SSH multiplexer
#[derive(Parser, Debug)]
#[clap(author, version, about, long_about = None)]
struct Args {
    /// Block until the session is terminted
    #[clap(short, long)]
    block: bool,

    /// Host(s) to connect to
    #[clap(required = true)]
    hosts: Vec<String>,
}

fn main() {
    let args = Args::parse();
    let mut daemon = Command::new(format!("{}-daemon", PKG_NAME))
        .args(args.hosts)
        .creation_flags(CREATE_NEW_CONSOLE.0)
        .spawn()
        .expect("Failed to start daemon process.");
    if args.block {
        daemon.wait().expect("Failed to wait for daemon process");
    }
}
