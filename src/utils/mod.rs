//! Utilities shared by daemon and client.

#![deny(clippy::implicit_return)]
#![allow(clippy::needless_return, clippy::doc_overindented_list_items)]

use log::error;
use std::{mem, ptr, thread, time};

use windows::core::HSTRING;
use windows::Win32::Foundation::{COLORREF, HANDLE, HWND, RECT};
use windows::Win32::Graphics::Dwm::{DwmSetWindowAttribute, DWMWA_BORDER_COLOR};
use windows::Win32::System::Console::{
    FillConsoleOutputAttribute, GetConsoleScreenBufferInfo, GetConsoleWindow, GetStdHandle,
    ReadConsoleInputW, SetConsoleTextAttribute, CONSOLE_CHARACTER_ATTRIBUTES,
    CONSOLE_SCREEN_BUFFER_INFO, COORD, INPUT_RECORD, INPUT_RECORD_0, STD_HANDLE, STD_INPUT_HANDLE,
    STD_OUTPUT_HANDLE,
};
use windows::Win32::System::Console::{
    ScrollConsoleScreenBufferW, SetConsoleCursorPosition, CHAR_INFO, KEY_EVENT as KEY_EVENT_U32,
    SMALL_RECT,
};
use windows::Win32::UI::WindowsAndMessaging::{
    GetWindowRect, GetWindowTextW, MoveWindow, SetWindowTextW,
};

use self::constants::MAX_WINDOW_TITLE_LENGTH;

pub mod config;
pub mod constants;
pub mod debug;

/// u16 representation of a [KEY_EVENT][1].
///
/// For some reason the public [KEY_EVENT][1] constant is a u32
/// while the [INPUT_RECORD][2].`EventType` is u16...
///
/// [1]: https://microsoft.github.io/windows-docs-rs/doc/windows/Win32/System/Console/constant.KEY_EVENT.html
/// [2]: https://microsoft.github.io/windows-docs-rs/doc/windows/Win32/System/Console/struct.INPUT_RECORD.html
const KEY_EVENT: u16 = KEY_EVENT_U32 as u16;

/// Continously prints the window rectangle of the current console window.
///
/// Intended use for debugging only.
pub fn print_console_rect() {
    loop {
        let mut window_rect = RECT::default();
        unsafe { GetWindowRect(GetConsoleWindow(), ptr::addr_of_mut!(window_rect)).unwrap() };
        println!("{:?}", window_rect);
        thread::sleep(time::Duration::from_millis(100));
    }
}

/// Sets the window title of the current console window.
///
/// # Arguments
///
/// * `title` - The string to be set as window title.
pub fn set_console_title(title: &str) {
    unsafe {
        SetWindowTextW(GetConsoleWindow(), &HSTRING::from(title)).unwrap();
    }
}

/// Sets the back- and foreground color of the current console window.
///
/// # Arguments
///
/// * `color` - The color value describing the back- and foreground color.
pub fn set_console_color(color: CONSOLE_CHARACTER_ATTRIBUTES) {
    unsafe {
        SetConsoleTextAttribute(get_console_output_buffer(), color).unwrap();
    }
    let mut number_of_attrs_written: u32 = 0;
    let mut buffer_info = CONSOLE_SCREEN_BUFFER_INFO::default();
    unsafe {
        GetConsoleScreenBufferInfo(get_console_output_buffer(), &mut buffer_info).unwrap();
        for y in 0..buffer_info.dwSize.Y {
            FillConsoleOutputAttribute(
                get_console_output_buffer(),
                color.0,
                buffer_info.dwSize.X.try_into().unwrap(),
                COORD { X: 0, Y: y },
                &mut number_of_attrs_written,
            )
            .unwrap();
        }
    }
}

/// Empties the console screen output buffer of the current console window.
pub fn clear_screen() {
    let mut buffer_info = CONSOLE_SCREEN_BUFFER_INFO::default();
    let console_output_handle = get_console_output_buffer();
    unsafe {
        GetConsoleScreenBufferInfo(console_output_handle, &mut buffer_info).unwrap();
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
        )
        .unwrap();
    }

    buffer_info.dwCursorPosition.X = 0;
    buffer_info.dwCursorPosition.Y = 0;

    unsafe {
        SetConsoleCursorPosition(console_output_handle, buffer_info.dwCursorPosition).unwrap();
    }
}

