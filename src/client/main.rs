use clap::Parser;
use std::{ptr, thread, time};
use whoami::username;
use win32console::console::WinConsole;
use windows::Win32::Foundation::RECT;
use windows::Win32::System::Console::GetConsoleWindow;
use windows::Win32::UI::WindowsAndMessaging::{GetWindowRect, MoveWindow};

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
    println!("{:?}", args);
    WinConsole::set_title(&format!("{} - {}@{}", PKG_NAME, username(), args.host))
        .expect("Failed to set console window title.");
    let hwnd = unsafe { GetConsoleWindow() };
    unsafe {
        MoveWindow(hwnd, args.x, args.y, args.width, args.height, true);
    }
    let mut i = 0;
    loop {
        i = i + 1;
        let mut window_rect = RECT::default();
        unsafe { GetWindowRect(hwnd, ptr::addr_of_mut!(window_rect)) };
        println!("{:?}", window_rect);
        if i > 2000 {
            break;
        };
        thread::sleep(time::Duration::from_millis(100));
    }
}
