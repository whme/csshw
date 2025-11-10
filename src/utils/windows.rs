//! Windows API abstraction layer for console and system operations.
//!
//! This module provides a trait-based abstraction over Windows APIs to enable
//! mocking in tests and centralize Windows-specific functionality.

#![deny(clippy::implicit_return)]
#![allow(
    clippy::needless_return,
    clippy::doc_overindented_list_items,
    rustdoc::private_intra_doc_links
)]

use log::error;
use std::ffi::OsString;
use std::os::windows::ffi::OsStrExt;
use std::{mem, ptr};

use windows::core::{HSTRING, PCWSTR};
use windows::Win32::Foundation::{BOOL, COLORREF, FALSE, HANDLE, HWND, LPARAM, TRUE};
use windows::Win32::Graphics::Dwm::{DwmSetWindowAttribute, DWMWA_BORDER_COLOR};
use windows::Win32::System::Com::{CoCreateInstance, CLSCTX_ALL};
use windows::Win32::System::Console::{
    FillConsoleOutputAttribute, GetConsoleScreenBufferInfo, GetConsoleWindow, GetStdHandle,
    ReadConsoleInputW, SetConsoleTextAttribute, CONSOLE_CHARACTER_ATTRIBUTES,
    CONSOLE_SCREEN_BUFFER_INFO, COORD, INPUT_RECORD, INPUT_RECORD_0, STD_HANDLE, STD_INPUT_HANDLE,
    STD_OUTPUT_HANDLE,
};
use windows::Win32::System::Console::{GetConsoleMode, SetConsoleMode, CONSOLE_MODE};
use windows::Win32::System::Console::{
    ScrollConsoleScreenBufferW, SetConsoleCursorPosition, CHAR_INFO, KEY_EVENT as KEY_EVENT_U32,
    SMALL_RECT,
};
use windows::Win32::System::Threading::PROCESS_ACCESS_RIGHTS;
use windows::Win32::System::Threading::{
    CreateProcessW, CREATE_NEW_CONSOLE, PROCESS_INFORMATION, STARTUPINFOW,
};
use windows::Win32::System::Threading::{GetExitCodeProcess, OpenProcess};
use windows::Win32::UI::Accessibility::{CUIAutomation, IUIAutomation};
use windows::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GetWindowTextW, GetWindowThreadProcessId, MoveWindow, SetWindowTextW,
    SYSTEM_METRICS_INDEX,
};
use windows::Win32::UI::WindowsAndMessaging::{
    GetForegroundWindow, GetWindowPlacement, SetForegroundWindow, ShowWindow, SHOW_WINDOW_CMD,
    WINDOWPLACEMENT,
};

#[cfg(test)]
use mockall::automock;

use super::constants::MAX_WINDOW_TITLE_LENGTH;

/// Trait for Windows API operations to enable mocking in tests.
///
/// This trait abstracts Windows API calls to allow for unit testing without
/// actual system interaction. All console and system operations should go
/// through this trait.
#[cfg_attr(test, automock)]
pub trait WindowsApi: Send + Sync {
    /// Sets the console window title.
    ///
    /// # Arguments
    ///
    /// * `title` - The string to be set as window title
    ///
    /// # Returns
    ///
    /// Result indicating success or failure of the operation
    fn set_console_title(&self, title: &str) -> windows::core::Result<()>;

    /// Gets the console window title as UTF-16 buffer.
    ///
    /// # Arguments
    ///
    /// * `buffer` - Mutable buffer to store the UTF-16 encoded title
    ///
    /// # Returns
    ///
    /// Number of characters copied to the buffer
    fn get_console_title(&self, buffer: &mut [u16]) -> i32;

    /// Gets OS version string.
    ///
    /// # Returns
    ///
    /// String representation of the OS version
    fn get_os_version(&self) -> String;

    /// Arranges the console window position and size.
    ///
    /// # Arguments
    ///
    /// * `x` - The x coordinate to move the window to
    /// * `y` - The y coordinate to move the window to
    /// * `width` - The width in pixels to resize the window to
    /// * `height` - The height in pixels to resize the window to
    ///
    /// # Returns
    ///
    /// Result indicating success or failure of the operation
    fn arrange_console(&self, x: i32, y: i32, width: i32, height: i32)
        -> windows::core::Result<()>;

