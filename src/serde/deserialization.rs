use windows::Win32::{
    Foundation::BOOL,
    System::Console::{INPUT_RECORD_0, KEY_EVENT_RECORD, KEY_EVENT_RECORD_0},
};

/// Deserialize a [KEY_EVENT_RECORD_0] from a u8 slice using custom binary format.
///
/// Tries to read a u16 from the given slice in little-endian format.
///
/// Panics if reconstruction fails.
pub fn deserialize_key_event_record_0(slice: &[u8]) -> KEY_EVENT_RECORD_0 {
    return KEY_EVENT_RECORD_0 {
        UnicodeChar: u16::from_le_bytes([slice[0], slice[1]]),
    };
}

/// Deserialize a [KEY_EVENT_RECORD] from a u8 slice using custom binary format.
/// The slice is expected to be 13 bytes long.
///
/// Layout: [1 byte KeyDown][2 bytes RepeatCount][2 bytes VirtualKeyCode]
///         [2 bytes VirtualScanCode][2 bytes UnicodeChar][4 bytes ControlKeyState]
///
/// Panics if reconstruction fails.
pub fn deserialize_key_event_record(slice: &[u8]) -> KEY_EVENT_RECORD {
    return KEY_EVENT_RECORD {
        // KeyDown (1 byte)
        bKeyDown: BOOL::from(slice[0] != 0),
        // RepeatCount (2 bytes LE)
        wRepeatCount: u16::from_le_bytes([slice[1], slice[2]]),
        // VirtualKeyCode (2 bytes LE)
        wVirtualKeyCode: u16::from_le_bytes([slice[3], slice[4]]),
        // VirtualScanCode (2 bytes LE)
        wVirtualScanCode: u16::from_le_bytes([slice[5], slice[6]]),
        // UnicodeChar (2 bytes LE)
        uChar: KEY_EVENT_RECORD_0 {
            UnicodeChar: u16::from_le_bytes([slice[7], slice[8]]),
        },
        // ControlKeyState (4 bytes LE)
        dwControlKeyState: u32::from_le_bytes([slice[9], slice[10], slice[11], slice[12]]),
    };
}

/// Deserialize an [INPUT_RECORD_0].`KeyEvent` from a u8 slice using custom binary format.
///
/// Panics if reconstruction fails.
pub fn deserialize_input_record_0(slice: &[u8]) -> INPUT_RECORD_0 {
    let key_event = deserialize_key_event_record(slice);
    return INPUT_RECORD_0 {
        KeyEvent: key_event,
    };
}
