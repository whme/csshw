use std::env;
use std::fs::File;
use std::io::{self, BufReader};
use std::path::Path;
use std::process::Command;
use std::time::Duration;

use clap::Parser;
use csshw::utils::constants::DEFAULT_SSH_USERNAME_KEY;
use csshw::utils::{get_console_input_buffer, get_console_title, set_console_title};
use ssh2_config::SshConfig;
use tokio::net::windows::named_pipe::NamedPipeClient;
use tokio::{io::Interest, net::windows::named_pipe::ClientOptions};
use windows::Win32::Foundation::GetLastError;
use windows::Win32::System::Console::{
    GenerateConsoleCtrlEvent, GetConsoleWindow, WriteConsoleInputW, INPUT_RECORD, INPUT_RECORD_0,
    KEY_EVENT,
};
use windows::Win32::UI::WindowsAndMessaging::MoveWindow;

use csshw::{
    serde::{deserialization::Deserialize, SERIALIZED_INPUT_RECORD_0_LENGTH},
    utils::constants::{PIPE_NAME, PKG_NAME},
};

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

#[tokio::main]
async fn main() {
    let args = Args::parse();
    let hwnd = unsafe { GetConsoleWindow() };
    // FIXME: for some client it doesn't seem to work and they do not re-arange themselves
    // when connected to an external screen
    unsafe {
        MoveWindow(hwnd, args.x, args.y, args.width, args.height, true);
    }

    // TODO: make SSH config file configurable
    let mut reader = BufReader::new(
        File::open(Path::new(
            format!("{}\\.ssh\\config", env::var("USERPROFILE").unwrap()).as_str(),
        ))
        .expect("Could not open SSH configuration file."),
    );
    let ssh_config = SshConfig::default()
        .parse(&mut reader)
        .expect("Failed to parse SSH configuration file");

    let host = args.host.clone();
    let username: String;
    let username_host: String;

    if args.username.as_str() == DEFAULT_SSH_USERNAME_KEY {
        let default_params = ssh_config.default_params();
        let host_specific_params = ssh_config.query(host.clone());
        // FIXME: find a better default
        username = host_specific_params
            .user
            .unwrap_or(default_params.user.unwrap_or("undefined".to_string()));
        // No need to specify the username as it is already specified in the SSH config
        username_host = host;
    } else {
        username = args.username.clone();
        username_host = format!("{}@{}", args.username, host);
    }

    // Set the console title (child might overwrite it, so we have to set it again later)
    let console_title = format!("{} - {}@{}", PKG_NAME, username, args.host);
    set_console_title(console_title.as_str());

    // TODO: make executable (ssh, wsl-distro, etc..) and args configurable
    let mut child = Command::new("ubuntu")
        .args([
            "run",
            format!(
                "source ~/.bash_profile; \
                ssh -XY {} || \
                [[ $? -eq 130 ]]",
                username_host
            )
            .as_str(),
        ])
        .spawn()
        .unwrap();

    // Wait for child to overwrite console title on startup and set it once more
    loop {
        if get_console_title() != console_title.as_str() {
            set_console_title(console_title.as_str());
            break;
        }
        match child.try_wait() {
            Ok(Some(_)) => {
                // If the child exits while were in this loop, it can only mean
                // we couldn't establish an ssh connection
                // Then set the console title again
                set_console_title(console_title.as_str());
                // TODO: wait for input before exiting
            }
            Ok(None) => (
                // child is still running
            ),
            Err(e) => panic!("{}", e),
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }

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

    loop {
        match child.try_wait() {
            Ok(Some(_)) => {
                // TODO: maybe differentiate between exit code 0
                // and errors. For the latter stay alive until a key is pressed
                break;
            }
            Ok(None) => (
                // child is still running
            ),
            Err(e) => panic!("{}", e),
        }
        // Sleep some time to avoid hogging 100% CPU usage.
        tokio::time::sleep(Duration::from_millis(5)).await;
        let mut buf: [u8; SERIALIZED_INPUT_RECORD_0_LENGTH] = [0; SERIALIZED_INPUT_RECORD_0_LENGTH];
        match named_pipe_client.try_read(&mut buf) {
            Ok(read_bytes) => {
                if read_bytes != SERIALIZED_INPUT_RECORD_0_LENGTH {
                    // Seems to only happen if the pipe is closed/server disconnects
                    // indicating that the daemon has been closed.
                    // Exit the client too in that case.
                    break;
                }
            }
            Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                continue;
            }
            Err(e) => {
                println!("{}", e);
                continue;
            }
        }
        write_console_input(INPUT_RECORD_0::deserialize(&mut buf));
    }

    // Make sure the client and all its subprocesses
    // are aware they need to shutdown.
    unsafe {
        GenerateConsoleCtrlEvent(0, 0);
    }

    // Apparently calling wait is necessary on some systems,
    // so we'll just do it
    // https://doc.rust-lang.org/std/process/struct.Child.html#warning
    child.wait().expect("Failed to wait on child");
    drop(child);
}
