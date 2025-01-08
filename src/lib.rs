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

// https://github.com/microsoft/terminal/blob/v1.22.3232.0/src/propslib/DelegationConfig.hpp#L105
const CLSID_CONHOST: &str = "{B23D10C0-E52E-411E-9D5B-C09FDF709C7D}";
const CLSID_DEFAULT: &str = "{00000000-0000-0000-0000-000000000000}";
// https://github.com/microsoft/terminal/blob/v1.22.3232.0/src/propslib/DelegationConfig.cpp#L29
const DEFAULT_TERMINAL_APP_REGISTRY_PATH: &str = r"Console\%%Startup";
const DELEGATION_CONSOLE: &str = "DelegationConsole";
const DELEGATION_TERMINAL: &str = "DelegationTerminal";

// Guard that configures conhost.exe as the default terminal application
// and reverts to the original configuration when being dropped
#[derive(Default)]
pub struct WindowsSettingsDefaultTerminalApplicationGuard {
    old_windows_terminal_console: Option<String>,
    old_windows_terminal_terminal: Option<String>,
}

// Retrieve the RegistryKey under which the default terminal application system settings
// is being stored
fn get_reg_key() -> Option<RegKey> {
    return match Hive::CurrentUser.open(
        DEFAULT_TERMINAL_APP_REGISTRY_PATH,
        Security::Read | Security::Write,
    ) {
        Ok(val) => Some(val),
        Err(_) => None,
    };
}

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

// Tries to retrieve the registry value for the given value_name from the given regkey
// returns a default value if no value is found for the given value_name
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

impl WindowsSettingsDefaultTerminalApplicationGuard {
    // Read the existing default terminal application setting from the registry
    // before overwriting it with the value for conhost.exe
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
    // Restore the original default terminal application setting to the registry
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
            command_line,
            Some(ptr::null_mut()),
            Some(ptr::null_mut()),
            BOOL::from(false),
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

fn enumerate_windows<F>(mut callback: F)
where
    F: FnMut(HWND) -> bool,
{
    let mut trait_obj: &mut dyn FnMut(HWND) -> bool = &mut callback;
    let closure_pointer_pointer: *mut c_void = unsafe { mem::transmute(&mut trait_obj) };

    let lparam = LPARAM(closure_pointer_pointer as isize);
    unsafe { EnumWindows(Some(enumerate_callback), lparam).unwrap() };
}

unsafe extern "system" fn enumerate_callback(hwnd: HWND, lparam: LPARAM) -> BOOL {
    let closure: &mut &mut dyn FnMut(HWND) -> bool = &mut *(lparam.0 as *mut c_void
        as *mut &mut dyn std::ops::FnMut(windows::Win32::Foundation::HWND) -> bool);
    if closure(hwnd) {
        return TRUE;
    } else {
        return FALSE;
    }
}

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
