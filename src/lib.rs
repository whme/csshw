#![deny(clippy::implicit_return)]
#![allow(clippy::needless_return)]
use std::ffi::OsString;
use std::fs::{create_dir, File};
use std::{mem, ptr};

use std::os::windows::ffi::OsStrExt;

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

pub fn init_logger(name: &str) {
    let utc_now = chrono::offset::Utc::now()
        .format("%Y-%m-%d_%H-%M-%S")
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
