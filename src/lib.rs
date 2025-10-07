//! Cluster SSH tool for Windows inspired by csshX

#![deny(clippy::implicit_return)]
#![allow(clippy::needless_return, clippy::doc_overindented_list_items)]
#![warn(missing_docs)]
#![doc(html_no_source)]

use std::ffi::OsString;
use std::fs::{create_dir, File};
use std::{mem, ptr};

use std::os::windows::ffi::OsStrExt;

use log::warn;
use registry::{value, Data, Hive, Security};
use simplelog::{format_description, ConfigBuilder, LevelFilter, WriteLogger};
use windows::core::{HSTRING, PCWSTR, PWSTR};
use windows::Win32::Foundation::{BOOL, FALSE, HWND, LPARAM, TRUE};
use windows::Win32::System::Threading::{
    CreateProcessW, CREATE_NEW_CONSOLE, PROCESS_INFORMATION, STARTUPINFOW,
};
use windows::Win32::UI::WindowsAndMessaging::{EnumWindows, GetWindowThreadProcessId};

#[cfg(test)]
use mockall::automock;

pub mod cli;
pub mod client;
pub mod daemon;
pub mod serde;
pub mod utils;

/// CLSID identifying `conhost.exe` in the registry.
///
/// As used in Windows Terminal:
/// <https://github.com/microsoft/terminal/blob/v1.22.3232.0/src/propslib/DelegationConfig.hpp#L105>
const CLSID_CONHOST: &str = "{B23D10C0-E52E-411E-9D5B-C09FDF709C7D}";
/// CLSID identifying the default configuration in the registry.
///
/// The default configuration is "let windows choose".
/// Also defined in Windows Terminal:
/// <https://github.com/microsoft/terminal/blob/v1.22.3232.0/src/propslib/DelegationConfig.hpp#L104>
const CLSID_DEFAULT: &str = "{00000000-0000-0000-0000-000000000000}";
/// Registry path where `DelegationConsole` and `DelegationTerminal` registry keys are stored.
///
/// These registry keys store the configuration value for the default terminal application.
const DEFAULT_TERMINAL_APP_REGISTRY_PATH: &str = r"Console\%%Startup";
/// `DelegationConsole` registry key.
///
/// As used in Windows Terminal:
/// <https://github.com/microsoft/terminal/blob/v1.22.3232.0/src/propslib/DelegationConfig.cpp#L29>
const DELEGATION_CONSOLE: &str = "DelegationConsole";
/// `DelegationTerminal` registry key.
///
/// As used in Windows Terminal:
/// <https://github.com/microsoft/terminal/blob/v1.22.3232.0/src/propslib/DelegationConfig.cpp#L30>
const DELEGATION_TERMINAL: &str = "DelegationTerminal";

/// Trait for registry operations to enable mocking in tests
#[cfg_attr(test, automock)]
pub trait Registry {
    /// Get a string value from the registry
    fn get_registry_string_value(&self, path: &str, name: &str) -> Option<String>;
    /// Set a string value in the registry
    fn set_registry_string_value(&self, path: &str, name: &str, value: &str) -> bool;
}

/// Default implementation of Registry trait that performs actual Windows registry API calls
pub struct DefaultRegistry;

impl Registry for DefaultRegistry {
    fn get_registry_string_value(&self, path: &str, name: &str) -> Option<String> {
        let key = Hive::CurrentUser
            .open(path, Security::Read | Security::Write)
            .ok()?;
        match key.value(name) {
            Ok(Data::String(value)) => return Some(value.to_string_lossy()),
            Ok(_) => panic!("Expected string data for {name} registry value"),
            Err(value::Error::NotFound(_, _)) => return Some(CLSID_DEFAULT.to_owned()),
            Err(err) => {
                warn!("Failed to read {} value from registry: {}", name, err);
                return None;
            }
        }
    }

    fn set_registry_string_value(&self, path: &str, name: &str, value: &str) -> bool {
        if let Ok(key) = Hive::CurrentUser.open(path, Security::Read | Security::Write) {
            match key.set_value::<String>(
                name.to_owned(),
                &Data::String(value.to_owned().try_into().unwrap()),
            ) {
                Ok(()) => return true,
                Err(_) => {
                    warn!("Failed to set registry value {} to {}", name, value);
                    return false;
                }
            }
        } else {
            return false;
        }
    }
}

/// Trait for Windows API operations to enable mocking in tests
#[cfg_attr(test, automock)]
pub trait WindowsApi {
    /// Create a new process
    fn create_process_with_args(
        &self,
        application: &str,
        args: Vec<String>,
    ) -> Option<PROCESS_INFORMATION>;
    /// Get window handle for process ID
    fn get_window_handle_for_process(&self, process_id: u32) -> HWND;
    /// Low-level process creation API call
    fn create_process_raw(
        &self,
        application: &str,
        command_line: PWSTR,
        startup_info: &mut STARTUPINFOW,
        process_info: &mut PROCESS_INFORMATION,
    ) -> windows::core::Result<()>;
}

