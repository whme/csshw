//! CLI interface

use crate::client::main as client_main;
use crate::daemon::{main as daemon_main, resolve_cluster_tags};
use crate::utils::config::{ClientConfig, Cluster, Config, ConfigOpt, DaemonConfig};
use crate::{
    get_concole_window_handle, init_logger, spawn_console_process,
    WindowsSettingsDefaultTerminalApplicationGuard,
};
use clap::{ArgAction, Parser, Subcommand};

#[cfg(test)]
use mockall::{automock, predicate::*};
use windows::Win32::UI::HiDpi::{SetProcessDpiAwareness, PROCESS_PER_MONITOR_DPI_AWARE};

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
    #[clap(long, short = 'u')]
    username: Option<String>,
    /// Hosts and/or cluster tag(s) to connect to
    ///
    /// Hosts or cluster tags might use brace expansion,
    /// but need to be properly quoted.
    ///
    /// E.g.: `csshw.exe "host{1..3}" hostA`
    ///
    /// Hosts can include a username which will take precedence over the
    /// username given via the `-u` option and over any ssh config value.
    ///
    /// E.g.: `csshw.exe -u user3 user1@host1 userA@hostA host3`
    #[clap(required = false, global = true)]
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
    Daemon {},
}

/// Main Entrypoint struct
///
/// Used to implement the entrypoint functions of the different
/// subcommands
pub struct MainEntrypoint;

/// Trait defining the entrypoint functions of the different
/// subcommands
#[cfg_attr(test, automock)]
pub trait Entrypoint {
    /// Entrypoint for the client subcommand
    fn client_main(
        &mut self,
        host: String,
        username: Option<String>,
        config: &ClientConfig,
    ) -> impl std::future::Future<Output = ()> + Send;
    /// Entrypoint for the daemon subcommand
    fn daemon_main(
        &mut self,
        hosts: Vec<String>,
        username: Option<String>,
        config: &DaemonConfig,
        clusters: &[Cluster],
        debug: bool,
    ) -> impl std::future::Future<Output = ()> + Send;
    /// Entrypoint for the main command
    fn main(&mut self, config_path: &str, config: &Config, args: Args);
}

impl Entrypoint for MainEntrypoint {
    async fn client_main(&mut self, host: String, username: Option<String>, config: &ClientConfig) {
        client_main(host, username, config).await;
    }

    async fn daemon_main(
        &mut self,
        hosts: Vec<String>,
        username: Option<String>,
        config: &DaemonConfig,
        clusters: &[Cluster],
        debug: bool,
    ) {
        daemon_main(hosts, username, config, clusters, debug).await;
    }

    fn main(&mut self, config_path: &str, config: &Config, args: Args) {
        confy::store_path(config_path, config).unwrap();

        let mut daemon_args: Vec<&str> = Vec::new();
        if args.debug {
            daemon_args.push("-d");
        }
        if let Some(username) = args.username.as_ref() {
            daemon_args.push("-u");
            daemon_args.push(username);
        }
        daemon_args.push("daemon");
        // Order is important here. If the hosts are passed before the daemon subcommand
        // it will not be recognizes as such and just be passed along as one of the hosts.
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

/// The main entrypoint
///
/// Parses the CLI arguments,
/// loads an existing config or writes the default config to disk, and
/// calls the respective subcommand.
/// If no subcommand is given we launch the daemon subcommand in a new window.
pub async fn main<T: Entrypoint>(args: Args, mut entrypoint: T) {
    // Set DPI awareness programatically. Using the manifest is the recommended way
    // but conhost.exe does not do any manifest loading.
    // https://github.com/microsoft/terminal/issues/18464#issuecomment-2623392013
    if let Err(err) = unsafe { SetProcessDpiAwareness(PROCESS_PER_MONITOR_DPI_AWARE) } {
        eprintln!("Failed to set DPI awareness programatically: {err:?}");
    }
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
        Some(Commands::Client { host }) => {
            if args.debug {
                init_logger(&format!("csshw_client_{host}"));
            }
            entrypoint
                .client_main(host.to_owned(), args.username.to_owned(), &config.client)
                .await;
        }
        Some(Commands::Daemon {}) => {
            if args.debug {
                init_logger("csshw_daemon");
            }
            entrypoint
                .daemon_main(
                    args.hosts.to_owned(),
                    args.username.clone(),
                    &config.daemon,
                    &config.clusters,
                    args.debug,
                )
                .await;
        }
        None => {
            entrypoint.main(&config_path, &config, args);
        }
    }
}

#[cfg(test)]
#[path = "./tests/test_cli.rs"]
mod test_cli;