    /// Sets console text attribute.
    ///
    /// # Arguments
    ///
    /// * `attributes` - Console character attributes to set
    ///
    /// # Returns
    ///
    /// Result indicating success or failure of the operation
    fn set_console_text_attribute(
        &self,
        attributes: CONSOLE_CHARACTER_ATTRIBUTES,
    ) -> windows::core::Result<()>;

    /// Gets console screen buffer info.
    ///
    /// # Returns
    ///
    /// Console screen buffer information or error
    fn get_console_screen_buffer_info(&self) -> windows::core::Result<CONSOLE_SCREEN_BUFFER_INFO>;

    /// Fills console output with specified attribute.
    ///
    /// # Arguments
    ///
    /// * `attribute` - Attribute to fill with
    /// * `length` - Number of characters to fill
    /// * `coord` - Starting coordinate
    ///
    /// # Returns
    ///
    /// Number of characters actually filled or error
    fn fill_console_output_attribute(
        &self,
        attribute: u16,
        length: u32,
        coord: COORD,
    ) -> windows::core::Result<u32>;

    /// Scrolls console screen buffer.
    ///
    /// # Arguments
    ///
    /// * `scroll_rect` - Rectangle to scroll
    /// * `scroll_target` - Target coordinate for scrolling
    /// * `fill_char` - Character to fill empty space with
    ///
    /// # Returns
    ///
    /// Result indicating success or failure of the operation
    fn scroll_console_screen_buffer(
        &self,
        scroll_rect: SMALL_RECT,
        scroll_target: COORD,
        fill_char: CHAR_INFO,
    ) -> windows::core::Result<()>;

    /// Sets console cursor position.
    ///
    /// # Arguments
    ///
    /// * `position` - New cursor position
    ///
    /// # Returns
    ///
    /// Result indicating success or failure of the operation
    fn set_console_cursor_position(&self, position: COORD) -> windows::core::Result<()>;

    /// Gets standard handle.
    ///
    /// # Arguments
    ///
    /// * `handle_type` - Type of standard handle to retrieve
    ///
    /// # Returns
    ///
    /// Handle to the requested standard device or error
    fn get_std_handle(&self, handle_type: STD_HANDLE) -> windows::core::Result<HANDLE>;

    /// Reads console input.
    ///
    /// # Arguments
    ///
    /// * `buffer` - Buffer to store input records
    ///
    /// # Returns
    ///
    /// Number of records read or error
    fn read_console_input(&self, buffer: &mut [INPUT_RECORD]) -> windows::core::Result<u32>;

    /// Sets DWM window attribute for border color.
    ///
    /// # Arguments
    ///
    /// * `color` - Color to set as border color
    ///
    /// # Returns
    ///
    /// Result indicating success or failure of the operation
    fn set_console_border_color(&self, color: &COLORREF) -> windows::core::Result<()>;

    /// Writes input records to the console input buffer.
    ///
    /// # Arguments
    ///
    /// * `buffer` - Input records to write
    /// * `number_written` - Mutable reference to store number of records written
    ///
    /// # Returns
    ///
    /// Result indicating success or failure of the operation
    fn write_console_input(
        &self,
        buffer: &[INPUT_RECORD],
        number_written: &mut u32,
    ) -> windows::core::Result<()>;

    /// Gets the last Windows error code.
    ///
    /// # Returns
    ///
    /// The last error code from Windows API
    fn get_last_error(&self) -> u32;

    /// Generates a console control event.
    ///
    /// # Arguments
    ///
    /// * `ctrl_event` - Control event type to generate
    /// * `process_group_id` - Process group ID to send event to
    ///
    /// # Returns
    ///
    /// Result indicating success or failure of the operation
    fn generate_console_ctrl_event(
        &self,
        ctrl_event: u32,
        process_group_id: u32,
    ) -> windows::core::Result<()>;

