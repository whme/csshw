//! Utilities shared by daemon and client.

#![deny(clippy::implicit_return)]
#![allow(
    clippy::needless_return,
    clippy::doc_overindented_list_items,
    rustdoc::private_intra_doc_links
)]

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

#[cfg(test)]
use mockall::automock;

use self::constants::MAX_WINDOW_TITLE_LENGTH;

/// Trait for Windows API operations to enable mocking in tests.
#[cfg_attr(test, automock)]
pub trait WindowsApi {
    /// Sets the console window title.
    fn set_console_title(&self, title: &str) -> windows::core::Result<()>;

    /// Gets the console window title as UTF-16 buffer.
    fn get_console_title_utf16(&self, buffer: &mut [u16]) -> i32;

    /// Gets OS version string.
    fn get_os_version(&self) -> String;

    /// Arranges the console window position and size.
    fn arrange_console(&self, x: i32, y: i32, width: i32, height: i32)
        -> windows::core::Result<()>;

    /// Sets console text attribute.
    fn set_console_text_attribute(
        &self,
        attributes: CONSOLE_CHARACTER_ATTRIBUTES,
    ) -> windows::core::Result<()>;

    /// Gets console screen buffer info.
    fn get_console_screen_buffer_info(&self) -> windows::core::Result<CONSOLE_SCREEN_BUFFER_INFO>;

    /// Fills console output with specified attribute.
    fn fill_console_output_attribute(
        &self,
        attribute: u16,
        length: u32,
        coord: COORD,
    ) -> windows::core::Result<u32>;

    /// Scrolls console screen buffer.
    fn scroll_console_screen_buffer(
        &self,
        scroll_rect: SMALL_RECT,
        scroll_target: COORD,
        fill_char: CHAR_INFO,
    ) -> windows::core::Result<()>;

    /// Sets console cursor position.
    fn set_console_cursor_position(&self, position: COORD) -> windows::core::Result<()>;

    /// Gets standard handle.
    fn get_std_handle(&self, handle_type: STD_HANDLE) -> windows::core::Result<HANDLE>;

    /// Reads console input.
    fn read_console_input(&self, buffer: &mut [INPUT_RECORD]) -> windows::core::Result<u32>;

    /// Sets DWM window attribute for border color.
    fn set_dwm_border_color(&self, color: &COLORREF) -> windows::core::Result<()>;
}

/// Default implementation of WindowsApi that calls actual Windows APIs.
pub struct DefaultWindowsApi;

impl WindowsApi for DefaultWindowsApi {
    fn set_console_title(&self, title: &str) -> windows::core::Result<()> {
        return unsafe { SetWindowTextW(GetConsoleWindow(), &HSTRING::from(title)) };
    }

    fn get_console_title_utf16(&self, buffer: &mut [u16]) -> i32 {
        return unsafe { GetWindowTextW(GetConsoleWindow(), buffer) };
    }

    fn get_os_version(&self) -> String {
        return os_info::get().version().to_string();
    }

    fn arrange_console(
        &self,
        x: i32,
        y: i32,
        width: i32,
        height: i32,
    ) -> windows::core::Result<()> {
        return unsafe { MoveWindow(GetConsoleWindow(), x, y, width, height, true) };
    }

    fn set_console_text_attribute(
        &self,
        attributes: CONSOLE_CHARACTER_ATTRIBUTES,
    ) -> windows::core::Result<()> {
        return unsafe { SetConsoleTextAttribute(GetStdHandle(STD_OUTPUT_HANDLE)?, attributes) };
    }

    fn get_console_screen_buffer_info(&self) -> windows::core::Result<CONSOLE_SCREEN_BUFFER_INFO> {
        let mut buffer_info = CONSOLE_SCREEN_BUFFER_INFO::default();
        unsafe { GetConsoleScreenBufferInfo(GetStdHandle(STD_OUTPUT_HANDLE)?, &mut buffer_info)? };
        return Ok(buffer_info);
    }

    fn fill_console_output_attribute(
        &self,
        attribute: u16,
        length: u32,
        coord: COORD,
    ) -> windows::core::Result<u32> {
        let mut number_written = 0u32;
        unsafe {
            FillConsoleOutputAttribute(
                GetStdHandle(STD_OUTPUT_HANDLE)?,
                attribute,
                length,
                coord,
                &mut number_written,
            )?
        };
        return Ok(number_written);
    }

    fn scroll_console_screen_buffer(
        &self,
        scroll_rect: SMALL_RECT,
        scroll_target: COORD,
        fill_char: CHAR_INFO,
    ) -> windows::core::Result<()> {
        return unsafe {
            ScrollConsoleScreenBufferW(
                GetStdHandle(STD_OUTPUT_HANDLE)?,
                &scroll_rect,
                None,
                scroll_target,
                &fill_char,
            )
        };
    }

    fn set_console_cursor_position(&self, position: COORD) -> windows::core::Result<()> {
        return unsafe { SetConsoleCursorPosition(GetStdHandle(STD_OUTPUT_HANDLE)?, position) };
    }

