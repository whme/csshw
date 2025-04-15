//! Cluster SSH tool for Windows inspired by csshX - Binary
//! ---
//! ```
//! Usage: csshw.exe [OPTIONS] [HOSTS]... [COMMAND]
//!
//! Commands:
//!   client  Subcommand that will launch a single client window
//!   daemon  Subcommand that will launch the daemon window
//!   help    Print this message or the help of the given subcommand(s)
//!
//! Arguments:
//!   [HOSTS]...  Hosts to connect to
//!
//! Options:
//!   -u, --username <USERNAME>  Optional username used to connect to the hosts
//!   -d, --debug                Enable extensive logging
//!   -h, --help                 Print help
//!   -V, --version              Print version
//! ```

#![deny(clippy::implicit_return)]
#![allow(clippy::needless_return, clippy::doc_overindented_list_items)]
#![warn(missing_docs)]
#![doc(html_no_source)]

use clap::Parser as _;
use csshw_lib::cli::{self, Args, MainEntrypoint};

/// The main entrypoint of the binary
#[tokio::main]
async fn main() {
    cli::main(Args::parse(), MainEntrypoint).await;
}
