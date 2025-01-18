use windows::Win32::System::Console::{INPUT_RECORD_0, KEY_EVENT_RECORD, KEY_EVENT_RECORD_0};

/// Deserialize a struct from a u8 slice.
pub trait Deserialize {
    /// Constructs and returns a struct from the the given u8 slice.
    ///
    /// Panics if reconstruction fails.
    fn deserialize(slice: &mut [u8]) -> Self;
}

impl Deserialize for KEY_EVENT_RECORD_0 {
    /// Constructs and returns a [KEY_EVENT_RECORD_0] struct from the given u8 slice.
    ///
    /// Tries to read a u16 from the given slice.
    ///
    /// Panics if reconstruction fails.
    fn deserialize(slice: &mut [u8]) -> KEY_EVENT_RECORD_0 {
        return KEY_EVENT_RECORD_0 {
            UnicodeChar: rmp::decode::read_u16(&mut &(slice[..])).unwrap(),
        };
    }
}

impl Deserialize for KEY_EVENT_RECORD {
    /// Constructs and returns a [KEY_EVENT_RECORD] struct from the given u8 slice.
    /// The slice is expected to be [`SERIALIZED_INPUT_RECORD_0_LENGTH`] long.
    ///
    /// Tries to read various datatypes in the following order:
    ///
    /// ```
    /// [bool KeyDown, u16 ReapetCount, u16 VirtualKeyCode, u16 VirtualScanCode, u16 UnicodeChar, u32 ControlKeyState]
    /// ```
    ///
    /// Panics if reconstruction fails.
    fn deserialize(slice: &mut [u8]) -> KEY_EVENT_RECORD {
        return KEY_EVENT_RECORD {
            bKeyDown: rmp::decode::read_bool(&mut &(slice[0..1])).unwrap().into(),
            wRepeatCount: rmp::decode::read_u16(&mut &(slice[1..4])).unwrap(),
            wVirtualKeyCode: rmp::decode::read_u16(&mut &(slice[4..7])).unwrap(),
            wVirtualScanCode: rmp::decode::read_u16(&mut &(slice[7..10])).unwrap(),
            uChar: KEY_EVENT_RECORD_0::deserialize(&mut slice[10..13]),
            dwControlKeyState: rmp::decode::read_u32(&mut &(slice[13..18])).unwrap(),
        };
    }
}

impl Deserialize for INPUT_RECORD_0 {
    /// Constructs and returns a [INPUT_RECORD_0].`KeyEvent` struct from the given u8 slice.
    ///
    /// Panics if reconstruction fails.
    fn deserialize(slice: &mut [u8]) -> INPUT_RECORD_0 {
        return INPUT_RECORD_0 {
            KeyEvent: KEY_EVENT_RECORD::deserialize(slice),
        };
    }
}
