use std::io;
use std::process::Command;
use std::time::Duration;

use clap::Parser;
use dissh::utils::constants::DEFAULT_SSH_USERNAME_KEY;
use dissh::utils::{get_console_input_buffer, set_console_title};
use tokio::net::windows::named_pipe::NamedPipeClient;
use tokio::{io::Interest, net::windows::named_pipe::ClientOptions};
use whoami::username;
use windows::Win32::Foundation::GetLastError;
use windows::Win32::System::Console::{
    GenerateConsoleCtrlEvent, GetConsoleWindow, WriteConsoleInputW, INPUT_RECORD, INPUT_RECORD_0,
    KEY_EVENT,
};
use windows::Win32::UI::WindowsAndMessaging::MoveWindow;

use dissh::{
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

    // Username used to connect to the hosts
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
    println!("{:?}", args);
    let hwnd = unsafe { GetConsoleWindow() };
    unsafe {
        MoveWindow(hwnd, args.x, args.y, args.width, args.height, true);
    }
    let host = args.host.clone();
    let username_host = if args.username.as_str() == DEFAULT_SSH_USERNAME_KEY {
        host
    } else {
        format!("{}@{}", args.username, host)
    };

    // TODO: make executable (ssh, wsl-distro, etc..) and args configurable
    let mut child = Command::new("ubuntu")
        .args([
            "run",
            format!(
                "source ~/.bash_profile; \
                ssh -XY {} || \
                [[ $? -eq 130 ]] || \
                read -n 1 -p 'Press a key to exit'",
                username_host
            )
            .as_str(),
        ])
        .spawn()
        .unwrap();

    // FIXME: wait until after the child has started before changing the title
    // TODO: instead of using args.host it would be nice to use the actual fqdn hostname
    // the ssh client will connect to ...
    set_console_title(format!("{} - {}@{}", PKG_NAME, username(), args.host).as_str());

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
