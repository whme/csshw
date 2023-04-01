use rmp::encode::ByteBuf;
use windows::Win32::System::Console::{INPUT_RECORD_0, KEY_EVENT_RECORD, KEY_EVENT_RECORD_0};

pub trait Serialize {
    fn serialize(&self) -> ByteBuf;
}

impl Serialize for KEY_EVENT_RECORD_0 {
    fn serialize(&self) -> ByteBuf {
        let mut buf = ByteBuf::new();
        rmp::encode::write_u16(&mut buf, unsafe { self.UnicodeChar }).unwrap();
        return buf;
    }
}

impl Serialize for KEY_EVENT_RECORD {
    fn serialize(&self) -> ByteBuf {
        let mut buf = ByteBuf::new();
        rmp::encode::write_bool(&mut buf, self.bKeyDown.as_bool()).unwrap();
        rmp::encode::write_u16(&mut buf, self.wRepeatCount).unwrap();
        rmp::encode::write_u16(&mut buf, self.wVirtualKeyCode).unwrap();
        rmp::encode::write_u16(&mut buf, self.wVirtualScanCode).unwrap();
        buf.as_mut_vec().append(self.uChar.serialize().as_mut_vec());
        rmp::encode::write_u32(&mut buf, self.dwControlKeyState).unwrap();
        return buf;
    }
}

impl Serialize for INPUT_RECORD_0 {
    fn serialize(&self) -> ByteBuf {
        let mut buf = ByteBuf::new();
        buf.as_mut_vec()
            .append(unsafe { self.KeyEvent }.serialize().as_mut_vec());
        return buf;
    }
}
