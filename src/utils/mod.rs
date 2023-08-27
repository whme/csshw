use std::{mem, ptr, thread, time};

use windows::core::HSTRING;
use windows::Win32::Foundation::{COLORREF, HANDLE, RECT};
use windows::Win32::Graphics::Dwm::{DwmSetWindowAttribute, DWMWA_BORDER_COLOR};
use windows::Win32::System::Console::{
    FillConsoleOutputAttribute, GetConsoleScreenBufferInfo, GetConsoleWindow, GetStdHandle,
    ReadConsoleInputW, ScrollConsoleScreenBufferW, SetConsoleCursorPosition,
    SetConsoleTextAttribute, CHAR_INFO, CONSOLE_CHARACTER_ATTRIBUTES, CONSOLE_SCREEN_BUFFER_INFO,
    COORD, INPUT_RECORD, INPUT_RECORD_0, SMALL_RECT, STD_HANDLE, STD_INPUT_HANDLE,
    STD_OUTPUT_HANDLE,
};
use windows::Win32::UI::WindowsAndMessaging::{
    GetWindowRect, GetWindowTextW, MoveWindow, SetWindowTextW,
};

use self::constants::MAX_WINDOW_TITLE_LENGTH;

pub mod config;
pub mod constants;
pub mod debug;

const KEY_EVENT: u16 = 1;

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

pub fn set_console_color(color: CONSOLE_CHARACTER_ATTRIBUTES) {
    unsafe {
        SetConsoleTextAttribute(get_std_handle(STD_OUTPUT_HANDLE), color);
    }
    let mut number_of_attrs_written: u32 = 0;
    let mut buffer_info = CONSOLE_SCREEN_BUFFER_INFO::default();
    unsafe {
        GetConsoleScreenBufferInfo(get_std_handle(STD_OUTPUT_HANDLE), &mut buffer_info);
        for y in 0..buffer_info.dwSize.Y {
            FillConsoleOutputAttribute(
                get_std_handle(STD_OUTPUT_HANDLE),
                color.0,
                buffer_info.dwSize.X.try_into().unwrap(),
                COORD { X: 0, Y: y },
                &mut number_of_attrs_written,
            );
        }
    }
}

pub fn clear_screen() {
    let mut buffer_info = CONSOLE_SCREEN_BUFFER_INFO::default();
    let console_output_handle = get_std_handle(STD_OUTPUT_HANDLE);
    unsafe {
        GetConsoleScreenBufferInfo(console_output_handle, &mut buffer_info);
    }
    let scroll_rect = SMALL_RECT {
        Left: 0,
        Top: 0,
        Right: buffer_info.dwSize.X,
        Bottom: buffer_info.dwSize.Y,
    };
    let scroll_target = COORD {
        X: buffer_info.dwSize.X,
        Y: 0 - buffer_info.dwSize.Y,
    };
    let mut char_info = CHAR_INFO::default();
    char_info.Char.UnicodeChar = ' ' as u16;
    char_info.Attributes = buffer_info.wAttributes.0;

    unsafe {
        ScrollConsoleScreenBufferW(
            console_output_handle,
            &scroll_rect,
            None,
            scroll_target,
            &char_info,
        );
    }

    buffer_info.dwCursorPosition.X = 0;
    buffer_info.dwCursorPosition.Y = 0;

    unsafe {
        SetConsoleCursorPosition(console_output_handle, buffer_info.dwCursorPosition);
    }
}

pub fn set_console_border_color(color: COLORREF) {
    let version = os_info::get().version().to_string();
    let mut iter = version.split('.');
    let (major, _, build) = (
        iter.next().unwrap().parse::<usize>().unwrap(),
        iter.next().unwrap().parse::<usize>().unwrap(),
        iter.next().unwrap().parse::<usize>().unwrap(),
    );
    if major >= 10 && build >= 22000 {
        unsafe {
            DwmSetWindowAttribute(
                GetConsoleWindow(),
                DWMWA_BORDER_COLOR,
                &color as *const COLORREF as *const _,
                mem::size_of::<COLORREF>() as u32,
            )
            .unwrap();
        }
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
            .unwrap_or_else(|_| panic!("Failed to retrieve standard handle: {:?}", nstdhandle))
    };
}

pub fn get_console_input_buffer() -> HANDLE {
    return get_std_handle(STD_INPUT_HANDLE);
}

fn read_console_input() -> INPUT_RECORD {
    const NB_EVENTS: usize = 1;
    let mut input_buffer: [INPUT_RECORD; NB_EVENTS] = [INPUT_RECORD::default(); NB_EVENTS];
    let mut number_of_events_read = 0;
    loop {
        unsafe {
            ReadConsoleInputW(
                get_console_input_buffer(),
                &mut input_buffer,
                &mut number_of_events_read,
            )
            .expect("Failed to read console input");
        }
        if number_of_events_read == NB_EVENTS as u32 {
            break;
        }
    }
    return input_buffer[0];
}

pub fn read_keyboard_input() -> INPUT_RECORD_0 {
    loop {
        let input_record = read_console_input();
        match input_record.EventType {
            KEY_EVENT => {
                return input_record.Event;
            }
            _ => {
                continue;
            }
        }
    }
}

pub fn arrange_console(x: i32, y: i32, width: i32, height: i32) {
    // FIXME: sometimes a daemon or client console isn't being arrange correctly
    // when this simply retrying doesn't solve the issue. Maybe it has something to do
    // with DPI awareness => https://docs.rs/embed-manifest/latest/embed_manifest/
    unsafe {
        MoveWindow(GetConsoleWindow(), x, y, width, height, true);
    }
}
