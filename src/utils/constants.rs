use windows::Win32::Foundation::BOOL;
use windows::Win32::System::Console::{INPUT_RECORD_0, KEY_EVENT_RECORD, KEY_EVENT_RECORD_0};

pub const PKG_NAME: &str = env!("CARGO_PKG_NAME");
// https://learn.microsoft.com/en-us/windows/win32/ipc/pipe-namess
pub const PIPE_NAME: &str = concat!(r"\\.\pipe\", env!("CARGO_PKG_NAME"), "-named-pipe-for-ipc");
// 0x97 - 0x9F are unassigned
// https://learn.microsoft.com/en-us/windows/win32/inputdev/virtual-key-codes
pub const VK_CTRL_C: u16 = 0x97;
pub const CTRL_C_INPUT_RECORD: INPUT_RECORD_0 = INPUT_RECORD_0 {
    KeyEvent: KEY_EVENT_RECORD {
        bKeyDown: BOOL(1),
        wRepeatCount: 1,
        wVirtualKeyCode: VK_CTRL_C,
        wVirtualScanCode: 0,
        uChar: KEY_EVENT_RECORD_0 {
            UnicodeChar: VK_CTRL_C,
        },
        dwControlKeyState: 0,
    },
};
