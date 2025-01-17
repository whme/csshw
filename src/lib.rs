#![deny(clippy::implicit_return)]
#![allow(clippy::needless_return)]
use std::fs::{create_dir, File};
use std::{ffi::c_void, ffi::OsString};
use std::{mem, ptr};

use std::os::windows::ffi::OsStrExt;

use log::warn;
use registry::{value, Data, Hive, RegKey, Security};
use simplelog::{format_description, ConfigBuilder, LevelFilter, WriteLogger};
use windows::core::{HSTRING, PCWSTR, PWSTR};
use windows::Win32::Foundation::{BOOL, FALSE, HWND, LPARAM, TRUE};
use windows::Win32::System::Threading::{
    CreateProcessW, CREATE_NEW_CONSOLE, PROCESS_INFORMATION, STARTUPINFOW,
};
use windows::Win32::UI::WindowsAndMessaging::{EnumWindows, GetWindowThreadProcessId};

pub mod client;
pub mod daemon;
pub mod serde;
pub mod utils;

/// CLSID identifying `conhost.exe` in the registry.
///
/// As used in Windows Terminal:
/// https://github.com/microsoft/terminal/blob/v1.22.3232.0/src/propslib/DelegationConfig.hpp#L105
const CLSID_CONHOST: &str = "{B23D10C0-E52E-411E-9D5B-C09FDF709C7D}";
/// CLSID identifying the default configuration in the registry.
///
/// The default configuration is "let windows choose".
/// Also defined in Windows Terminal:
/// https://github.com/microsoft/terminal/blob/v1.22.3232.0/src/propslib/DelegationConfig.hpp#L104
const CLSID_DEFAULT: &str = "{00000000-0000-0000-0000-000000000000}";
/// Registry path where `DelegationConsole` and `DelegationTerminal` registry keys are stored.
///
/// These registry keys store the configuration value for the default terminal application.
const DEFAULT_TERMINAL_APP_REGISTRY_PATH: &str = r"Console\%%Startup";
/// `DelegationConsole` registry key.
///
/// As used in Windows Terminal:
/// https://github.com/microsoft/terminal/blob/v1.22.3232.0/src/propslib/DelegationConfig.cpp#L29
const DELEGATION_CONSOLE: &str = "DelegationConsole";
/// `DelegationTerminal` registry key.
///
/// As used in Windows Terminal:
/// https://github.com/microsoft/terminal/blob/v1.22.3232.0/src/propslib/DelegationConfig.cpp#L30
const DELEGATION_TERMINAL: &str = "DelegationTerminal";

/// Guard storing previous/old `DelegationConsole` and `DelegationTerminal` registry values.
///
/// Configures `conhost.exe` as the default terminal application
/// and reverts to the original configuration when being dropped.
#[derive(Default)]
pub struct WindowsSettingsDefaultTerminalApplicationGuard {
    /// Old `DelegationConsole` registry value
    old_windows_terminal_console: Option<String>,
    /// Old `DelegationTerminal` registry value
    old_windows_terminal_terminal: Option<String>,
}

impl WindowsSettingsDefaultTerminalApplicationGuard {
    /// Read the existing default terminal application setting from the registry
    /// before overwriting it with the value for `conhost.exe`.
    ///
    /// If `DelegationConsole` or `DelegationTerminal` registry values are missing
    /// they will be created with the default value.
    /// They are missing by default if the setting was never changed.
    pub fn new() -> Self {
        let regkey = match get_reg_key() {
            Some(val) => val,
            None => {
                warn!(
                    "Failed to read registry key {}, \
                    cannot make sure conhost.exe is the configured default terminal application",
                    DEFAULT_TERMINAL_APP_REGISTRY_PATH,
                );
                return WindowsSettingsDefaultTerminalApplicationGuard::default();
            }
        };
        let old_windows_terminal_console = match get_registry_value(&regkey, DELEGATION_CONSOLE) {
            Some(val) => val,
            _ => return WindowsSettingsDefaultTerminalApplicationGuard::default(),
        };
        let old_windows_terminal_terminal = match get_registry_value(&regkey, DELEGATION_TERMINAL) {
            Some(val) => val,
            _ => return WindowsSettingsDefaultTerminalApplicationGuard::default(),
        };

        // No need to change the default terminal application if it is already set to conhost.exe
        if old_windows_terminal_console == old_windows_terminal_terminal
            && old_windows_terminal_console == CLSID_CONHOST
        {
            return WindowsSettingsDefaultTerminalApplicationGuard::default();
        }

        write_registry_values(&regkey, CLSID_CONHOST.to_owned(), CLSID_CONHOST.to_owned());

        return WindowsSettingsDefaultTerminalApplicationGuard {
            old_windows_terminal_console: Some(old_windows_terminal_console),
            old_windows_terminal_terminal: Some(old_windows_terminal_terminal),
        };
    }
}

