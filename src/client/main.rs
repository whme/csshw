#![deny(clippy::implicit_return)]
#![allow(clippy::needless_return)]
use std::env;
use std::fs::File;
use std::io::{self, BufReader};
use std::path::Path;
use std::time::Duration;

use clap::Parser;
use csshw::utils::constants::DEFAULT_SSH_USERNAME_KEY;
use csshw::utils::{
    arrange_console as arrange_client_console, get_console_input_buffer, set_console_title,
};
use serde_derive::{Deserialize, Serialize};
use ssh2_config::SshConfig;
use tokio::net::windows::named_pipe::NamedPipeClient;
use tokio::process::{Child, Command};
use tokio::{io::Interest, net::windows::named_pipe::ClientOptions};
use windows::Win32::Foundation::GetLastError;
use windows::Win32::System::Console::{
    GenerateConsoleCtrlEvent, WriteConsoleInputW, INPUT_RECORD, INPUT_RECORD_0, KEY_EVENT,
};

use csshw::{
    serde::{deserialization::Deserialize, SERIALIZED_INPUT_RECORD_0_LENGTH},
    utils::constants::{PIPE_NAME, PKG_NAME},
};
use windows::core::PCWSTR;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::{LoadImageW, IMAGE_ICON, LR_DEFAULTSIZE};

const DEFAULT_USERNAME_HOST_PLACEHOLDER: &str = "{{USERNAME_AT_HOST}}";

/// Daemon CLI. Manages client consoles and user input
#[derive(Parser, Debug)]
#[clap(author, version, about, long_about = None)]
struct Args {
    /// Host(s) to connect to
    #[clap(required = true)]
    host: String,

    /// Username used to connect to the hosts
    #[clap(required = true)]
    username: String,

    /// X coordinates of the upper left corner of the console window
    /// in reference to the upper left corner of the screen
    #[clap(required = true)]
    x: i32,

    /// Y coordinates of the upper left corner of the console window
    /// in reference to the upper left corner of the screen
    #[clap(required = true)]
    y: i32,

    /// Width of the console window
    #[clap(required = true)]
    width: i32,

    /// Height of the console window
    #[clap(required = true)]
    height: i32,
}

/// If not present the default config will be written to the default
/// configuration place, under windows this is `%AppData%`
#[derive(Serialize, Deserialize)]
struct ClientConfig {
    /// Full path to the SSH config.
    /// e.g. `'C:\Users\<username>\.ssh\config'`
    ssh_config_path: String,
    /// Name of the program used to establish the SSH connection.
    /// e.g. `'ssh'`
    program: String,
    /// List of arguments provided to the program.
    /// Must include the `username_host_placeholder`.
    /// e.g. `['-XY' '{{USERNAME_AT_HOST}}']`
    arguments: Vec<String>,
    /// Placeholder string used to inject `<user>@<host>` into the list of arguments.
    /// e.g. `'{{USERNAME_AT_HOST}}'`
    username_host_placeholder: String,
}

impl Default for ClientConfig {
    fn default() -> Self {
        return ClientConfig {
            ssh_config_path: format!("{}\\.ssh\\config", env::var("USERPROFILE").unwrap()),
            program: "ssh".to_string(),
            arguments: vec![
                "-XY".to_string(),
                DEFAULT_USERNAME_HOST_PLACEHOLDER.to_string(),
            ],
            username_host_placeholder: DEFAULT_USERNAME_HOST_PLACEHOLDER.to_string(),
        };
    }
}

fn write_console_input(input_record: INPUT_RECORD_0) {
    let buffer: [INPUT_RECORD; 1] = [INPUT_RECORD {
        EventType: KEY_EVENT as u16,
        Event: input_record,
    }];
    let mut nb_of_events_written: u32 = 0;
    unsafe {
        if WriteConsoleInputW(
            get_console_input_buffer(),
            &buffer,
            &mut nb_of_events_written,
        ) == false
            || nb_of_events_written == 0
        {
            println!("Failed to write console input");
            println!("{:?}", GetLastError());
        }
    };
}

/// Use `args.username` or load the adequate one from SSH config.
///
/// Returns `<username>@<host>`.
fn get_username_and_host(args: &Args, config: &ClientConfig) -> String {
    let mut reader = BufReader::new(
        File::open(Path::new(config.ssh_config_path.as_str()))
            .expect("Could not open SSH configuration file."),
    );
    let ssh_config = SshConfig::default()
        .parse(&mut reader)
        .expect("Failed to parse SSH configuration file");

    let default_params = ssh_config.default_params();
    let host_specific_params = ssh_config.query(args.host.clone());

    let username: String = if args.username.as_str() == DEFAULT_SSH_USERNAME_KEY {
        // FIXME: find a better default
        host_specific_params
            .user
            .unwrap_or(default_params.user.unwrap_or("undefined".to_string()))
    } else {
        args.username.clone()
    };

    return format!("{}@{}", username, args.host);
}