    fn get_std_handle(&self, handle_type: STD_HANDLE) -> windows::core::Result<HANDLE> {
        return unsafe { GetStdHandle(handle_type) };
    }

    fn read_console_input(&self, buffer: &mut [INPUT_RECORD]) -> windows::core::Result<u32> {
        let mut number_read = 0u32;
        unsafe { ReadConsoleInputW(GetStdHandle(STD_INPUT_HANDLE)?, buffer, &mut number_read)? };
        return Ok(number_read);
    }

    fn set_dwm_border_color(&self, color: &COLORREF) -> windows::core::Result<()> {
        return unsafe {
            DwmSetWindowAttribute(
                GetConsoleWindow(),
                DWMWA_BORDER_COLOR,
                color as *const COLORREF as *const _,
                mem::size_of::<COLORREF>() as u32,
            )
        };
    }
}

/// Global instance of the Windows API implementation.
static DEFAULT_WINDOWS_API: DefaultWindowsApi = DefaultWindowsApi;

pub mod config;
pub mod constants;
pub mod debug;
pub mod named_pipe;

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
        println!("{window_rect:?}");
        thread::sleep(time::Duration::from_millis(100));
    }
}

/// Sets the window title of the current console window.
///
/// # Arguments
///
/// * `title` - The string to be set as window title.
pub fn set_console_title(title: &str) {
    return set_console_title_with_api(&DEFAULT_WINDOWS_API, title);
}

/// Sets the window title of the current console window using the provided API.
///
/// # Arguments
///
/// * `api` - The Windows API implementation to use.
/// * `title` - The string to be set as window title.
pub fn set_console_title_with_api(api: &dyn WindowsApi, title: &str) {
    api.set_console_title(title).unwrap();
}

/// Sets the back- and foreground color of the current console window.
///
/// # Arguments
///
/// * `color` - The color value describing the back- and foreground color.
pub fn set_console_color(color: CONSOLE_CHARACTER_ATTRIBUTES) {
    return set_console_color_with_api(&DEFAULT_WINDOWS_API, color);
}

/// Sets the back- and foreground color of the current console window using the provided API.
///
/// # Arguments
///
/// * `api` - The Windows API implementation to use.
/// * `color` - The color value describing the back- and foreground color.
pub fn set_console_color_with_api(api: &dyn WindowsApi, color: CONSOLE_CHARACTER_ATTRIBUTES) {
    api.set_console_text_attribute(color).unwrap();
    let buffer_info = api.get_console_screen_buffer_info().unwrap();
    for y in 0..buffer_info.dwSize.Y {
        api.fill_console_output_attribute(
            color.0,
            buffer_info.dwSize.X.try_into().unwrap(),
            COORD { X: 0, Y: y },
        )
        .unwrap();
    }
}

/// Empties the console screen output buffer of the current console window.
pub fn clear_screen() {
    return clear_screen_with_api(&DEFAULT_WINDOWS_API);
}

/// Empties the console screen output buffer of the current console window using the provided API.
///
/// # Arguments
///
/// * `api` - The Windows API implementation to use.
pub fn clear_screen_with_api(api: &dyn WindowsApi) {
    let buffer_info = api.get_console_screen_buffer_info().unwrap();
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

    api.scroll_console_screen_buffer(scroll_rect, scroll_target, char_info)
        .unwrap();

    let cursor_position = COORD { X: 0, Y: 0 };
    api.set_console_cursor_position(cursor_position).unwrap();
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
    return set_console_border_color_with_api(&DEFAULT_WINDOWS_API, color);
}

/// Sets the border color of the current console window using the provided APIs.
///
/// Windows10 does not support this.
///
/// # Arguments
///
/// * `api` - The Windows API implementation;
/// * `color` - RGB [COLORREF][1] to set as border color.
///
/// [1]: https://learn.microsoft.com/en-us/windows/win32/gdi/colorref
pub fn set_console_border_color_with_api(api: &dyn WindowsApi, color: COLORREF) {
    if !is_windows_10_with_api(api) {
        api.set_dwm_border_color(&color).unwrap();
    }
}

/// Returns the title of the current console window.
pub fn get_console_title() -> String {
    return get_console_title_with_api(&DEFAULT_WINDOWS_API);
}

/// Converts a UTF-16 buffer to a Rust String, filtering out null characters.
///
/// # Arguments
///
/// * `buffer` - The UTF-16 buffer to convert.
///
/// # Returns
///
/// The converted string.
pub fn utf16_buffer_to_string(buffer: &[u16]) -> String {
    let vec: Vec<u16> = buffer
        .iter()
        .copied()
        .filter(|val| return *val != 0u16)
        .collect();
    return String::from_utf16(&vec).unwrap_or_else(|err| {
        error!("{}", err);
        panic!("Failed to convert UTF-16 buffer to string, invalid utf16")
    });
}