    /// Get standard output handle
    ///
    /// # Returns
    ///
    /// Handle to standard output or error
    fn get_stdout_handle(&self) -> windows::core::Result<HANDLE>;

    /// Get console screen buffer information
    ///
    /// # Arguments
    ///
    /// * `handle` - Handle to console screen buffer
    ///
    /// # Returns
    ///
    /// Console screen buffer information or error
    fn get_console_screen_buffer_info_with_handle(
        &self,
        handle: HANDLE,
    ) -> windows::core::Result<CONSOLE_SCREEN_BUFFER_INFO>;

    /// Create a new process
    ///
    /// # Arguments
    ///
    /// * `application` - Application name including file extension
    /// * `args` - List of arguments to the application
    ///
    /// # Returns
    ///
    /// Process information if successful, None otherwise
    fn create_process_with_args(
        &self,
        application: &str,
        args: Vec<String>,
    ) -> Option<windows::Win32::System::Threading::PROCESS_INFORMATION> {
        let command_line = build_command_line(application, &args);
        let mut startupinfo = STARTUPINFOW {
            cb: mem::size_of::<STARTUPINFOW>() as u32,
            ..Default::default()
        };
        let mut process_information = PROCESS_INFORMATION::default();
        let mut cmd_line = command_line;
        let command_line_ptr = windows::core::PWSTR(cmd_line.as_mut_ptr());

        match self.create_process_raw(
            application,
            command_line_ptr,
            &mut startupinfo,
            &mut process_information,
        ) {
            Ok(()) => return Some(process_information),
            Err(_) => return None,
        }
    }

    /// Low-level process creation API call
    ///
    /// # Arguments
    ///
    /// * `application` - Application name
    /// * `command_line` - Command line string as PWSTR
    /// * `startup_info` - Startup information structure
    /// * `process_info` - Process information structure to fill
    ///
    /// # Returns
    ///
    /// Result indicating success or failure of the operation
    fn create_process_raw(
        &self,
        application: &str,
        command_line: windows::core::PWSTR,
        startup_info: &mut windows::Win32::System::Threading::STARTUPINFOW,
        process_info: &mut windows::Win32::System::Threading::PROCESS_INFORMATION,
    ) -> windows::core::Result<()>;

    /// Get window handle for process ID
    ///
    /// # Arguments
    ///
    /// * `process_id` - Process ID to find window for
    ///
    /// # Returns
    ///
    /// Window handle for the process
    fn get_window_handle_for_process(&self, process_id: u32) -> HWND;

    /// Gets the console window handle.
    ///
    /// # Returns
    ///
    /// Handle to the console window
    fn get_console_window(&self) -> HWND;

    /// Gets the foreground window handle.
    ///
    /// # Returns
    ///
    /// Handle to the foreground window
    fn get_foreground_window(&self) -> HWND;

    /// Sets the foreground window.
    ///
    /// # Arguments
    ///
    /// * `hwnd` - Handle to the window to set as foreground
    ///
    /// # Returns
    ///
    /// Result indicating success or failure of the operation
    fn set_foreground_window(&self, hwnd: HWND) -> windows::core::Result<()>;

    /// Gets console mode for the specified handle.
    ///
    /// # Arguments
    ///
    /// * `handle` - Handle to the console input buffer
    ///
    /// # Returns
    ///
    /// Console mode or error
    fn get_console_mode(&self, handle: HANDLE) -> windows::core::Result<CONSOLE_MODE>;

    /// Sets console mode for the specified handle.
    ///
    /// # Arguments
    ///
    /// * `handle` - Handle to the console input buffer
    /// * `mode` - Console mode to set
    ///
    /// # Returns
    ///
    /// Result indicating success or failure of the operation
    fn set_console_mode(&self, handle: HANDLE, mode: CONSOLE_MODE) -> windows::core::Result<()>;

    /// Gets the exit code of the specified process.
    ///
    /// # Arguments
    ///
    /// * `handle` - Handle to the process
    ///
    /// # Returns
    ///
    /// Exit code or error
    fn get_exit_code(&self, handle: HANDLE) -> windows::core::Result<u32>;

