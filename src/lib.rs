use std::{ptr, thread, time};

use windows::Win32::{
    Foundation::RECT, System::Console::GetConsoleWindow, UI::WindowsAndMessaging::GetWindowRect,
};

pub const PKG_NAME: &str = env!("CARGO_PKG_NAME");

pub fn print_console_rect() {
    loop {
        let mut window_rect = RECT::default();
        unsafe { GetWindowRect(GetConsoleWindow(), ptr::addr_of_mut!(window_rect)) };
        println!("{:?}", window_rect);
        thread::sleep(time::Duration::from_millis(100));
    }
}

pub fn wait_for_input() {
    println!("Waiting for input ...");
    loop {
        let mut line = String::new();
        std::io::stdin().read_line(&mut line).unwrap();
        println!("Read: {:?}", line);
        if line.as_str().to_lowercase() == "exit\r\n" {
            println!("Exiting in 5 sec ...");
            thread::sleep(time::Duration::from_secs(5));
            break;
        }
    }
}
