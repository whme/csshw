//! CLI interface

use crate::client::main as client_main;
use crate::daemon::{main as daemon_main, resolve_cluster_tags};
use crate::utils::config::{Config, ConfigOpt};
use crate::{
    get_concole_window_handle, init_logger, spawn_console_process,
    WindowsSettingsDefaultTerminalApplicationGuard,
};
use clap::{ArgAction, Parser, Subcommand};

const PKG_NAME: &str = env!("CARGO_PKG_NAME");

/// Cluster SSH tool for Windows inspired by csshX
///
/// The main CLI arguments
#[derive(Parser, Debug)]
#[clap(author, version, about, long_about = None)]
pub struct Args {
    /// Optional subcommand
    /// Usually not specified by the user
    #[clap(subcommand)]
    command: Option<Commands>,
    /// Optional username used to connect to the hosts
    #[clap(short, long)]
    username: Option<String>,
    /// Hosts and/or cluster tag(s) to connect to
    #[clap(required = false)]
    hosts: Vec<String>,
    /// Enable extensive logging
    #[clap(short, long, action=ArgAction::SetTrue)]
    debug: bool,
}

/// The ``command`` CLI subcommand
#[derive(Debug, Subcommand, PartialEq)]
enum Commands {
    /// Subcommand that will launch a single client window
    ///
    /// connecting to the given host with the given username.
    /// It will also try to read input from a daemon via the named pipe.
    Client {
        /// Username used to connect to the host
        #[clap(long, short = 'u')]
        username: Option<String>,
        /// Host to connect to
        host: String,
    },
    /// Subcommand that will launch the daemon window.
    ///
    /// The daemon is responsible to launch the client windows,
    /// one for each given host.
    /// For each client a named pipe will be created and any keystrokes
    /// the daemon window receives are forwarded via the pipes to all the clients.
    /// Also handles control mode.
    Daemon {
        /// Username used to connect to the hosts
        #[clap(long, short = 'u')]
        username: Option<String>,
        /// Host(s) to connect to
        hosts: Vec<String>,
    },
}

/// The main entrypoint
///
/// Parses the CLI arguments,
/// loads an existing config or writes the default config to disk, and
/// calls the respective subcommand.
/// If no subcommand is given we launch the daemon subcommand in a new window.
pub async fn entrypoint(args: Args) {
    match std::env::current_exe() {
        Ok(path) => match path.parent() {
            None => {
                eprintln!("Failed to get executable path parent working directory");
            }
            Some(exe_dir) => {
                std::env::set_current_dir(exe_dir)
                    .expect("Failed to change current working directory");
            }
        },
        Err(_) => {
            eprintln!("Failed to get executable directory");
        }
    }

    let config_path = format!("{PKG_NAME}-config.toml");
    let config_on_disk: ConfigOpt = confy::load_path(&config_path).unwrap();
    let config: Config = config_on_disk.into();

    match &args.command {
        Some(Commands::Client { host, username }) => {
            if args.debug {
                init_logger(&format!("csshw_client_{host}"));
            }
            client_main(host.to_owned(), username.to_owned(), &config.client).await;
        }
        Some(Commands::Daemon { username, hosts }) => {
            if args.debug {
                init_logger("csshw_daemon");
            }
            daemon_main(
                hosts.to_owned(),
                username.clone(),
                &config.daemon,
                &config.clusters,
                args.debug,
            )
            .await;
        }
        None => {
            confy::store_path(&config_path, &config).unwrap();

            let mut daemon_args: Vec<&str> = Vec::new();
            if args.debug {
                daemon_args.push("-d");
            }
            daemon_args.push("daemon");
            if let Some(username) = args.username.as_ref() {
                daemon_args.push("-u");
                daemon_args.push(username);
            }
            daemon_args.extend(resolve_cluster_tags(
                args.hosts.iter().map(|host| return &**host).collect(),
                &config.clusters,
            ));
            let _guard = WindowsSettingsDefaultTerminalApplicationGuard::new();
            // We must wait for the window to actually launch before dropping the _guard as we might otherwise
            // reset the configuration before the window was launched
            let _ = get_concole_window_handle(
                spawn_console_process(&format!("{PKG_NAME}.exe"), daemon_args).dwProcessId,
            );
        }
    }
}

#[cfg(test)]
#[path = "./tests/test_cli.rs"]
mod test_cli;