    /// Moves and resizes a window.
    ///
    /// # Arguments
    ///
    /// * `hwnd` - Handle to the window
    /// * `x` - New x position
    /// * `y` - New y position
    /// * `width` - New width
    /// * `height` - New height
    /// * `repaint` - Whether to repaint the window
    ///
    /// # Returns
    ///
    /// Result indicating success or failure of the operation
    fn move_window(
        &self,
        hwnd: HWND,
        x: i32,
        y: i32,
        width: i32,
        height: i32,
        repaint: bool,
    ) -> windows::core::Result<()>;

    /// Gets window placement information.
    ///
    /// # Arguments
    ///
    /// * `hwnd` - Handle to the window
    ///
    /// # Returns
    ///
    /// Window placement information or error
    fn get_window_placement(&self, hwnd: HWND) -> windows::core::Result<WINDOWPLACEMENT>;

    /// Shows a window in the specified state.
    ///
    /// # Arguments
    ///
    /// * `hwnd` - Handle to the window
    /// * `cmd_show` - Show command
    ///
    /// # Returns
    ///
    /// Result indicating success or failure of the operation
    fn show_window(&self, hwnd: HWND, cmd_show: SHOW_WINDOW_CMD) -> windows::core::Result<bool>;

    /// Focuses a window using UI Automation.
    ///
    /// # Arguments
    ///
    /// * `hwnd` - Handle to the window to focus
    ///
    /// # Returns
    ///
    /// Result indicating success or failure of the operation
    fn focus_window_with_automation(&self, hwnd: HWND) -> windows::core::Result<()>;

    /// Checks if a window handle is valid.
    ///
    /// # Arguments
    ///
    /// * `hwnd` - Handle to the window to check
    ///
    /// # Returns
    ///
    /// True if the window is valid, false otherwise
    fn is_window(&self, hwnd: HWND) -> bool;

    /// Opens a process with the specified access rights.
    ///
    /// # Arguments
    ///
    /// * `access` - Access rights for the process handle
    /// * `inherit` - Whether the handle can be inherited
    /// * `process_id` - Process ID to open
    ///
    /// # Returns
    ///
    /// Process handle or error
    fn open_process(
        &self,
        access: u32,
        inherit: bool,
        process_id: u32,
    ) -> windows::core::Result<HANDLE>;

    /// Initializes the COM library for use by the calling thread.
    ///
    /// # Arguments
    ///
    /// * `coinit` - Initialization options for the COM library
    ///
    /// # Returns
    ///
    /// Result indicating success or failure of the operation
    fn initialize_com_library(
        &self,
        coinit: windows::Win32::System::Com::COINIT,
    ) -> windows::core::Result<()>;

    /// Gets system metrics information.
    ///
    /// # Arguments
    ///
    /// * `index` - System metric index to retrieve
    ///
    /// # Returns
    ///
    /// The requested system metric value
    fn get_system_metrics(&self, index: SYSTEM_METRICS_INDEX) -> i32;

    /// Sets the process DPI awareness.
    ///
    /// # Arguments
    ///
    /// * `value` - DPI awareness value to set
    ///
    /// # Returns
    ///
    /// Result indicating success or failure of the operation
    fn set_process_dpi_awareness(
        &self,
        value: windows::Win32::UI::HiDpi::PROCESS_DPI_AWARENESS,
    ) -> windows::core::Result<()>;
}

#[cfg(test)]
impl Clone for MockWindowsApi {
    fn clone(&self) -> Self {
        return MockWindowsApi::new();
    }
}

/// Default implementation of WindowsApi that calls actual Windows APIs.
///
/// This implementation provides direct access to Windows system APIs and should
/// be used in production code. For testing, use the MockWindowsApi instead.
#[derive(Clone)]
pub struct DefaultWindowsApi;

impl WindowsApi for DefaultWindowsApi {
    fn set_console_title(&self, title: &str) -> windows::core::Result<()> {
        return unsafe { SetWindowTextW(GetConsoleWindow(), &HSTRING::from(title)) };
    }

