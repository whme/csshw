use clap::Parser;
use std::{thread, time};
use whoami::username;
use win32console::console::WinConsole;
use windows::Win32::System::Console::GetConsoleWindow;
use windows::Win32::UI::WindowsAndMessaging::MoveWindow;

const PKG_NAME: &str = env!("CARGO_PKG_NAME");

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

fn main() {
    let args = Args::parse();
    WinConsole::set_title(&format!("{} - {}@{}", PKG_NAME, username(), args.host))
        .expect("Failed to set console window title.");
    unsafe {
        MoveWindow(
            GetConsoleWindow(),
            args.x,
            args.y,
            args.width,
            args.height,
            true,
        );
    }
    thread::sleep(time::Duration::from_millis(20000));
}
