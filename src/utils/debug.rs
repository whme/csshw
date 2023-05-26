use windows::Win32::System::Console::{INPUT_RECORD_0, KEY_EVENT_RECORD, KEY_EVENT_RECORD_0};

pub trait StringRepr {
    fn string_repr(&self) -> String;
}

impl StringRepr for KEY_EVENT_RECORD_0 {
    fn string_repr(&self) -> String {
        return format!("unicode_char: {}", unsafe { self.UnicodeChar });
    }
}

impl StringRepr for KEY_EVENT_RECORD {
    fn string_repr(&self) -> String {
        return vec![
            format!("key_down: {}", self.bKeyDown.as_bool()),
            format!("repeat_count: {}", self.wRepeatCount),
            format!("virtual_key_code: 0x{:x}", self.wVirtualKeyCode),
            format!("virtual_scan_code: 0x{:x}", self.wVirtualScanCode),
            format!("char: 0x{:x}", unsafe { self.uChar.UnicodeChar }),
            format!("control_key_state: {}", self.dwControlKeyState),
        ]
        .join(",\n");
    }
}

impl StringRepr for INPUT_RECORD_0 {
    fn string_repr(&self) -> String {
        return unsafe { self.KeyEvent }.string_repr();
    }
}