    fn get_console_title(&self, buffer: &mut [u16]) -> i32 {
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
        return unsafe { SetConsoleTextAttribute(self.get_stdout_handle()?, attributes) };
    }

    fn get_console_screen_buffer_info(&self) -> windows::core::Result<CONSOLE_SCREEN_BUFFER_INFO> {
        let mut buffer_info = CONSOLE_SCREEN_BUFFER_INFO::default();
        unsafe { GetConsoleScreenBufferInfo(self.get_stdout_handle()?, &mut buffer_info)? };
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
                self.get_stdout_handle()?,
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
                self.get_stdout_handle()?,
                &scroll_rect,
                None,
                scroll_target,
                &fill_char,
            )
        };
    }

    fn set_console_cursor_position(&self, position: COORD) -> windows::core::Result<()> {
        return unsafe { SetConsoleCursorPosition(self.get_stdout_handle()?, position) };
    }

    fn get_std_handle(&self, handle_type: STD_HANDLE) -> windows::core::Result<HANDLE> {
        return unsafe { GetStdHandle(handle_type) };
    }

    fn read_console_input(&self, buffer: &mut [INPUT_RECORD]) -> windows::core::Result<u32> {
        let mut number_read = 0u32;
        unsafe {
            ReadConsoleInputW(
                self.get_std_handle(STD_INPUT_HANDLE)?,
                buffer,
                &mut number_read,
            )?
        };
        return Ok(number_read);
    }

    fn set_console_border_color(&self, color: &COLORREF) -> windows::core::Result<()> {
        return unsafe {
            DwmSetWindowAttribute(
                GetConsoleWindow(),
                DWMWA_BORDER_COLOR,
                color as *const COLORREF as *const _,
                mem::size_of::<COLORREF>() as u32,
            )
        };
    }

    fn write_console_input(
        &self,
        buffer: &[INPUT_RECORD],
        number_written: &mut u32,
    ) -> windows::core::Result<()> {
        unsafe {
            windows::Win32::System::Console::WriteConsoleInputW(
                GetStdHandle(STD_INPUT_HANDLE)?,
                buffer,
                number_written,
            )?
        };
        return Ok(());
    }

    fn get_last_error(&self) -> u32 {
        return unsafe { windows::Win32::Foundation::GetLastError().0 };
    }

    fn generate_console_ctrl_event(
        &self,
        ctrl_event: u32,
        process_group_id: u32,
    ) -> windows::core::Result<()> {
        return unsafe {
            windows::Win32::System::Console::GenerateConsoleCtrlEvent(ctrl_event, process_group_id)
        };
    }

    fn get_stdout_handle(&self) -> windows::core::Result<HANDLE> {
        return self.get_std_handle(STD_OUTPUT_HANDLE);
    }

    fn get_console_screen_buffer_info_with_handle(
        &self,
        handle: HANDLE,
    ) -> windows::core::Result<CONSOLE_SCREEN_BUFFER_INFO> {
        let mut csbi = CONSOLE_SCREEN_BUFFER_INFO::default();
        unsafe { GetConsoleScreenBufferInfo(handle, &mut csbi) }?;
        return Ok(csbi);
    }

    fn get_window_handle_for_process(&self, process_id: u32) -> HWND {
        /// Data structure for window search callback
        struct WindowSearchData {
            /// The process ID we're searching for
            target_process_id: u32,
            /// Mutable reference to store the found window handle
            found_handle: *mut Option<HWND>,
        }

        /// Callback function for finding windows by process ID with proper handle capture
        unsafe extern "system" fn find_window_callback_with_capture(
            hwnd: HWND,
            lparam: LPARAM,
        ) -> BOOL {
            let search_data = &mut *(lparam.0 as *mut WindowSearchData);
            let mut window_process_id: u32 = 0;
            GetWindowThreadProcessId(hwnd, Some(&mut window_process_id));

            if search_data.target_process_id == window_process_id {
                // Store the found window handle
                *search_data.found_handle = Some(hwnd);
                return FALSE; // Stop enumeration
            }
            return TRUE; // Continue enumeration
        }

        let mut found_handle = None;
        let mut search_data = WindowSearchData {
            target_process_id: process_id,
            found_handle: &mut found_handle,
        };

        loop {
            let _ = unsafe {
                EnumWindows(
                    Some(find_window_callback_with_capture),
                    LPARAM(&mut search_data as *mut WindowSearchData as isize),
                )
            };
            if let Some(handle) = found_handle {
                return handle;
            }
        }
    }

    fn create_process_raw(
        &self,
        application: &str,
        command_line: windows::core::PWSTR,
        startup_info: &mut windows::Win32::System::Threading::STARTUPINFOW,
        process_info: &mut windows::Win32::System::Threading::PROCESS_INFORMATION,
    ) -> windows::core::Result<()> {
        return unsafe {
            CreateProcessW(
                &HSTRING::from(application),
                Some(command_line),
                Some(ptr::null_mut()),
                Some(ptr::null_mut()),
                false,
                CREATE_NEW_CONSOLE,
                Some(ptr::null_mut()),
                PCWSTR::null(),
                ptr::addr_of_mut!(*startup_info),
                ptr::addr_of_mut!(*process_info),
            )
        };
    }

    fn get_console_window(&self) -> HWND {
        return unsafe { GetConsoleWindow() };
    }

    fn get_foreground_window(&self) -> HWND {
        return unsafe { GetForegroundWindow() };
    }

    fn set_foreground_window(&self, hwnd: HWND) -> windows::core::Result<()> {
        let result = unsafe { SetForegroundWindow(hwnd) };
        if result.as_bool() {
            return Ok(());
        } else {
            return Err(windows::core::Error::from_win32());
        }
    }

    fn get_console_mode(&self, handle: HANDLE) -> windows::core::Result<CONSOLE_MODE> {
        let mut mode = CONSOLE_MODE(0u32);
        unsafe { GetConsoleMode(handle, &mut mode)? };
        return Ok(mode);
    }

    fn set_console_mode(&self, handle: HANDLE, mode: CONSOLE_MODE) -> windows::core::Result<()> {
        return unsafe { SetConsoleMode(handle, mode) };
    }

    fn get_exit_code(&self, handle: HANDLE) -> windows::core::Result<u32> {
        let mut exit_code: u32 = 0;
        unsafe { GetExitCodeProcess(handle, &mut exit_code)? };
        return Ok(exit_code);
    }

    fn move_window(
        &self,
        hwnd: HWND,
        x: i32,
        y: i32,
        width: i32,
        height: i32,
        repaint: bool,
    ) -> windows::core::Result<()> {
        return unsafe { MoveWindow(hwnd, x, y, width, height, repaint) };
    }

    fn get_window_placement(&self, hwnd: HWND) -> windows::core::Result<WINDOWPLACEMENT> {
        let mut placement: WINDOWPLACEMENT = WINDOWPLACEMENT {
            length: mem::size_of::<WINDOWPLACEMENT>() as u32,
            ..Default::default()
        };
        unsafe { GetWindowPlacement(hwnd, &mut placement)? };
        return Ok(placement);
    }

    fn show_window(&self, hwnd: HWND, cmd_show: SHOW_WINDOW_CMD) -> windows::core::Result<bool> {
        let result = unsafe { ShowWindow(hwnd, cmd_show) };
        return Ok(result.as_bool());
    }

    fn focus_window_with_automation(&self, hwnd: HWND) -> windows::core::Result<()> {
        let automation: IUIAutomation =
            unsafe { CoCreateInstance(&CUIAutomation, None, CLSCTX_ALL)? };
        let window = unsafe { automation.ElementFromHandle(hwnd)? };
        unsafe { window.SetFocus()? };
        return Ok(());
    }

    fn is_window(&self, hwnd: HWND) -> bool {
        return unsafe { windows::Win32::UI::WindowsAndMessaging::IsWindow(Some(hwnd)).as_bool() };
    }

    fn open_process(
        &self,
        access: u32,
        inherit: bool,
        process_id: u32,
    ) -> windows::core::Result<HANDLE> {
        return unsafe { OpenProcess(PROCESS_ACCESS_RIGHTS(access), inherit, process_id) };
    }

    fn initialize_com_library(
        &self,
        coinit: windows::Win32::System::Com::COINIT,
    ) -> windows::core::Result<()> {
        let result = unsafe { windows::Win32::System::Com::CoInitializeEx(None, coinit) };
        if result.is_ok() {
            return Ok(());
        } else {
            return Err(windows::core::Error::from(result));
        }
    }

    fn get_system_metrics(&self, index: SYSTEM_METRICS_INDEX) -> i32 {
        return unsafe { windows::Win32::UI::WindowsAndMessaging::GetSystemMetrics(index) };
    }

    fn set_process_dpi_awareness(
        &self,
        value: windows::Win32::UI::HiDpi::PROCESS_DPI_AWARENESS,
    ) -> windows::core::Result<()> {
        return unsafe { windows::Win32::UI::HiDpi::SetProcessDpiAwareness(value) };
    }
}

