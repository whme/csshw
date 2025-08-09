use windows::Win32::{
    Foundation::BOOL,
    System::Console::{INPUT_RECORD_0, KEY_EVENT_RECORD, KEY_EVENT_RECORD_0},
};

use crate::serde::SERIALIZED_INPUT_RECORD_0_LENGTH;

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

const EXPECTED_KEY_EVENT_RECORD_0_SEQUENCE: [u8; 2] = [
    // UnicodeChar as u16 LE (2 bytes)
    61, 0,
];
const EXPECTED_KEY_EVENT_RECORD_SEQUENCE: [u8; SERIALIZED_INPUT_RECORD_0_LENGTH] = [
    // bKeyDown (1 byte)
    1, // wRepeatCount (2 bytes LE)
    61, 0, // wVirtualKeyCode (2 bytes LE)
    61, 0, // wVirtualScanCode (2 bytes LE)
    61, 0, // uChar (2 bytes LE)
    61, 0, // dwControlKeyState (4 bytes LE)
    61, 0, 0, 0,
];
const EXPECTED_INPUT_RECORD_0_SEQUENCE: [u8; SERIALIZED_INPUT_RECORD_0_LENGTH] =
    EXPECTED_KEY_EVENT_RECORD_SEQUENCE;

mod serialization_test {
    use super::*;
    use crate::serde::serialization::*;

    #[test]
    fn test_serialize_key_event_record_0() {
        assert_eq!(
            serialize_key_event_record_0(&EXPECTED_KEY_EVENT_RECORD_0),
            EXPECTED_KEY_EVENT_RECORD_0_SEQUENCE.to_vec()
        )
    }

    #[test]
    fn test_serialize_key_event_record() {
        assert_eq!(
            serialize_key_event_record(&EXPECTED_KEY_EVENT_RECORD),
            EXPECTED_KEY_EVENT_RECORD_SEQUENCE.to_vec()
        )
    }

    #[test]
    fn test_serialize_input_record_0() {
        assert_eq!(
            serialize_input_record_0(&EXPECTED_INPUT_RECORD_0),
            EXPECTED_KEY_EVENT_RECORD_SEQUENCE.to_vec()
        )
    }
}

mod deserialization_test {
    use super::*;
    use crate::serde::deserialization::*;

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
            deserialize_key_event_record_0(&EXPECTED_KEY_EVENT_RECORD_0_SEQUENCE)
                .equals(EXPECTED_KEY_EVENT_RECORD_0)
        );
    }

    #[test]
    fn test_deserialize_key_event_record() {
        assert!(
            deserialize_key_event_record(&EXPECTED_KEY_EVENT_RECORD_SEQUENCE)
                .equals(EXPECTED_KEY_EVENT_RECORD)
        )
    }

    #[test]
    fn test_deserialize_input_record_0() {
        assert!(
            deserialize_input_record_0(&EXPECTED_INPUT_RECORD_0_SEQUENCE)
                .equals(EXPECTED_INPUT_RECORD_0),
        )
    }
}
