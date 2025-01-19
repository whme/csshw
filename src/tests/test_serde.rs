use rmp::encode::ByteBuf;
use windows::Win32::{
    Foundation::BOOL,
    System::Console::{INPUT_RECORD_0, KEY_EVENT_RECORD, KEY_EVENT_RECORD_0},
};

const EXPECTED_KEY_EVENT_RECORD_0: KEY_EVENT_RECORD_0 = KEY_EVENT_RECORD_0 { UnicodeChar: 61u16 };
const EXPECTED_KEY_EVENT_RECORD: KEY_EVENT_RECORD = KEY_EVENT_RECORD {
    bKeyDown: BOOL(1),
    wRepeatCount: 61u16,
    wVirtualKeyCode: 61u16,
    wVirtualScanCode: 61u16,
    uChar: EXPECTED_KEY_EVENT_RECORD_0,
    dwControlKeyState: 61u32,
};
const EXPECTED_INPUT_RECORD_0: INPUT_RECORD_0 = INPUT_RECORD_0 {
    KeyEvent: EXPECTED_KEY_EVENT_RECORD,
};

const EXPECTED_KEY_EVENT_RECORD_0_SEQUENCE: [u8; 3] = [
    // `rmp::encode::write_u16` always writes a 3-byte sequence with the first byte being the marker.
    205, // The UnicodeChar we encode is a u16 so we have 2u8 here.
    0, 61,
];
const EXPECTED_KEY_EVENT_RECORD_SEQUENCE: [u8; 18] = [
    // bKeyDown
    195,
    // wRepeatCount
    205,
    0,
    61,
    // wVirtualKeyCode
    205,
    0,
    61,
    // wVirtualScanCode
    205,
    0,
    61,
    // uChar
    EXPECTED_KEY_EVENT_RECORD_0_SEQUENCE[0],
    EXPECTED_KEY_EVENT_RECORD_0_SEQUENCE[1],
    EXPECTED_KEY_EVENT_RECORD_0_SEQUENCE[2],
    // dwControlKeyState
    206,
    0,
    0,
    0,
    61,
];
const EXPECTED_INPUT_RECORD_0_SEQUENCE: [u8; 18] = EXPECTED_KEY_EVENT_RECORD_SEQUENCE;

mod serialization_test {
    use super::*;
    use crate::serde::serialization::Serialize as _;

    #[test]
    fn test_serialize_key_event_record_0() {
        assert_eq!(
            EXPECTED_KEY_EVENT_RECORD_0.serialize(),
            ByteBuf::from_vec(EXPECTED_KEY_EVENT_RECORD_0_SEQUENCE.to_vec())
        )
    }

    #[test]
    fn test_serialize_key_event_record() {
        assert_eq!(
            EXPECTED_KEY_EVENT_RECORD.serialize(),
            ByteBuf::from_vec(EXPECTED_KEY_EVENT_RECORD_SEQUENCE.to_vec())
        )
    }

    #[test]
    fn test_serialize_input_record_0() {
        assert_eq!(
            EXPECTED_INPUT_RECORD_0.serialize(),
            ByteBuf::from_vec(EXPECTED_KEY_EVENT_RECORD_SEQUENCE.to_vec())
        )
    }
}

mod deserialization_test {
    use super::*;
    use crate::serde::deserialization::Deserialize as _;

    trait Equality<T = Self> {
        fn equals(&self, other: T) -> bool;
    }

    impl Equality for KEY_EVENT_RECORD_0 {
        fn equals(&self, other: Self) -> bool {
            return unsafe { self.UnicodeChar } == unsafe { other.UnicodeChar };
        }
    }

    impl Equality for KEY_EVENT_RECORD {
        fn equals(&self, other: Self) -> bool {
            return self.bKeyDown == other.bKeyDown
                && self.wRepeatCount == other.wRepeatCount
                && self.wVirtualKeyCode == other.wVirtualKeyCode
                && self.wVirtualScanCode == other.wVirtualScanCode
                && self.uChar.equals(other.uChar)
                && self.dwControlKeyState == other.dwControlKeyState;
        }
    }

    impl Equality for INPUT_RECORD_0 {
        fn equals(&self, other: Self) -> bool {
            return unsafe { self.KeyEvent }.equals(unsafe { other.KeyEvent });
        }
    }

    #[test]
    fn test_deserialize_key_event_record_0() {
        assert!(
            KEY_EVENT_RECORD_0::deserialize(&mut EXPECTED_KEY_EVENT_RECORD_0_SEQUENCE.clone())
                .equals(EXPECTED_KEY_EVENT_RECORD_0)
        );
    }

    #[test]
    fn test_deserialize_key_event_record() {
        assert!(
            KEY_EVENT_RECORD::deserialize(&mut EXPECTED_KEY_EVENT_RECORD_SEQUENCE.clone())
                .equals(EXPECTED_KEY_EVENT_RECORD)
        )
    }

    #[test]
    fn test_deserialize_input_record_0() {
        assert!(
            INPUT_RECORD_0::deserialize(&mut EXPECTED_INPUT_RECORD_0_SEQUENCE.clone())
                .equals(EXPECTED_INPUT_RECORD_0),
        )
    }
}
