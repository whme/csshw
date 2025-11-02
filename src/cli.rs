//! CLI interface

use crate::client::main as client_main;
use crate::daemon::{main as daemon_main, resolve_cluster_tags};
use crate::utils::config::{ClientConfig, Cluster, Config, ConfigOpt, DaemonConfig};
use crate::utils::windows::WindowsApi;
use crate::{
    get_console_window_handle, init_logger, is_launched_from_gui, spawn_console_process,
    WindowsSettingsDefaultTerminalApplicationGuard,
};
use clap::{ArgAction, CommandFactory, Parser, Subcommand};

#[cfg(test)]
use mockall::{automock, predicate::*};
use windows::Win32::UI::HiDpi::PROCESS_PER_MONITOR_DPI_AWARE;

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
    /// Optional port used for all SSH connections
    #[clap(long, short = 'p')]
    port: Option<u16>,
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
    ///
    /// Hosts can include a port number which will take precedence over the
    /// port given via the `-p` option.
    ///
    /// E.g.: `csshw.exe -p 33 host1:11 host2:22 host3`
    ///
    /// If no hosts are provided and the application is launched in a new console window
    /// (e.g. by double clicking the executable in the File Explorer),
    /// it will launch in interactive mode.
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

/// Trait for Args operations to enable mocking in tests
#[cfg_attr(test, automock)]
pub trait ArgsCommand {
    /// Print help message
    fn print_help(&self) -> Result<(), std::io::Error>;
}

/// Default implementation of ArgsCommand trait
pub struct CLIArgsCommand;

impl ArgsCommand for CLIArgsCommand {
    fn print_help(&self) -> Result<(), std::io::Error> {
        return Args::command().print_help();
    }
}

/// Trait for logger initialization to enable mocking in tests
#[cfg_attr(test, automock)]
pub trait LoggerInitializer {
    /// Initialize logger with the given name
    fn init_logger(&self, name: &str);
}

/// Default implementation of LoggerInitializer trait
pub struct CLILoggerInitializer;

impl LoggerInitializer for CLILoggerInitializer {
    fn init_logger(&self, name: &str) {
        init_logger(name);
    }
}

/// Trait for writing output to enable dependency injection and testing
#[cfg_attr(test, automock)]
pub trait Writer {
    /// Write a line to the output
    fn writeln(&mut self, text: &str);
    /// Write text without a newline to the output
    fn write(&mut self, text: &str);
    /// Write a line to stderr
    fn write_err(&mut self, text: &str);
    /// Flush the output
    fn flush(&mut self);
}

/// Default implementation of Writer trait that writes to stdout/stderr
pub struct CLIWriter;

impl Writer for CLIWriter {
    fn writeln(&mut self, text: &str) {
        println!("{text}");
    }

    fn write(&mut self, text: &str) {
        print!("{text}");
    }

    fn write_err(&mut self, text: &str) {
        eprintln!("{text}");
    }

    fn flush(&mut self) {
        use std::io::Write;
        std::io::stdout().flush().unwrap();
    }
}

/// Trait defining the entrypoint functions of the different
/// subcommands
#[cfg_attr(test, automock)]
pub trait Entrypoint {
    /// Entrypoint for the client subcommand
    fn client_main<W: WindowsApi + 'static>(
        &mut self,
        windows_api: &W,
        host: String,
        username: Option<String>,
        port: Option<u16>,
        config: &ClientConfig,
    ) -> impl std::future::Future<Output = ()> + Send;
    /// Entrypoint for the daemon subcommand
    fn daemon_main<W: WindowsApi + Clone + 'static>(
        &mut self,
        windows_api: &W,
        hosts: Vec<String>,
        username: Option<String>,
        port: Option<u16>,
        config: &DaemonConfig,
        clusters: &[Cluster],
        debug: bool,
    ) -> impl std::future::Future<Output = ()> + Send;
    /// Entrypoint for the main command
    fn main<W: WindowsApi + 'static>(
        &mut self,
        windows_api: &W,
        config_path: &str,
        config: &Config,
        args: Args,
    );
}

impl Entrypoint for MainEntrypoint {
    async fn client_main<W: WindowsApi>(
        &mut self,
        windows_api: &W,
        host: String,
        username: Option<String>,
        port: Option<u16>,
        config: &ClientConfig,
    ) {
        client_main(windows_api, host, username, port, config).await;
    }