/// u16 representation of a [KEY_EVENT][1].
///
/// For some reason the public [KEY_EVENT][1] constant is a u32
/// while the [INPUT_RECORD][2].`EventType` is u16...
///
/// [1]: https://microsoft.github.io/windows-docs-rs/doc/windows/Win32/System/Console/constant.KEY_EVENT.html
/// [2]: https://microsoft.github.io/windows-docs-rs/doc/windows/Win32/System/Console/struct.INPUT_RECORD.html
pub const KEY_EVENT: u16 = KEY_EVENT_U32 as u16;

/// Build command line string for Windows process creation
///
/// # Arguments
///
/// * `application` - Application name including file extension
/// * `args` - List of arguments to the application
///
/// # Returns
///
/// UTF-16 encoded command line with proper quoting
///
/// # Examples
///
/// ```
/// use csshw_lib::utils::windows::build_command_line;
///
/// let cmd_line = build_command_line("cmd.exe", &["arg1".to_string(), "arg2".to_string()]);
/// // Returns UTF-16 encoded: "cmd.exe" "arg1" "arg2"\0
/// ```
pub fn build_command_line(application: &str, args: &[String]) -> Vec<u16> {
    let mut cmd: Vec<u16> = Vec::new();
    cmd.push(b'"' as u16);
    cmd.extend(OsString::from(application).encode_wide());
    cmd.push(b'"' as u16);

    for arg in args {
        cmd.push(' ' as u16);
        cmd.push(b'"' as u16);
        cmd.extend(OsString::from(arg).encode_wide());
        cmd.push(b'"' as u16);
    }
    cmd.push(0); // add null terminator

    return cmd;
}