/// Launch the SSH process.
/// It might overwrite the console title once it launches, so we wait for that
/// to happen and set the title again.
async fn launch_ssh_process(username_host: &str, config: &ClientConfig) -> Child {
    let child = Command::new(&config.program)
        .args(config.arguments.clone().into_iter().map(|arg| {
            return arg.replace(config.username_host_placeholder.as_str(), username_host);
        }))
        .spawn()
        .unwrap();
    return child;
}

async fn read_write_loop(named_pipe_client: &NamedPipeClient) -> bool {
    let mut buf: [u8; SERIALIZED_INPUT_RECORD_0_LENGTH] = [0; SERIALIZED_INPUT_RECORD_0_LENGTH];
    match named_pipe_client.try_read(&mut buf) {
        Ok(read_bytes) if read_bytes != SERIALIZED_INPUT_RECORD_0_LENGTH => {
            // Seems to only happen if the pipe is closed/server disconnects
            // indicating that the daemon has been closed.
            // Exit the client too in that case.
            return false;
        }
        Ok(_) => {
            write_console_input(INPUT_RECORD_0::deserialize(&mut buf));
            return true;
        }
        Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
            return true;
        }
        Err(e) => {
            println!("{}", e);
            return true;
        }
    }
}

async fn run(child: &mut Child) {
    // Many clients trying to open the pipe at the same time can cause
    // a file not found error, so keep trying until we managed to open it
    let named_pipe_client: NamedPipeClient = loop {
        match ClientOptions::new().open(PIPE_NAME) {
            Ok(named_pipe_client) => {
                break named_pipe_client;
            }
            Err(_) => {
                continue;
            }
        }
    };
    named_pipe_client.ready(Interest::READABLE).await.unwrap();
    let mut failure_iterations = 0;
    while read_write_loop(&named_pipe_client).await {
        match child.try_wait() {
            Ok(Some(exit_status)) => match exit_status.code().unwrap() {
                0 | 1 | 130 => {
                    // 0 -> last command successful
                    // 1 -> last command unsuccessful
                    // 130 -> last command cancelled (Ctrl + C)
                    return;
                }
                255 => {
                    if failure_iterations == 0 {
                        println!("Failed to establish SSH connection: {exit_status}");
                        println!("Exiting after 60 seconds ...");
                        // TODO: alternatively exit upon a keypress; either in the daemon
                        // or directly in the client
                    } else if failure_iterations >= 60 * 1000 / 5 {
                        return;
                    }
                    failure_iterations += 1;
                }
                _ => {
                    if failure_iterations == 0 {
                        println!("SSH terminated with status {exit_status}");
                        println!("Exiting after 60 seconds ...");
                        // TODO: alternatively exit upon a keypress; either in the daemon
                        // or directly in the client
                    } else if failure_iterations >= 60 * 1000 / 5 {
                        return;
                    }
                    failure_iterations += 1;
                }
            },
            Ok(None) => (
                // child is still running
            ),
            Err(e) => panic!("{}", e),
        }
        // Sleep some time to avoid hogging 100% CPU usage.
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
}

#[tokio::main]
async fn main() {
    unsafe {
        LoadImageW(
            GetModuleHandleW(None).unwrap(),
            PCWSTR(1 as _), // Value must match the `nameID` in the .rc script
            IMAGE_ICON,
            0,
            0,
            LR_DEFAULTSIZE,
        )
        .unwrap()
    };
    let args = Args::parse();
    arrange_client_console(args.x, args.y, args.width, args.height);
    let config: ClientConfig = confy::load(PKG_NAME, "client-config").unwrap();

    let username_host = get_username_and_host(&args, &config);

    // Set the console title (child might overwrite it, so we have to set it again later)
    let console_title = format!("{} - {}", PKG_NAME, username_host.clone());
    set_console_title(console_title.as_str());

    let mut child = launch_ssh_process(&username_host, &config).await;

    run(&mut child).await;

    // Make sure the client and all its subprocesses
    // are aware they need to shutdown.
    unsafe {
        GenerateConsoleCtrlEvent(0, 0);
    }
    drop(child);
}
