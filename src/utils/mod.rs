use std::{ptr, thread, time};

use windows::Win32::Foundation::{GetLastError, HANDLE, RECT};
use windows::Win32::System::Console::{
    GetConsoleWindow, GetStdHandle, WriteConsoleInputW, INPUT_RECORD, INPUT_RECORD_0, KEY_EVENT,
    KEY_EVENT_RECORD, KEY_EVENT_RECORD_0, STD_HANDLE, STD_INPUT_HANDLE, STD_OUTPUT_HANDLE,
};
use windows::Win32::System::Threading::GetExitCodeProcess;
use windows::Win32::UI::WindowsAndMessaging::GetWindowRect;

pub mod constants;
pub mod debug;

use crate::serde::deserialization::Deserialize;
use crate::serde::serialization::Serialize;
use debug::StringRepr;

pub fn print_console_rect() {
    loop {
        let mut window_rect = RECT::default();
        unsafe { GetWindowRect(GetConsoleWindow(), ptr::addr_of_mut!(window_rect)) };
        println!("{:?}", window_rect);
        thread::sleep(time::Duration::from_millis(100));
    }
}

pub fn sleep(seconds: u64) {
    thread::sleep(time::Duration::from_secs(seconds));
}

pub fn wait_for_input() {
    println!("Waiting for input ...");
    loop {
        let mut line = String::new();
        std::io::stdin().read_line(&mut line).unwrap();
        println!("Read: {:?}", line);
        if line.as_str().to_lowercase() == "exit\r\n" {
            println!("Exiting in 5 sec ...");
            sleep(5);
            break;
        }
        if line.as_str().to_lowercase() == "wtb\r\n" {
            write_console_input_buffer();
        }
    }
}

// TODO: make this a function that takes an input array/vec of characters
// and sends them to the input buffer.
// Maybe split it into a function that translates everything
// into KeyEvents and puts them into a buffer and another one
// that writes that buffer to the console input
fn write_console_input_buffer() {
    let mut down_event = INPUT_RECORD_0::default();
    down_event.KeyEvent = KEY_EVENT_RECORD {
        bKeyDown: true.into(),
        wRepeatCount: 1,
        wVirtualKeyCode: 0x41,
        wVirtualScanCode: 0,
        uChar: KEY_EVENT_RECORD_0 { UnicodeChar: 0x41 },
        dwControlKeyState: 0,
    };
    let mut up_event = INPUT_RECORD_0::default();
    up_event.KeyEvent = KEY_EVENT_RECORD {
        bKeyDown: false.into(),
        wRepeatCount: 1,
        wVirtualKeyCode: 0x41,
        wVirtualScanCode: 0,
        uChar: KEY_EVENT_RECORD_0 { UnicodeChar: 0x41 },
        dwControlKeyState: 0,
    };
    // In theory the down_event is enough to write characters
    // but for completeness sake we should always send down and up
    let buffer: [INPUT_RECORD; 2] = [
        INPUT_RECORD {
            EventType: KEY_EVENT as u16,
            Event: down_event,
        },
        INPUT_RECORD {
            EventType: KEY_EVENT as u16,
            Event: up_event,
        },
    ];
    let mut buffer_len = buffer.len() as u32;
    unsafe {
        if WriteConsoleInputW(get_console_input_buffer(), &buffer, &mut buffer_len) == false {
            println!("Failed to write console input");
            println!("{:?}", GetLastError());
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

pub fn get_console_input_buffer() -> HANDLE {
    return get_std_handle(STD_INPUT_HANDLE);
}

fn get_console_screen_buffer() -> HANDLE {
    return get_std_handle(STD_OUTPUT_HANDLE);
}

pub fn get_process_exit_code(hprocess: HANDLE) -> u32 {
    let mut exit_code: u32 = 0;
    unsafe {
        GetExitCodeProcess(hprocess, &mut exit_code).expect("Failed to get process exit code");
    }
    return exit_code;
}

pub fn serde_input_record() {
    let mut rec = INPUT_RECORD_0::default();
    rec.KeyEvent = KEY_EVENT_RECORD {
        bKeyDown: true.into(),
        wRepeatCount: 1,
        wVirtualKeyCode: 0x41,
        wVirtualScanCode: 0,
        uChar: KEY_EVENT_RECORD_0 { UnicodeChar: 0x41 },
        dwControlKeyState: 0,
    };
    let mut serialized = rec.serialize();
    let deserialized = INPUT_RECORD_0::deserialize(&mut serialized.as_mut_vec()[..]);
    assert_eq!(
        rec.string_repr(),
        deserialized.string_repr(),
        "Serialization/Deserialization works!"
    );
}
