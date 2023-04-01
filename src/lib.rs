use std::ffi::OsString;
use std::{env, mem, ptr, thread, time};

use std::os::windows::ffi::OsStrExt;

use windows::core::{HSTRING, PCWSTR, PWSTR};
use windows::Win32::Foundation::{BOOL, HANDLE, RECT};
use windows::Win32::System::Console::{
    GetConsoleWindow, GetStdHandle, STD_HANDLE, STD_INPUT_HANDLE, STD_OUTPUT_HANDLE,
};
use windows::Win32::System::Threading::{
    CreateProcessW, CREATE_NEW_CONSOLE, PROCESS_INFORMATION, STARTUPINFOW,
};
use windows::Win32::UI::WindowsAndMessaging::GetWindowRect;

pub const PKG_NAME: &str = env!("CARGO_PKG_NAME");

pub fn print_console_rect() {
    loop {
        let mut window_rect = RECT::default();
        unsafe { GetWindowRect(GetConsoleWindow(), ptr::addr_of_mut!(window_rect)) };
        println!("{:?}", window_rect);
        thread::sleep(time::Duration::from_millis(100));
    }
}

pub fn wait_for_input() {
    println!("Waiting for input ...");
    loop {
        let mut line = String::new();
        std::io::stdin().read_line(&mut line).unwrap();
        println!("Read: {:?}", line);
        if line.as_str().to_lowercase() == "exit\r\n" {
            println!("Exiting in 5 sec ...");
            thread::sleep(time::Duration::from_secs(5));
            break;
        }
    }
}

fn get_std_handle(nstdhandle: STD_HANDLE) -> HANDLE {
    return unsafe {
        GetStdHandle(nstdhandle)
            .expect(format!("Failed to retrieve standard handle: {:?}", nstdhandle).as_str())
    };
}

pub fn print_std_handles() {
    println!(
        "{:?} {:?}",
        get_console_input_buffer(),
        get_console_screen_buffer()
    );
}

fn get_console_input_buffer() -> HANDLE {
    return get_std_handle(STD_INPUT_HANDLE);
}

fn get_console_screen_buffer() -> HANDLE {
    return get_std_handle(STD_OUTPUT_HANDLE);
}

pub fn spawn_console_process(application: String, args: Vec<String>) -> PROCESS_INFORMATION {
    let mut application_full_path = env::current_dir()
        .expect("Failed to get current working directory")
        .as_os_str()
        .to_owned();
    application_full_path.push("\\");
    application_full_path.push(application);
    application_full_path.push(".exe");

    let mut cmd: Vec<u16> = Vec::new();
    cmd.push(b'"' as u16);
    cmd.extend(application_full_path.as_os_str().encode_wide());
    cmd.push(b'"' as u16);

    for arg in args {
        cmd.push(' ' as u16);
        cmd.extend(OsString::from(arg).encode_wide());
    }

    let mut startupinfo = STARTUPINFOW::default();
    startupinfo.cb = mem::size_of::<STARTUPINFOW>() as u32;
    let mut process_information = PROCESS_INFORMATION::default();
    let command_line = PWSTR(cmd.as_mut_ptr());
    unsafe {
        CreateProcessW(
            &HSTRING::from(application_full_path),
            command_line,
            Some(ptr::null_mut()),
            Some(ptr::null_mut()),
            BOOL(0),
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