/// Data structure for window search callback
struct WindowSearchData {
    /// The process ID we're searching for
    target_process_id: u32,
    /// Mutable reference to store the found window handle
    found_handle: *mut Option<HWND>,
}

/// Default implementation of WindowsApi trait that performs actual Windows API calls
pub struct DefaultWindowsApi;

impl WindowsApi for DefaultWindowsApi {
    fn create_process_with_args(
        &self,
        application: &str,
        args: Vec<String>,
    ) -> Option<PROCESS_INFORMATION> {
        let command_line = build_command_line(application, &args);
        return create_process_windows_api(application, &command_line);
    }

    fn create_process_raw(
        &self,
        application: &str,
        command_line: PWSTR,
        startup_info: &mut STARTUPINFOW,
        process_info: &mut PROCESS_INFORMATION,
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

    fn get_window_handle_for_process(&self, process_id: u32) -> HWND {
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
}

/// Callback function for finding windows by process ID with proper handle capture
unsafe extern "system" fn find_window_callback_with_capture(hwnd: HWND, lparam: LPARAM) -> BOOL {
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

/// Return the Window Handle [HWND] for the foreground window associated with the given `process_id`.
///
/// If multiple foreground windows are associated with the given `process_id` it is undefined which [HWND] gets returned.
///
/// # Arguments
///
/// * `process_id` - ID of the process for which to retrieve the window handle.
///
/// # Returns
///
/// The Window Handle [HWND] for the window associated with the given `process_id`.
pub fn get_console_window_handle(process_id: u32) -> HWND {
    return DefaultWindowsApi.get_window_handle_for_process(process_id);
}

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

/// Create process with command line using the provided API (testable version)
///
/// # Arguments
///
/// * `api` - Windows API operations implementation
/// * `application` - Application name including file extension
/// * `command_line` - UTF-16 encoded command line
///
/// # Returns
///
/// [PROCESS_INFORMATION] of the spawned process or None if failed
pub fn create_process_with_command_line_api<W: WindowsApi>(
    api: &W,
    application: &str,
    command_line: &[u16],
) -> Option<PROCESS_INFORMATION> {
    let mut startupinfo = STARTUPINFOW {
        cb: mem::size_of::<STARTUPINFOW>() as u32,
        ..Default::default()
    };
    let mut process_information = PROCESS_INFORMATION::default();
    let mut cmd_line = command_line.to_vec();
    let command_line_ptr = PWSTR(cmd_line.as_mut_ptr());

    match api.create_process_raw(
        application,
        command_line_ptr,
        &mut startupinfo,
        &mut process_information,
    ) {
        Ok(()) => return Some(process_information),
        Err(_) => return None,
    }
}

/// Create process using Windows API (legacy function for backward compatibility)
///
/// # Arguments
///
/// * `application` - Application name including file extension
/// * `command_line` - UTF-16 encoded command line
///
/// # Returns
///
/// [PROCESS_INFORMATION] of the spawned process or None if failed
pub fn create_process_windows_api(
    application: &str,
    command_line: &[u16],
) -> Option<PROCESS_INFORMATION> {
    return create_process_with_command_line_api(&DefaultWindowsApi, application, command_line);
}

/// Trait for file system operations to enable mocking in tests
#[cfg_attr(test, automock)]
pub trait FileSystem {
    /// Create a directory
    fn create_directory(&self, path: &str) -> bool;
    /// Create a log file
    fn create_log_file(&self, filename: &str) -> bool;
}

/// Default implementation of FileSystem trait that performs actual file system operations
pub struct ProductionFileSystem;

impl FileSystem for ProductionFileSystem {
    fn create_directory(&self, path: &str) -> bool {
        return create_dir(path).is_ok() || std::path::Path::new(path).exists();
    }

    fn create_log_file(&self, filename: &str) -> bool {
        return File::create(filename).is_ok();
    }
}

/// Guard storing previous/old `DelegationConsole` and `DelegationTerminal` registry values.
///
/// Configures `conhost.exe` as the default terminal application
/// and reverts to the original configuration when being dropped.
pub struct WindowsSettingsDefaultTerminalApplicationGuard<R: Registry> {
    /// Old `DelegationConsole` registry value
    old_windows_terminal_console: Option<String>,
    /// Old `DelegationTerminal` registry value
    old_windows_terminal_terminal: Option<String>,
    /// Registry operations trait
    registry: R,
}

impl<R: Registry> WindowsSettingsDefaultTerminalApplicationGuard<R> {
    /// Create a new guard with the given registry operations
    ///
    /// # Arguments
    ///
    /// * `registry` - Registry operations implementation
    ///
    /// # Returns
    ///
    /// A new guard that will restore registry values on drop
    pub fn new_with_registry(registry: R) -> Self {
        let mut guard = WindowsSettingsDefaultTerminalApplicationGuard {
            old_windows_terminal_console: None,
            old_windows_terminal_terminal: None,
            registry,
        };

        if let (Some(console_val), Some(terminal_val)) = (
            guard
                .registry
                .get_registry_string_value(DEFAULT_TERMINAL_APP_REGISTRY_PATH, DELEGATION_CONSOLE),
            guard
                .registry
                .get_registry_string_value(DEFAULT_TERMINAL_APP_REGISTRY_PATH, DELEGATION_TERMINAL),
        ) {
            // No need to change if already set to conhost
            if console_val == CLSID_CONHOST && terminal_val == CLSID_CONHOST {
                return guard;
            }

            // Store old values and set new ones
            guard.old_windows_terminal_console = Some(console_val);
            guard.old_windows_terminal_terminal = Some(terminal_val);

            guard.registry.set_registry_string_value(
                DEFAULT_TERMINAL_APP_REGISTRY_PATH,
                DELEGATION_CONSOLE,
                CLSID_CONHOST,
            );
            guard.registry.set_registry_string_value(
                DEFAULT_TERMINAL_APP_REGISTRY_PATH,
                DELEGATION_TERMINAL,
                CLSID_CONHOST,
            );
        } else {
            warn!(
                "Failed to read registry key {}, \
                cannot make sure conhost.exe is the configured default terminal application",
                DEFAULT_TERMINAL_APP_REGISTRY_PATH,
            );
        }

        return guard;
    }
}

impl WindowsSettingsDefaultTerminalApplicationGuard<DefaultRegistry> {
    /// Create a new guard with production registry operations
    pub fn new() -> Self {
        return Self::new_with_registry(DefaultRegistry);
    }
}

impl<R: Registry> Default for WindowsSettingsDefaultTerminalApplicationGuard<R>
where
    R: Default,
{
    fn default() -> Self {
        return Self::new_with_registry(R::default());
    }
}

impl Default for DefaultRegistry {
    fn default() -> Self {
        return DefaultRegistry;
    }
}

impl<R: Registry> Drop for WindowsSettingsDefaultTerminalApplicationGuard<R> {
    /// Restore the original default terminal application setting to the registry.
    ///
    /// If old values weren't stored, nothing is done.
    fn drop(&mut self) {
        if let (Some(old_console), Some(old_terminal)) = (
            &self.old_windows_terminal_console,
            &self.old_windows_terminal_terminal,
        ) {
            self.registry.set_registry_string_value(
                DEFAULT_TERMINAL_APP_REGISTRY_PATH,
                DELEGATION_CONSOLE,
                old_console,
            );
            self.registry.set_registry_string_value(
                DEFAULT_TERMINAL_APP_REGISTRY_PATH,
                DELEGATION_TERMINAL,
                old_terminal,
            );
        }
    }
}

/// Launch the given console application with the given arguments as a new detached process with its own console window.
///
/// Input/Output handles are not being inherited.
/// Whichever default terminal application is configured in the windows system settings will be used
/// to host the application (i.e. create the window).
///
/// # Arguments
///
/// * `application` - Application name including file extension (`.exe`).
///                   If the application is not in the `PATH` environment variable, the full path
///                   must be specified.
/// * `args`        - List of arguments to the application.
///
/// # Returns
///
/// [PROCESS_INFORMATION] of the spawned process.
pub fn spawn_console_process(application: &str, args: Vec<String>) -> PROCESS_INFORMATION {
    return spawn_console_process_with_api(&DefaultWindowsApi, application, args)
        .expect("Failed to create process");
}

/// Launch the given console application with the given arguments using the provided API.
///
/// # Arguments
///
/// * `api` - Windows API operations implementation
/// * `application` - Application name including file extension
/// * `args` - List of arguments to the application
///
/// # Returns
///
/// [PROCESS_INFORMATION] of the spawned process or None if failed
pub fn spawn_console_process_with_api<W: WindowsApi>(
    api: &W,
    application: &str,
    args: Vec<String>,
) -> Option<PROCESS_INFORMATION> {
    return api.create_process_with_args(application, args);
}

/// Initialize the logger.
///
/// Makes sure a `logs` directory exists in the current working directory.
/// Log filename format: `<utc-time-of-executable-start>_<name>.log`.
/// Configures [log_panics].
///
/// # Arguments
///
/// * `name` - Will be part of the log filename.
pub fn init_logger(name: &str) {
    init_logger_with_fs(&ProductionFileSystem, name);
}

/// Initialize the logger with the provided file system operations.
///
/// # Arguments
///
/// * `fs` - File system operations implementation
/// * `name` - Will be part of the log filename
pub fn init_logger_with_fs<F: FileSystem>(fs: &F, name: &str) {
    let utc_now = chrono::offset::Utc::now()
        .format("%Y-%m-%d_%H-%M-%S.%f")
        .to_string();

    fs.create_directory("logs");

    let filename = format!("logs/{utc_now}_{name}.log");
    if fs.create_log_file(&filename) {
        if let Ok(file) = File::create(&filename) {
            let _ = WriteLogger::init(
                LevelFilter::Debug,
                ConfigBuilder::new()
                    .set_time_format_custom(format_description!(
                        "[hour]:[minute]:[second].[subsecond]"
                    ))
                    .build(),
                file,
            );
            log_panics::init();
        }
    }
}

#[cfg(test)]
#[path = "./tests/test_lib.rs"]
mod test_lib;