/// Returns the title of the current console window using the provided API.
///
/// # Arguments
///
/// * `api` - The Windows API implementation to use.
///
/// # Returns
///
/// The title of the current console window.
pub fn get_console_title_with_api(api: &dyn WindowsApi) -> String {
    let mut title: [u16; MAX_WINDOW_TITLE_LENGTH] = [0; MAX_WINDOW_TITLE_LENGTH];
    api.get_console_title_utf16(&mut title);
    return utf16_buffer_to_string(&title);
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
    return get_window_title_with_api(&DEFAULT_WINDOWS_API, handle);
}

/// Returns the title of the window represented by the given window handle using the provided API.
///
/// # Arguments
///
/// * `api` - The Windows API implementation to use.
/// * `handle` - Reference to a window handle for which to retrieve the window title.
///
/// # Returns
///
/// The title of the window.
pub fn get_window_title_with_api(_api: &dyn WindowsApi, handle: &HWND) -> String {
    // For individual HWND, we use the direct Windows API since the trait is console-focused
    let mut title: [u16; MAX_WINDOW_TITLE_LENGTH] = [0; MAX_WINDOW_TITLE_LENGTH];
    unsafe { GetWindowTextW(*handle, &mut title) };
    let vec: Vec<u16> = title
        .into_iter()
        .filter(|val| return *val != 0u16)
        .collect();
    return String::from_utf16(&vec).unwrap_or_else(|err| {
        error!("{}", err);
        panic!("Failed to get window title, invalid utf16")
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
            .unwrap_or_else(|_| panic!("Failed to retrieve standard handle: {nstdhandle:?}"))
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

/// Returns a single [INPUT_RECORD] read from the current process stdinput using the provided API.
///
/// Blocks until 1 record was read.
///
/// # Arguments
///
/// * `api` - The Windows API implementation to use.
///
/// # Returns
///
/// A single INPUT_RECORD that was read.
pub fn read_console_input_with_api(api: &dyn WindowsApi) -> INPUT_RECORD {
    const NB_EVENTS: usize = 1;
    let mut input_buffer: [INPUT_RECORD; NB_EVENTS] = [INPUT_RECORD::default(); NB_EVENTS];
    loop {
        let number_of_events_read = api
            .read_console_input(&mut input_buffer)
            .expect("Failed to read console input");
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
    return read_keyboard_input_with_api(&DEFAULT_WINDOWS_API);
}

/// Returns a single [INPUT_RECORD_0] where `EventType` == [`KEY_EVENT`] using the provided API.
///
/// Blocks until 1 key event record was read.
///
/// # Arguments
///
/// * `api` - The Windows API implementation to use.
///
/// # Returns
///
/// A single INPUT_RECORD_0 with EventType == KEY_EVENT.
pub fn read_keyboard_input_with_api(api: &dyn WindowsApi) -> INPUT_RECORD_0 {
    loop {
        let input_record = read_console_input_with_api(api);
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
    return arrange_console_with_api(&DEFAULT_WINDOWS_API, x, y, width, height);
}

/// Changes size and position of the current console window using the provided API.
///
/// # Arguments
///
/// * `api` - The Windows API implementation to use.
/// * `x`       - The x coordinate to move the window to.
///               From the top left corner of the screen.
/// * `y`       - The y coordinate to move the window to.
///               From the top left corner of the screen.
/// * `width`   - The width in pixels to resize the window to.
///               In logical scaling.
/// * `height`  - The height in pixels to resize the window to.
///               In logical scaling.
pub fn arrange_console_with_api(api: &dyn WindowsApi, x: i32, y: i32, width: i32, height: i32) {
    // FIXME: sometimes a daemon or client console isn't being arrange correctly
    // when this simply retrying doesn't solve the issue. Maybe it has something to do
    // with DPI awareness => https://docs.rs/embed-manifest/latest/embed_manifest/
    api.arrange_console(x, y, width, height).unwrap();
}

/// Detects if the current windows installation is Windows 10 or not.
///
/// Uses the os version, Windows 10 is < `10._.22000`. Windows 11 started with build 22000.
///
/// # Returns
///
/// Whether the current windows installation is Windows 10 or not.
pub fn is_windows_10() -> bool {
    return is_windows_10_with_api(&DEFAULT_WINDOWS_API);
}

/// Detects if the current windows installation is Windows 10 or not using the provided API.
///
/// Uses the os version, Windows 10 is < `10._.22000`. Windows 11 started with build 22000.
///
/// # Arguments
///
/// * `api` - The Windows API implementation to use.
///
/// # Returns
///
/// Whether the current windows installation is Windows 10 or not.
pub fn is_windows_10_with_api(api: &dyn WindowsApi) -> bool {
    let version = api.get_os_version();
    let mut iter = version.split('.');
    let (major, _, build) = (
        iter.next().unwrap().parse::<usize>().unwrap(),
        iter.next().unwrap().parse::<usize>().unwrap(),
        iter.next().unwrap().parse::<usize>().unwrap(),
    );
    return major < 10 || (major == 10 && build < 22000);
}

#[cfg(test)]
#[path = "../tests/utils/test_mod.rs"]
mod test_mod;
