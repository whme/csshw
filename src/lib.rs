#![deny(clippy::implicit_return)]
#![allow(clippy::needless_return)]
use std::ffi::OsString;
use std::fs::{create_dir, File};
use std::{mem, ptr};

use std::os::windows::ffi::OsStrExt;

use log::warn;
use registry::{value, Data, Hive, RegKey, Security};
use simplelog::{format_description, ConfigBuilder, LevelFilter, WriteLogger};
use windows::core::{HSTRING, PCWSTR, PWSTR};
use windows::Win32::Foundation::BOOL;
use windows::Win32::System::Threading::{
    CreateProcessW, CREATE_NEW_CONSOLE, PROCESS_INFORMATION, STARTUPINFOW,
};

pub mod client;
pub mod daemon;
pub mod serde;
pub mod utils;

// https://github.com/microsoft/terminal/blob/main/src/propslib/DelegationConfig.hpp#L105
const CLSID_CONHOST: &str = "{B23D10C0-E52E-411E-9D5B-C09FDF709C7D}";
// https://github.com/microsoft/terminal/blob/main/src/propslib/DelegationConfig.cpp#L29
const DEFAULT_TERMINAL_APP_REGISTRY_PATH: &str = r"Console\%%Startup";
const DELEGATION_CONSOLE: &str = "DelegationConsole";
const DELEGATION_TERMINAL: &str = "DelegationTerminal";

#[derive(Default)]
struct DefaultTerminalApplicationGuard {
    old_windows_terminal_console: Option<String>,
    old_windows_terminal_terminal: Option<String>,
}

fn get_reg_key() -> RegKey {
    return Hive::CurrentUser
        .open(
            DEFAULT_TERMINAL_APP_REGISTRY_PATH,
            Security::Read | Security::Write,
        )
        .unwrap_or_else(|_| {
            panic!("Failed to open registry path {DEFAULT_TERMINAL_APP_REGISTRY_PATH}")
        });
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

impl DefaultTerminalApplicationGuard {
    //
    pub fn new() -> Self {
        let regkey = get_reg_key();
        let old_windows_terminal_console = match regkey
            .value(DELEGATION_CONSOLE)
            .unwrap_or_else(|_| panic!("Failed to read value {DELEGATION_CONSOLE} from registry"))
        {
            Data::String(value) => value.to_string_lossy(),
            _ => {
                panic!("Expected string data");
            }
        };
        let old_windows_terminal_terminal = match regkey
            .value(DELEGATION_TERMINAL)
            .unwrap_or_else(|_| panic!("Failed to read value {DELEGATION_TERMINAL} from registry"))
        {
            Data::String(value) => value.to_string_lossy(),
            _ => {
                panic!("Expected string data");
            }
        };
        // No need to change the default terminal application if it is already set to conhost
        if old_windows_terminal_console == old_windows_terminal_terminal
            && old_windows_terminal_console == CLSID_CONHOST
        {
            return DefaultTerminalApplicationGuard::default();
        }

        write_registry_values(&regkey, CLSID_CONHOST.to_owned(), CLSID_CONHOST.to_owned());

        return DefaultTerminalApplicationGuard {
            old_windows_terminal_console: Some(old_windows_terminal_console),
            old_windows_terminal_terminal: Some(old_windows_terminal_terminal),
        };
    }
}

// FIXME: the drop happens to fast and we reset the config before the windows got launched.
impl Drop for DefaultTerminalApplicationGuard {
    fn drop(&mut self) {
        if let (Some(old_windows_terminal_console), Some(old_windows_terminal_terminal)) = (
            &self.old_windows_terminal_console,
            &self.old_windows_terminal_terminal,
        ) {
            write_registry_values(
                &get_reg_key(),
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
    {
        let _guard = DefaultTerminalApplicationGuard::new();
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
    }
    return process_information;
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
