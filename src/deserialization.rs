use rmp::encode::ByteBuf;
use windows::Win32::System::Console::{INPUT_RECORD_0, KEY_EVENT_RECORD, KEY_EVENT_RECORD_0};

pub trait Deserialize {
    fn deserialize(buf: &mut ByteBuf) -> Self;
}

impl Deserialize for KEY_EVENT_RECORD_0 {
    fn deserialize(buf: &mut ByteBuf) -> KEY_EVENT_RECORD_0 {
        return KEY_EVENT_RECORD_0 {
            UnicodeChar: rmp::decode::read_u16(&mut &(buf.as_mut_vec()[..])).unwrap(),
        };
    }
}

impl Deserialize for KEY_EVENT_RECORD {
    fn deserialize(buf: &mut ByteBuf) -> KEY_EVENT_RECORD {
        return KEY_EVENT_RECORD {
            bKeyDown: rmp::decode::read_bool(&mut &(buf.as_mut_vec()[..]))
                .unwrap()
                .into(),
            // FIXME: decoding wRepeatCount fails with TypeMismatch error
            wRepeatCount: rmp::decode::read_u16(&mut &(buf.as_mut_vec()[..])).unwrap(),
            wVirtualKeyCode: rmp::decode::read_u16(&mut &(buf.as_mut_vec()[..])).unwrap(),
            wVirtualScanCode: rmp::decode::read_u16(&mut &(buf.as_mut_vec()[..])).unwrap(),
            uChar: KEY_EVENT_RECORD_0::deserialize(buf),
            dwControlKeyState: rmp::decode::read_u32(&mut &(buf.as_mut_vec()[..])).unwrap(),
        };
    }
}

impl Deserialize for INPUT_RECORD_0 {
    fn deserialize(buf: &mut ByteBuf) -> INPUT_RECORD_0 {
        return INPUT_RECORD_0 {
            KeyEvent: KEY_EVENT_RECORD::deserialize(buf),
        };
    }
}
