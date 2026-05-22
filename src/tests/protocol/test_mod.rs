//! Unit tests for the protocol module.

use windows::Win32::{
    Foundation::BOOL,
    System::Console::{INPUT_RECORD_0, KEY_EVENT_RECORD, KEY_EVENT_RECORD_0},
};

use crate::protocol::{SERIALIZED_INPUT_RECORD_0_LENGTH, SERIALIZED_PID_LENGTH};

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
    use crate::protocol::serialization::*;

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
    use crate::protocol::deserialization::*;

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
        use crate::protocol::serialization::serialize_pid;
        let pid = 0xDEADBEEFu32;
        assert_eq!(deserialize_pid(&serialize_pid(pid)), pid);
    }
}

mod client_state_test {
    use crate::protocol::deserialization::deserialize_client_state;
    use crate::protocol::serialization::serialize_client_state;
    use crate::protocol::ClientState;

    /// Round-trip list of every [`ClientState`] variant through the
    /// byte-level serializer / deserializer.
    const ALL_VARIANTS: &[ClientState] = &[ClientState::Active, ClientState::Disabled];

    /// Maps each [`ClientState`] variant to a unique single-bit mask.
    ///
    /// The exhaustive `match` (no wildcard) forces an update on any new
    /// variant, where the developer must allocate a fresh bit. The bit
    /// allocations are then OR-ed together to form the canonical
    /// [`EXPECTED_VARIANTS_MASK`].
    const fn variant_bit(state: ClientState) -> u32 {
        match state {
            ClientState::Active => return 1 << 0,
            ClientState::Disabled => return 1 << 1,
        }
    }

    /// Bitmask of every known [`ClientState`] variant. Adding a variant
    /// requires extending [`variant_bit`] AND OR-ing the new bit in here.
    const EXPECTED_VARIANTS_MASK: u32 =
        variant_bit(ClientState::Active) | variant_bit(ClientState::Disabled);

    /// Compile-time guard: [`ALL_VARIANTS`] must list every [`ClientState`]
    /// variant. Adding a variant fails to compile in [`variant_bit`] until
    /// a fresh bit is allocated and OR-ed into [`EXPECTED_VARIANTS_MASK`];
    /// once both are updated, the const evaluation below panics unless
    /// [`ALL_VARIANTS`] was also extended to include the new variant.
    const _: () = {
        let mut seen: u32 = 0;
        let mut i = 0;
        while i < ALL_VARIANTS.len() {
            seen |= variant_bit(ALL_VARIANTS[i]);
            i += 1;
        }
        assert!(
            seen == EXPECTED_VARIANTS_MASK,
            "ALL_VARIANTS does not cover every ClientState variant",
        );
    };

    #[test]
    fn test_client_state_round_trip_all_variants() {
        for &state in ALL_VARIANTS {
            let byte = serialize_client_state(state);
            assert_eq!(deserialize_client_state(byte), state);
        }
    }

    #[test]
    fn test_serialize_client_state_active_byte() {
        assert_eq!(serialize_client_state(ClientState::Active), 0u8);
    }

    #[test]
    fn test_serialize_client_state_disabled_byte() {
        assert_eq!(serialize_client_state(ClientState::Disabled), 1u8);
    }

    #[test]
    #[should_panic(expected = "Unknown ClientState byte")]
    fn test_deserialize_client_state_unknown_panics() {
        let _ = deserialize_client_state(0xAB);
    }
}

mod highlight_test {
    use crate::protocol::deserialization::deserialize_highlight;
    use crate::protocol::serialization::serialize_highlight;

    #[test]
    fn test_serialize_highlight_true_byte() {
        assert_eq!(serialize_highlight(true), 1u8);
    }

    #[test]
    fn test_serialize_highlight_false_byte() {
        assert_eq!(serialize_highlight(false), 0u8);
    }

    #[test]
    fn test_highlight_round_trip_true() {
        assert!(deserialize_highlight(serialize_highlight(true)));
    }

    #[test]
    fn test_highlight_round_trip_false() {
        assert!(!deserialize_highlight(serialize_highlight(false)));
    }

    #[test]
    #[should_panic(expected = "Unknown highlight byte")]
    fn test_deserialize_highlight_unknown_panics() {
        let _ = deserialize_highlight(0xAB);
    }
}

mod framed_message_test {
    use super::deserialization_test::Equality;
    use super::*;
    use crate::protocol::{
        deserialization::parse_daemon_to_client_messages,
        serialization::serialize_daemon_to_client_message, ClientState, DaemonToClientMessage,
        FRAMED_HIGHLIGHT_LENGTH, FRAMED_INPUT_RECORD_LENGTH, FRAMED_KEEP_ALIVE_LENGTH,
        FRAMED_STATE_CHANGE_LENGTH, TAG_HIGHLIGHT, TAG_INPUT_RECORD, TAG_KEEP_ALIVE,
        TAG_STATE_CHANGE,
    };

    fn unwrap_input_record(msg: &DaemonToClientMessage) -> INPUT_RECORD_0 {
        match msg {
            DaemonToClientMessage::InputRecord(record) => return *record,
            other => panic!(
                "expected InputRecord, got {:?}",
                std::mem::discriminant(other)
            ),
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
    fn test_serialize_state_change_envelope() {
        let bytes = serialize_daemon_to_client_message(&DaemonToClientMessage::StateChange(
            ClientState::Active,
        ));
        assert_eq!(bytes.len(), FRAMED_STATE_CHANGE_LENGTH);
        assert_eq!(bytes[0], TAG_STATE_CHANGE);
        assert_eq!(bytes[1], ClientState::Active as u8);
    }

    #[test]
    fn test_parse_state_change_round_trip() {
        let bytes = serialize_daemon_to_client_message(&DaemonToClientMessage::StateChange(
            ClientState::Active,
        ));
        let (messages, remainder) = parse_daemon_to_client_messages(&bytes);
        assert!(remainder.is_empty());
        assert_eq!(messages.len(), 1);
        match messages[0] {
            DaemonToClientMessage::StateChange(state) => assert_eq!(state, ClientState::Active),
            _ => panic!("expected StateChange variant"),
        }
    }

    #[test]
    fn test_serialize_highlight_envelope() {
        let bytes = serialize_daemon_to_client_message(&DaemonToClientMessage::Highlight(true));
        assert_eq!(bytes.len(), FRAMED_HIGHLIGHT_LENGTH);
        assert_eq!(bytes[0], TAG_HIGHLIGHT);
        assert_eq!(bytes[1], 1u8);
    }

    #[test]
    fn test_parse_highlight_round_trip() {
        for value in [true, false] {
            let bytes =
                serialize_daemon_to_client_message(&DaemonToClientMessage::Highlight(value));
            let (messages, remainder) = parse_daemon_to_client_messages(&bytes);
            assert!(remainder.is_empty());
            assert_eq!(messages.len(), 1);
            match messages[0] {
                DaemonToClientMessage::Highlight(decoded) => assert_eq!(decoded, value),
                _ => panic!("expected Highlight variant"),
            }
        }
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
