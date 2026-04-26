//! Unit tests for the serde module.

use windows::Win32::{
    Foundation::BOOL,
    System::Console::{INPUT_RECORD_0, KEY_EVENT_RECORD, KEY_EVENT_RECORD_0},
};

use crate::serde::{SERIALIZED_INPUT_RECORD_0_LENGTH, SERIALIZED_PID_LENGTH};

const EXPECTED_PID: u32 = 0x04030201;
const EXPECTED_PID_SEQUENCE: [u8; SERIALIZED_PID_LENGTH] = [0x01, 0x02, 0x03, 0x04];

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

    #[test]
    fn test_serialize_pid() {
        assert_eq!(serialize_pid(EXPECTED_PID), EXPECTED_PID_SEQUENCE);
    }
}

mod deserialization_test {
    use super::*;
    use crate::serde::deserialization::*;

    pub(super) trait Equality<T = Self> {
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

    #[test]
    fn test_deserialize_pid() {
        assert_eq!(deserialize_pid(&EXPECTED_PID_SEQUENCE), EXPECTED_PID);
    }

    #[test]
    fn test_pid_round_trip() {
        use crate::serde::serialization::serialize_pid;
        let pid = 0xDEADBEEFu32;
        assert_eq!(deserialize_pid(&serialize_pid(pid)), pid);
    }
}

mod framed_message_test {
    use super::deserialization_test::Equality;
    use super::*;
    use crate::protocol::{
        DaemonToClientMessage, FRAMED_INPUT_RECORD_LENGTH, FRAMED_KEEP_ALIVE_LENGTH,
        TAG_INPUT_RECORD, TAG_KEEP_ALIVE,
    };
    use crate::serde::deserialization::parse_daemon_to_client_messages;
    use crate::serde::serialization::serialize_daemon_to_client_message;

    fn unwrap_input_record(msg: &DaemonToClientMessage) -> INPUT_RECORD_0 {
        match msg {
            DaemonToClientMessage::InputRecord(record) => return *record,
            DaemonToClientMessage::KeepAlive => panic!("expected InputRecord, got KeepAlive"),
        }
    }

    #[test]
    fn test_serialize_input_record_envelope() {
        let bytes = serialize_daemon_to_client_message(&DaemonToClientMessage::InputRecord(
            EXPECTED_INPUT_RECORD_0,
        ));
        assert_eq!(bytes.len(), FRAMED_INPUT_RECORD_LENGTH);
        assert_eq!(bytes[0], TAG_INPUT_RECORD);
        assert_eq!(&bytes[1..], &EXPECTED_INPUT_RECORD_0_SEQUENCE[..]);
    }

    #[test]
    fn test_serialize_keep_alive_envelope() {
        let bytes = serialize_daemon_to_client_message(&DaemonToClientMessage::KeepAlive);
        assert_eq!(bytes.len(), FRAMED_KEEP_ALIVE_LENGTH);
        assert_eq!(bytes[0], TAG_KEEP_ALIVE);
    }

    #[test]
    fn test_parse_input_record_round_trip() {
        let bytes = serialize_daemon_to_client_message(&DaemonToClientMessage::InputRecord(
            EXPECTED_INPUT_RECORD_0,
        ));
        let (messages, remainder) = parse_daemon_to_client_messages(&bytes);
        assert!(remainder.is_empty());
        assert_eq!(messages.len(), 1);
        assert!(unwrap_input_record(&messages[0]).equals(EXPECTED_INPUT_RECORD_0));
    }

    #[test]
    fn test_parse_keep_alive_round_trip() {
        let bytes = serialize_daemon_to_client_message(&DaemonToClientMessage::KeepAlive);
        let (messages, remainder) = parse_daemon_to_client_messages(&bytes);
        assert!(remainder.is_empty());
        assert_eq!(messages.len(), 1);
        assert!(matches!(messages[0], DaemonToClientMessage::KeepAlive));
    }

    #[test]
    fn test_parse_mixed_sequence() {
        let mut buf: Vec<u8> = Vec::new();
        buf.extend_from_slice(&serialize_daemon_to_client_message(
            &DaemonToClientMessage::KeepAlive,
        ));
        buf.extend_from_slice(&serialize_daemon_to_client_message(
            &DaemonToClientMessage::InputRecord(EXPECTED_INPUT_RECORD_0),
        ));
        buf.extend_from_slice(&serialize_daemon_to_client_message(
            &DaemonToClientMessage::KeepAlive,
        ));
        let (messages, remainder) = parse_daemon_to_client_messages(&buf);
        assert!(remainder.is_empty());
        assert_eq!(messages.len(), 3);
        assert!(matches!(messages[0], DaemonToClientMessage::KeepAlive));
        assert!(unwrap_input_record(&messages[1]).equals(EXPECTED_INPUT_RECORD_0));
        assert!(matches!(messages[2], DaemonToClientMessage::KeepAlive));
    }

    #[test]
    fn test_parse_partial_trailing_frame_returned_as_remainder() {
        let full = serialize_daemon_to_client_message(&DaemonToClientMessage::InputRecord(
            EXPECTED_INPUT_RECORD_0,
        ));
        // Truncate the trailing frame mid-payload to simulate a split read.
        let split_at = full.len() - 3;
        let (messages, remainder) = parse_daemon_to_client_messages(&full[..split_at]);
        assert!(messages.is_empty());
        assert_eq!(remainder.as_slice(), &full[..split_at]);

        // Concatenating the remainder with the rest yields the original bytes
        // and parses cleanly, mirroring the client's read loop.
        let mut next = remainder;
        next.extend_from_slice(&full[split_at..]);
        let (messages, remainder) = parse_daemon_to_client_messages(&next);
        assert!(remainder.is_empty());
        assert_eq!(messages.len(), 1);
        assert!(unwrap_input_record(&messages[0]).equals(EXPECTED_INPUT_RECORD_0));
    }

    #[test]
    #[should_panic(expected = "Unknown daemon-to-client message tag")]
    fn test_parse_unknown_tag_panics() {
        let bogus: [u8; 1] = [0x7E];
        let _ = parse_daemon_to_client_messages(&bogus);
    }
}
