use std::{io, process};

use clap::Parser;
use dissh::utils::{get_console_input_buffer, wait_for_input};
use tokio::{io::Interest, net::windows::named_pipe::ClientOptions};
use whoami::username;
use win32console::console::WinConsole;
use windows::Win32::Foundation::GetLastError;
use windows::Win32::System::Console::{
    GetConsoleWindow, WriteConsoleInputW, INPUT_RECORD, INPUT_RECORD_0, KEY_EVENT,
};
use windows::Win32::UI::WindowsAndMessaging::MoveWindow;

use dissh::{
    serde::{deserialization::Deserialize, SERIALIZED_INPUT_RECORD_0_LENGTH},
    utils::constants::{PIPE_NAME, PKG_NAME},
    utils::debug::StringRepr,
};

/// Daemon CLI. Manages client consoles and user input
#[derive(Parser, Debug)]
#[clap(author, version, about, long_about = None)]
struct Args {
    /// Host(s) to connect to
    #[clap(required = true)]
    host: String,

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
    println!("Received input_record: {}", input_record.string_repr());
    let buffer: [INPUT_RECORD; 1] = [INPUT_RECORD {
        EventType: KEY_EVENT as u16,
        Event: input_record,
    }];
    let mut nb_of_events_written: u32 = 0;
    // FIXME: somehow the input record is not being written
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
    WinConsole::set_title(&format!("{} - {}@{}", PKG_NAME, username(), args.host))
        .expect("Failed to set console window title.");
    let hwnd = unsafe { GetConsoleWindow() };
    unsafe {
        MoveWindow(hwnd, args.x, args.y, args.width, args.height, true);
    }

    let sub = tokio::spawn(async {
        let named_pipe_client = ClientOptions::new().open(PIPE_NAME).unwrap();
        named_pipe_client.ready(Interest::READABLE).await.unwrap();

        // FIXME: only works for the first character for what ever reason..
        loop {
            let mut buf: [u8; SERIALIZED_INPUT_RECORD_0_LENGTH] =
                [0; SERIALIZED_INPUT_RECORD_0_LENGTH];
            match named_pipe_client.try_read(&mut buf) {
                Ok(read_bytes) => {
                    if read_bytes != SERIALIZED_INPUT_RECORD_0_LENGTH {
                        // Seems to only happen if the pipe is closed/server disconnects
                        // indicating that the daemon has been closed.
                        // Exit the client too in that case.
                        // FIXME: after the first record has been written this also
                        // doesn't work anymore, leaving me to assume that this thread
                        // dies silently afterwards.
                        process::exit(0);
                    }
                    println!("Received {read_bytes} bytes");
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
    });

    wait_for_input();
    drop(sub);
}