/// Sets the back- and foreground color of the current console window using the provided API.
///
/// # Arguments
///
/// * `api` - The Windows API implementation to use.
/// * `color` - The color value describing the back- and foreground color.
///
/// # Examples
///
/// ```no_run
/// use csshw_lib::utils::windows::{set_console_color, DefaultWindowsApi};
/// use windows::Win32::System::Console::CONSOLE_CHARACTER_ATTRIBUTES;
///
/// let api = DefaultWindowsApi;
/// set_console_color(&api, CONSOLE_CHARACTER_ATTRIBUTES(0x0F));
/// ```
pub fn set_console_color(api: &dyn WindowsApi, color: CONSOLE_CHARACTER_ATTRIBUTES) {
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

/// Empties the console screen output buffer of the current console window using the provided API.
///
/// # Arguments
///
/// * `api` - The Windows API implementation to use.
///
/// # Examples
///
/// ```no_run
/// use csshw_lib::utils::windows::{clear_screen, DefaultWindowsApi};
///
/// let api = DefaultWindowsApi;
/// clear_screen(&api);
/// ```
pub fn clear_screen(api: &dyn WindowsApi) {
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

/// Sets the border color of the current console window using the provided APIs.
///
/// Windows10 does not support this.
///
/// # Arguments
///
/// * `api` - The Windows API implementation;
/// * `color` - RGB [COLORREF][1] to set as border color.
///
/// # Examples
///
/// ```no_run
/// use csshw_lib::utils::windows::{set_console_border_color, DefaultWindowsApi};
/// use windows::Win32::Foundation::COLORREF;
///
/// set_console_border_color(&DefaultWindowsApi, COLORREF(0x001A2B3C));
/// ```
///
/// [1]: https://learn.microsoft.com/en-us/windows/win32/gdi/colorref
pub fn set_console_border_color(api: &dyn WindowsApi, color: COLORREF) {
    if !is_windows_10(api) {
        api.set_console_border_color(&color).unwrap();
    }
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
///
/// # Examples
///
/// ```
/// use csshw_lib::utils::windows::utf16_buffer_to_string;
///
/// let utf16_data = vec![72, 101, 108, 108, 111, 0]; // "Hello" + null terminator
/// let result = utf16_buffer_to_string(&utf16_data);
/// assert_eq!(result, "Hello");
/// ```
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
///
/// # Examples
///
/// ```no_run
/// use csshw_lib::utils::windows::{get_console_title, DefaultWindowsApi};
///
/// let title = get_console_title(&DefaultWindowsApi);
/// println!("Console title: {}", title);
/// ```
pub fn get_console_title(api: &dyn WindowsApi) -> String {
    let mut title: [u16; MAX_WINDOW_TITLE_LENGTH] = [0; MAX_WINDOW_TITLE_LENGTH];
    api.get_console_title(&mut title);
    return utf16_buffer_to_string(&title);
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
///
/// # Returns
///
/// Handle to the standard input of the current process.
///
/// # Examples
///
/// ```no_run
/// use csshw_lib::utils::windows::get_console_input_buffer;
///
/// let input_handle = get_console_input_buffer();
/// ```
pub fn get_console_input_buffer() -> HANDLE {
    return get_std_handle(STD_INPUT_HANDLE);
}

/// Returns a [HANDLE] to the [STD_OUTPUT_HANDLE] of the current process.
///
/// # Returns
///
/// Handle to the standard output of the current process.
///
/// # Examples
///
/// ```no_run
/// use csshw_lib::utils::windows::get_console_output_buffer;
///
/// let output_handle = get_console_output_buffer();
/// ```
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
///
/// # Examples
///
/// ```no_run
/// use csshw_lib::utils::windows::{read_console_input, DefaultWindowsApi};
///
/// let api = DefaultWindowsApi;
/// let input_record = read_console_input(&api);
/// ```
pub fn read_console_input(api: &dyn WindowsApi) -> INPUT_RECORD {
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
///
/// # Examples
///
/// ```no_run
/// use csshw_lib::utils::windows::{read_keyboard_input, DefaultWindowsApi};
///
/// let api = DefaultWindowsApi;
/// let key_event = read_keyboard_input(&api);
/// ```
pub fn read_keyboard_input(api: &dyn WindowsApi) -> INPUT_RECORD_0 {
    loop {
        let input_record = read_console_input(api);
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
///
/// # Examples
///
/// ```no_run
/// use csshw_lib::utils::windows::{arrange_console, DefaultWindowsApi};
///
/// let api = DefaultWindowsApi;
/// arrange_console(&api, 100, 100, 800, 600);
/// ```
pub fn arrange_console(api: &dyn WindowsApi, x: i32, y: i32, width: i32, height: i32) {
    // FIXME: sometimes a daemon or client console isn't being arrange correctly
    // when this simply retrying doesn't solve the issue. Maybe it has something to do
    // with DPI awareness => https://docs.rs/embed-manifest/latest/embed_manifest/
    api.arrange_console(x, y, width, height).unwrap();
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
///
/// # Examples
///
/// ```no_run
/// use csshw_lib::utils::windows::{is_windows_10, DefaultWindowsApi};
///
/// if is_windows_10(&DefaultWindowsApi) {
///     println!("Running on Windows 10");
/// } else {
///     println!("Running on Windows 11 or newer");
/// }
/// ```
pub fn is_windows_10(api: &dyn WindowsApi) -> bool {
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
#[path = "../tests/utils/test_windows.rs"]
mod test_mod;
