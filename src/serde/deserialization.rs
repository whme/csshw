use windows::Win32::System::Console::{INPUT_RECORD_0, KEY_EVENT_RECORD, KEY_EVENT_RECORD_0};

pub trait Deserialize {
    fn deserialize(slice: &mut [u8]) -> Self;
}

impl Deserialize for KEY_EVENT_RECORD_0 {
    fn deserialize(slice: &mut [u8]) -> KEY_EVENT_RECORD_0 {
        return KEY_EVENT_RECORD_0 {
            UnicodeChar: rmp::decode::read_u16(&mut &(slice[..])).unwrap(),
        };
    }
}

impl Deserialize for KEY_EVENT_RECORD {
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
    fn deserialize(slice: &mut [u8]) -> INPUT_RECORD_0 {
        return INPUT_RECORD_0 {
            KeyEvent: KEY_EVENT_RECORD::deserialize(slice),
        };
    }
}
