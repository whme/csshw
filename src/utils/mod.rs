use std::{ptr, thread, time};

use windows::core::HSTRING;
use windows::Win32::Foundation::{HANDLE, RECT};
use windows::Win32::System::Console::{
    GetConsoleTitleW, GetConsoleWindow, GetStdHandle, STD_HANDLE, STD_INPUT_HANDLE,
};
use windows::Win32::UI::WindowsAndMessaging::{GetWindowRect, SetWindowTextW};

pub mod constants;
pub mod debug;

pub fn print_console_rect() {
    loop {
        let mut window_rect = RECT::default();
        unsafe { GetWindowRect(GetConsoleWindow(), ptr::addr_of_mut!(window_rect)) };
        println!("{:?}", window_rect);
        thread::sleep(time::Duration::from_millis(100));
    }
}

pub fn set_console_title(title: &str) {
    unsafe {
        SetWindowTextW(GetConsoleWindow(), &HSTRING::from(title));
    }
}

pub fn get_console_title() -> String {
    let mut title: Vec<u16> = Vec::new();
    unsafe {
        GetConsoleTitleW(&mut title);
    }
    return String::from_utf16(&title).expect("Failed to get console title");
}

fn get_std_handle(nstdhandle: STD_HANDLE) -> HANDLE {
    return unsafe {
        GetStdHandle(nstdhandle)
            .expect(format!("Failed to retrieve standard handle: {:?}", nstdhandle).as_str())
    };
}

pub fn get_console_input_buffer() -> HANDLE {
    return get_std_handle(STD_INPUT_HANDLE);
}
