use std::{ptr, thread, time};

use windows::core::HSTRING;
use windows::Win32::Foundation::{HANDLE, RECT};
use windows::Win32::System::Console::{
    GetConsoleWindow, GetStdHandle, STD_HANDLE, STD_INPUT_HANDLE,
};
use windows::Win32::UI::WindowsAndMessaging::{GetWindowRect, GetWindowTextW, SetWindowTextW};

use self::constants::MAX_WINDOW_TITLE_LENGTH;

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
    let mut title: [u16; MAX_WINDOW_TITLE_LENGTH] = [0; MAX_WINDOW_TITLE_LENGTH];
    let read_chars: i32;
    unsafe {
        read_chars = GetWindowTextW(GetConsoleWindow(), &mut title);
    }
    let mut read_title = title.to_vec();
    read_title.truncate(read_chars.try_into().unwrap());
    return String::from_utf16(&read_title)
        .expect("Failed to get console title")
        .trim()
        .to_string();
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