    async fn daemon_main<W: WindowsApi + Clone + 'static>(
        &mut self,
        windows_api: &W,
        hosts: Vec<String>,
        username: Option<String>,
        port: Option<u16>,
        config: &DaemonConfig,
        clusters: &[Cluster],
        debug: bool,
    ) {
        daemon_main(windows_api, hosts, username, port, config, clusters, debug).await;
    }

    fn main<W: WindowsApi + 'static>(
        &mut self,
        windows_api: &W,
        config_path: &str,
        config: &Config,
        args: Args,
    ) {
        confy::store_path(config_path, config).unwrap();

        let mut daemon_args: Vec<String> = Vec::new();
        if args.debug {
            daemon_args.push("-d".to_string());
        }
        if let Some(username) = args.username {
            daemon_args.push("-u".to_string());
            daemon_args.push(username);
        }
        if let Some(port) = args.port {
            daemon_args.push("-p".to_string());
            daemon_args.push(port.to_string());
        }
        daemon_args.push("daemon".to_string());
        // Order is important here. If the hosts are passed before the daemon subcommand
        // it will not be recognizes as such and just be passed along as one of the hosts.
        daemon_args.extend(
            resolve_cluster_tags(
                args.hosts.iter().map(|host| return &**host).collect(),
                &config.clusters,
            )
            .into_iter()
            .map(|host| return host.to_string()),
        );
        let _guard = WindowsSettingsDefaultTerminalApplicationGuard::new();
        // We must wait for the window to actually launch before dropping the _guard as we might otherwise
        // reset the configuration before the window was launched
        let _ = get_console_window_handle(
            windows_api,
            spawn_console_process(windows_api, &format!("{PKG_NAME}.exe"), daemon_args)
                .expect("Failed to create process")
                .dwProcessId,
        );
    }
}

/// Display the interactive mode prompt and instructions
fn show_interactive_prompt<Wr: Writer>(writer: &mut Wr) {
    writer.writeln("\n=== Interactive Mode ===");
    writer.writeln(&format!(
        "Enter your {PKG_NAME} arguments (or press Enter to exit):"
    ));
    writer.writeln("Example: -u myuser host1 host2 host3");
    writer.writeln("Example: --help");
    writer.write("> ");
    writer.flush();
}

/// Read user input from stdin
///
/// # Returns
///
/// * `Ok(Some(input))` - User provided input
/// * `Ok(None)` - User wants to exit (empty input or "exit")
/// * `Err(error)` - Error reading input
fn read_user_input() -> Result<Option<String>, std::io::Error> {
    let mut input = String::new();
    std::io::stdin().read_line(&mut input)?;

    let input = input.trim();
    if input.is_empty() || input.to_lowercase() == "exit" {
        return Ok(None);
    }

    return Ok(Some(input.to_string()));
}

/// Handle special commands that don't need full parsing
///
/// # Arguments
///
/// * `input` - The user input string
/// * `args_command` - The ArgsCommand trait object for printing help
///
/// # Returns
///
/// * `true` - Command was handled, continue loop
/// * `false` - Command needs full parsing
fn handle_special_commands<A: ArgsCommand>(input: &str, args_command: &A) -> bool {
    if input == "--help" || input == "-h" {
        let _ = args_command.print_help();
        return true;
    }
    return false;
}

/// Execute a parsed command using the provided entrypoint
async fn execute_parsed_command<
    W: WindowsApi + Clone + 'static,
    T: Entrypoint,
    A: ArgsCommand,
    L: LoggerInitializer,
>(
    windows_api: &W,
    parsed_args: Args,
    entrypoint: &mut T,
    args_command: &A,
    logger_initializer: &L,
    config: &Config,
    config_path: &str,
) {
    match &parsed_args.command {
        Some(Commands::Client { host }) => {
            if parsed_args.debug {
                logger_initializer.init_logger(&format!("csshw_client_{host}"));
            }
            entrypoint
                .client_main(
                    windows_api,
                    host.to_owned(),
                    parsed_args.username.to_owned(),
                    parsed_args.port,
                    &config.client,
                )
                .await;
        }
        Some(Commands::Daemon {}) => {
            if parsed_args.debug {
                logger_initializer.init_logger("csshw_daemon");
            }
            entrypoint
                .daemon_main(
                    windows_api,
                    parsed_args.hosts,
                    parsed_args.username,
                    parsed_args.port,
                    &config.daemon,
                    &config.clusters,
                    parsed_args.debug,
                )
                .await;
        }
        None => {
            if !parsed_args.hosts.is_empty() {
                entrypoint.main(windows_api, config_path, config, parsed_args);
            } else {
                // Show help for empty hosts
                let _ = args_command.print_help();
            }
        }
    }
}

