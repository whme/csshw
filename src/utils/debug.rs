//! Implements string representations for INPUT_RECORD related structs.
//! For debugging only.

use windows::Win32::System::Console::{INPUT_RECORD_0, KEY_EVENT_RECORD, KEY_EVENT_RECORD_0};

/// String represation trait.
///
/// As we cannot implement foreign traits for foreign structs
/// we can't implement the `Display` or `Debug`` traits for the windows
/// structs.
pub trait StringRepr {
    /// Returns a string representation of the struct.
    fn string_repr(&self) -> String;
}

impl StringRepr for KEY_EVENT_RECORD_0 {
    /// Returns a string representation of a [KEY_EVENT_RECORD_0][1] showing
    /// which unicode character it represents.
    ///
    /// [1]: https://microsoft.github.io/windows-docs-rs/doc/windows/Win32/System/Console/union.KEY_EVENT_RECORD_0.html
    fn string_repr(&self) -> String {
        return format!("unicode_char: {}", unsafe { self.UnicodeChar });
    }
}

impl StringRepr for KEY_EVENT_RECORD {
    /// Returns a string representation of a [KEY_EVENT_RECORD][1] showing
    /// all relevant attributes.
    ///
    /// [1]: https://microsoft.github.io/windows-docs-rs/doc/windows/Win32/System/Console/struct.KEY_EVENT_RECORD.html
    fn string_repr(&self) -> String {
        return [
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
    /// Returns a string representation of a [INPUT_RECORD_0][1].
    ///
    /// Note: we expect a [KeyEvent][2].
    ///
    /// [1]: https://microsoft.github.io/windows-docs-rs/doc/windows/Win32/System/Console/union.INPUT_RECORD_0.html
    /// [2]: https://microsoft.github.io/windows-docs-rs/doc/windows/Win32/System/Console/struct.KEY_EVENT_RECORD.html
    fn string_repr(&self) -> String {
        return unsafe { self.KeyEvent }.string_repr();
    }
}

#[cfg(test)]
#[path = "../tests/utils/test_debug.rs"]
mod test_utils_debug;
