use rmp::encode::ByteBuf;
use windows::Win32::System::Console::{INPUT_RECORD_0, KEY_EVENT_RECORD, KEY_EVENT_RECORD_0};

/// Serialize a struct into a [ByteBuf].
pub trait Serialize {
    /// Returns a serialized self as [ByteBuf].
    fn serialize(&self) -> ByteBuf;
}

impl Serialize for KEY_EVENT_RECORD_0 {
    /// Returns the u16 `UnicodeChar` in a [ByteBuf].
    fn serialize(&self) -> ByteBuf {
        let mut buf = ByteBuf::new();
        rmp::encode::write_u16(&mut buf, unsafe { self.UnicodeChar }).unwrap();
        return buf;
    }
}

impl Serialize for KEY_EVENT_RECORD {
    /// Returns the [KEY_EVENT_RECORD] as [ByteBuf] in the following layout:
    /// ```
    /// [bool KeyDown, u16 ReapetCount, u16 VirtualKeyCode, u16 VirtualScanCode, u16 UnicodeChar, u32 ControlKeyState]
    /// ```
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
    /// Returns the [INPUT_RECORD_0].`KeyEvent` serialized as [ByteBuf].
    ///
    /// Panics if the [INPUT_RECORD_0] is not a `KeyEvent`.
    fn serialize(&self) -> ByteBuf {
        return unsafe { self.KeyEvent }.serialize();
    }
}