/// Run the interactive mode loop for GUI launches
async fn run_interactive_mode<W: WindowsApi + Clone + 'static, T: Entrypoint, Wr: Writer>(
    windows_api: &W,
    mut entrypoint: T,
    config: &Config,
    config_path: &str,
    writer: &mut Wr,
) {
    loop {
        show_interactive_prompt(writer);

        match read_user_input() {
            Ok(Some(input)) => {
                // Handle special commands first
                if handle_special_commands(&input, &CLIArgsCommand) {
                    continue;
                }

                // Parse the input as command line arguments
                let input_args: Vec<&str> = input.split_whitespace().collect();
                let mut full_args = vec![PKG_NAME];
                full_args.extend(input_args);

                match Args::try_parse_from(full_args) {
                    Ok(parsed_args) => {
                        execute_parsed_command(
                            windows_api,
                            parsed_args,
                            &mut entrypoint,
                            &CLIArgsCommand,
                            &CLILoggerInitializer,
                            config,
                            config_path,
                        )
                        .await;
                    }
                    Err(err) => {
                        writer.write_err(&format!("\nError parsing arguments: {err}"));
                    }
                }
            }
            Ok(None) => {
                return;
            }
            Err(err) => {
                writer.write_err(&format!("Error reading input: {err}"));
            }
        }
    }
}

/// The main entrypoint
///
/// Parses the CLI arguments,
/// loads an existing config or writes the default config to disk, and
/// calls the respective subcommand.
/// If no subcommand is given we launch the daemon subcommand in a new window.
pub async fn main<
    W: WindowsApi + Clone + 'static,
    E: Entrypoint,
    Wr: Writer,
    A: ArgsCommand,
    L: LoggerInitializer,
>(
    windows_api: &W,
    args: Args,
    mut entrypoint: E,
    writer: &mut Wr,
    args_command: &A,
    logger_initializer: &L,
) {
    // CRITICAL: Check GUI launch BEFORE any output to console
    let launched_from_gui = is_launched_from_gui(windows_api);

    // Set DPI awareness programatically. Using the manifest is the recommended way
    // but conhost.exe does not do any manifest loading.
    // https://github.com/microsoft/terminal/issues/18464#issuecomment-2623392013
    if let Err(err) = windows_api.set_process_dpi_awareness(PROCESS_PER_MONITOR_DPI_AWARE) {
        writer.write_err(&format!(
            "Failed to set DPI awareness programatically: {err:?}"
        ));
    }
    match std::env::current_exe() {
        Ok(path) => match path.parent() {
            None => {
                writer.write_err("Failed to get executable path parent working directory");
            }
            Some(exe_dir) => {
                std::env::set_current_dir(exe_dir)
                    .expect("Failed to change current working directory");
            }
        },
        Err(_) => {
            writer.write_err("Failed to get executable directory");
        }
    }

    let config_path = format!("{PKG_NAME}-config.toml");
    let config_on_disk: ConfigOpt = confy::load_path(&config_path).unwrap();
    let config: Config = config_on_disk.into();

    match &args.command {
        Some(Commands::Client { host }) => {
            if args.debug {
                logger_initializer.init_logger(&format!("csshw_client_{host}"));
            }
            entrypoint
                .client_main(
                    windows_api,
                    host.to_owned(),
                    args.username.to_owned(),
                    args.port,
                    &config.client,
                )
                .await;
        }
        Some(Commands::Daemon {}) => {
            if args.debug {
                logger_initializer.init_logger("csshw_daemon");
            }
            entrypoint
                .daemon_main(
                    windows_api,
                    args.hosts.to_owned(),
                    args.username.clone(),
                    args.port,
                    &config.daemon,
                    &config.clusters,
                    args.debug,
                )
                .await;
        }
        None => {
            // If no hosts provided, show help and handle GUI vs console launch
            if args.hosts.is_empty() {
                // Show help using the injected args_command
                let _ = args_command.print_help();

                // If launched from GUI, allow user to input arguments interactively
                if launched_from_gui {
                    run_interactive_mode(windows_api, entrypoint, &config, &config_path, writer)
                        .await;
                }
                return;
            }

            entrypoint.main(windows_api, &config_path, &config, args);
        }
    }
}

#[cfg(test)]
#[path = "./tests/test_cli.rs"]
mod test_cli;