/// Sets the border color of the current console window.
///
/// Windows10 does not support this.
///
/// # Arguments
///
/// * `color` - RGB [COLORREF][1] to set as border color.
///
/// # Examples
///
/// ```
/// use csshw_lib::utils::set_console_border_color;
/// use windows::Win32::Foundation::COLORREF;
///
/// // Note: inversed order of RGB        BBGGRR
/// set_console_border_color(COLORREF(0x001A2B3C));
/// ```
///
/// [1]: https://learn.microsoft.com/en-us/windows/win32/gdi/colorref
pub fn set_console_border_color(color: COLORREF) {
    if !is_windows_10() {
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

/// Returns the title of the current console window.
pub fn get_console_title() -> String {
    return get_window_title(unsafe { &GetConsoleWindow() });
}

/// Returns the title of the window represented by the given window handle [HWND].
///
/// # Arguments
///
/// * `handle` - Reference to a window handle for which to retrieve the window title.
///
/// # Returns
///
/// The title of the window.
pub fn get_window_title(handle: &HWND) -> String {
    let mut title: [u16; MAX_WINDOW_TITLE_LENGTH] = [0; MAX_WINDOW_TITLE_LENGTH];
    unsafe {
        GetWindowTextW(*handle, &mut title);
    }
    let vec: Vec<u16> = title
        .into_iter()
        .filter(|val| return *val != 0u16)
        .collect();
    return String::from_utf16(&vec).unwrap_or_else(|err| {
        error!("{}", err);
        panic!("Failed to get window title, invalid utf16",)
    });
}

/// Returns a [HANDLE] to the requested [STD_HANDLE] of the current process.
///
/// # Arguments
///
/// * `nstdhandle` - The standard handle to retrieve.
///                  Either [STD_INPUT_HANDLE] or [STD_OUTPUT_HANDLE].
///
/// # Returns
///
/// The [HANDLE] to the requested [STD_HANDLE].
fn get_std_handle(nstdhandle: STD_HANDLE) -> HANDLE {
    return unsafe {
        GetStdHandle(nstdhandle)
            .unwrap_or_else(|_| panic!("Failed to retrieve standard handle: {:?}", nstdhandle))
    };
}

/// Returns a [HANDLE] to the [STD_INPUT_HANDLE] of the current process.
pub fn get_console_input_buffer() -> HANDLE {
    return get_std_handle(STD_INPUT_HANDLE);
}

/// Returns a [HANDLE] to the [STD_OUTPUT_HANDLE] of the current process.
pub fn get_console_output_buffer() -> HANDLE {
    return get_std_handle(STD_OUTPUT_HANDLE);
}

/// Returns a single [INPUT_RECORD] read from the current process stdinput.
///
/// Blocks until 1 record was read.
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

#[allow(rustdoc::private_intra_doc_links)]
/// Returns a single [INPUT_RECORD_0] where `EventType` == [`KEY_EVENT`].
///
/// Blocks until 1 key event record was read.
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

/// Changes size and position of the current console window.
///
/// # Arguments
///
/// * `x`       - The x coordinate to move the window to.
///               From the top left corner of the screen.
/// * `y`       - The y coordinate to move the window to.
///               From the top left corner of the screen.
/// * `width`   - The width in pixels to resize the window to.
///               In logical scaling.
/// * `height`  - The height in pixels to resize the window to.
///               In logical scaling.
pub fn arrange_console(x: i32, y: i32, width: i32, height: i32) {
    // FIXME: sometimes a daemon or client console isn't being arrange correctly
    // when this simply retrying doesn't solve the issue. Maybe it has something to do
    // with DPI awareness => https://docs.rs/embed-manifest/latest/embed_manifest/
    unsafe {
        MoveWindow(GetConsoleWindow(), x, y, width, height, true).unwrap();
    }
}

/// Detects if the current windows installation is Windows 10 or not.
///
/// Uses the os version, Windows 10 is <= `10._.22000`.
///
/// # Returns
///
/// Whether the current windows installation is Windows 10 or not.
pub fn is_windows_10() -> bool {
    let version = os_info::get().version().to_string();
    let mut iter = version.split('.');
    let (major, _, build) = (
        iter.next().unwrap().parse::<usize>().unwrap(),
        iter.next().unwrap().parse::<usize>().unwrap(),
        iter.next().unwrap().parse::<usize>().unwrap(),
    );
    return major <= 10 && build <= 22000;
}

#[cfg(test)]
#[path = "../tests/test_utils.rs"]
mod test_utils;
