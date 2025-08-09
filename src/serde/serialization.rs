use windows::Win32::System::Console::{INPUT_RECORD_0, KEY_EVENT_RECORD, KEY_EVENT_RECORD_0};

use crate::serde::SERIALIZED_INPUT_RECORD_0_LENGTH;

/// Serialize a [KEY_EVENT_RECORD_0] into a `Vec<u8>` using custom binary format.
///
/// Returns the u16 `UnicodeChar` as `Vec<u8>`in little-endian format.
pub fn serialize_key_event_record_0(record: &KEY_EVENT_RECORD_0) -> Vec<u8> {
    return unsafe { record.UnicodeChar }.to_le_bytes().to_vec();
}

/// Serialize a [KEY_EVENT_RECORD] into a `Vec<u8>`using custom binary format.
///
/// Layout: [1 byte KeyDown][2 bytes RepeatCount][2 bytes VirtualKeyCode]
///         [2 bytes VirtualScanCode][2 bytes UnicodeChar][4 bytes ControlKeyState]
pub fn serialize_key_event_record(record: &KEY_EVENT_RECORD) -> Vec<u8> {
    let mut buf = Vec::with_capacity(SERIALIZED_INPUT_RECORD_0_LENGTH);

    // KeyDown as u8 (1 byte)
    buf.push(if record.bKeyDown.as_bool() { 1u8 } else { 0u8 });

    // RepeatCount as u16 LE (2 bytes)
    buf.extend_from_slice(&record.wRepeatCount.to_le_bytes());

    // VirtualKeyCode as u16 LE (2 bytes)
    buf.extend_from_slice(&record.wVirtualKeyCode.to_le_bytes());

    // VirtualScanCode as u16 LE (2 bytes)
    buf.extend_from_slice(&record.wVirtualScanCode.to_le_bytes());

    // UnicodeChar as u16 LE (2 bytes)
    buf.extend_from_slice(&unsafe { record.uChar.UnicodeChar }.to_le_bytes());

    // ControlKeyState as u32 LE (4 bytes)
    buf.extend_from_slice(&record.dwControlKeyState.to_le_bytes());

    return buf;
}

/// Serialize an [INPUT_RECORD_0].`KeyEvent` into a `Vec<u8>`using custom binary format.
///
/// Panics if the [INPUT_RECORD_0] is not a `KeyEvent`.
pub fn serialize_input_record_0(record: &INPUT_RECORD_0) -> Vec<u8> {
    return serialize_key_event_record(&unsafe { record.KeyEvent });
}