impl Drop for WindowsSettingsDefaultTerminalApplicationGuard {
    /// Restore the original default terminal application setting to the registry.
    ///
    /// If `self.old_windows_terminal_console` or `self.old_windows_terminal_terminal`
    /// attributes are [None] nothing is done.
    fn drop(&mut self) {
        if let (Some(old_windows_terminal_console), Some(old_windows_terminal_terminal)) = (
            &self.old_windows_terminal_console,
            &self.old_windows_terminal_terminal,
        ) {
            // We can safely unwrap the registry_key here, as we can only reach this code path if we
            // managed to read the registry initially
            write_registry_values(
                &get_reg_key().unwrap(),
                old_windows_terminal_console.to_owned(),
                old_windows_terminal_terminal.to_owned(),
            );
        }
    }
}

/// Retrieve the [RegKey] under which the default terminal application system settings
/// are stored.
///
/// # Returns
///
/// The registry key where the default terminal application system settings are stored.
fn get_reg_key() -> Option<RegKey> {
    return match Hive::CurrentUser.open(
        DEFAULT_TERMINAL_APP_REGISTRY_PATH,
        Security::Read | Security::Write,
    ) {
        Ok(val) => Some(val),
        Err(_) => None,
    };
}

/// Write `DelegationConsole` and `DelegationTerminal` registry values to the given [RegKey].
///
/// # Arguments
///
/// * `regkey`                      - The registry key where the default terminal application system settings are stored.
/// * `delegation_console_value`    - The CLSID for `DelegationConsole`.
/// * `delegation_terminal_value`   - The CLSID for `DelegationTerminal`.
fn write_registry_values(
    regkey: &RegKey,
    delegation_console_value: String,
    delegation_terminal_value: String,
) {
    let _: Result<(), value::Error> = regkey
        .set_value::<String>(
            DELEGATION_CONSOLE.to_owned(),
            &Data::String(delegation_console_value.clone().try_into().unwrap()),
        )
        .or_else(|_| {
            warn!(
                "Failed to change the default console application for registry key {} to {:?}",
                DELEGATION_CONSOLE, delegation_console_value
            );
            return Ok(());
        });
    let _: Result<(), value::Error> = regkey
        .set_value::<String>(
            DELEGATION_TERMINAL.to_owned(),
            &Data::String(delegation_terminal_value.clone().try_into().unwrap()),
        )
        .or_else(|_| {
            warn!(
                "Failed to change the default console application for registry key {} to {:?}",
                DELEGATION_TERMINAL, delegation_terminal_value
            );
            return Ok(());
        });
}

/// Try to retrieve the registry value for the given value_name from the given regkey.
///
/// # Arguments
///
/// * `regkey`      - [RegKey] from which to retrieve a value
/// * `value_name`  - The name of the registry value to retrieve
///
/// # Returns
///
/// The value from registry or `CLSID_DEFAULT`` if no value is found for the given value_name.
/// Returns [None] if we failed to retrieve the value.
fn get_registry_value(regkey: &RegKey, value_name: &str) -> Option<String> {
    return match regkey.value(value_name) {
        Ok(value) => match value {
            Data::String(value) => Some(value.to_string_lossy()),
            _ => {
                panic!("Expected string data for {} registry value", value_name)
            }
        },
        Err(value::Error::NotFound(_, _)) => Some(CLSID_DEFAULT.to_owned()),
        Err(err) => {
            warn!("Failed to read {} value from registry: {}", value_name, err);
            None
        }
    };
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
pub fn spawn_console_process(application: &str, args: Vec<&str>) -> PROCESS_INFORMATION {
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

    let mut startupinfo = STARTUPINFOW {
        cb: mem::size_of::<STARTUPINFOW>() as u32,
        ..Default::default()
    };
    // Sadly we can't use the startupinfo to position the console window right away
    // as x and y coordinates must be u32 and we might have negative values
    let mut process_information = PROCESS_INFORMATION::default();
    let command_line = PWSTR(cmd.as_mut_ptr());
    unsafe {
        CreateProcessW(
            &HSTRING::from(application),
            Some(command_line),
            Some(ptr::null_mut()),
            Some(ptr::null_mut()),
            false,
            CREATE_NEW_CONSOLE,
            Some(ptr::null_mut()),
            PCWSTR::null(),
            ptr::addr_of_mut!(startupinfo),
            ptr::addr_of_mut!(process_information),
        )
        .expect("Failed to create process");
    }
    return process_information;
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
pub fn get_concole_window_handle(process_id: u32) -> HWND {
    let mut client_window_handle: Option<HWND> = None;
    loop {
        enumerate_windows(|handle| {
            let mut window_process_id: u32 = 0;
            unsafe { GetWindowThreadProcessId(handle, Some(&mut window_process_id)) };
            if process_id == window_process_id {
                client_window_handle = Some(handle);
            }
            return true;
        });
        if client_window_handle.is_some() {
            break;
        }
    }
    return client_window_handle.unwrap();
}

/// Enumerate all top-level windows on the screen and call the given `callback` for each.
///
/// # Arguments
///
/// * `callback` - Function to be called for each top-level window with the windows [HWND].
///                Function must return [TRUE] to continue enumeration.
fn enumerate_windows<F>(mut callback: F)
where
    F: FnMut(HWND) -> bool,
{
    let mut trait_obj: &mut dyn FnMut(HWND) -> bool = &mut callback;
    let closure_pointer_pointer: *mut c_void = unsafe { mem::transmute(&mut trait_obj) };

    let lparam = LPARAM(closure_pointer_pointer as isize);
    unsafe { EnumWindows(Some(enumerate_callback), lparam).unwrap() };
}

/// Callback function used in `enumerate_windows` to pass a Rust closure to windows C code.
///
/// This function must comply with the
/// [EnumWindowsProc][https://learn.microsoft.com/en-us/previous-versions/windows/desktop/legacy/ms633498(v=vs.85)]
/// function signature.
///
/// # Arguments
///
/// * `hwnd`    - A [HWND] to a top-level window.
/// * `lparam`  - A pointer to the Rust closure that will be called with the [HWND].
unsafe extern "system" fn enumerate_callback(hwnd: HWND, lparam: LPARAM) -> BOOL {
    let closure: &mut &mut dyn FnMut(HWND) -> bool = &mut *(lparam.0 as *mut c_void
        as *mut &mut dyn std::ops::FnMut(windows::Win32::Foundation::HWND) -> bool);
    if closure(hwnd) {
        return TRUE;
    } else {
        return FALSE;
    }
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
    let utc_now = chrono::offset::Utc::now()
        .format("%Y-%m-%d_%H-%M-%S.%f")
        .to_string();
    let _ = create_dir("logs"); // directory already exists is fine too
    WriteLogger::init(
        LevelFilter::Debug,
        ConfigBuilder::new()
            .set_time_format_custom(format_description!("[hour]:[minute]:[second].[subsecond]"))
            .build(),
        File::create(format!("logs/{utc_now}_{name}.log")).unwrap(),
    )
    .unwrap();
    log_panics::init();
}
