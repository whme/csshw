//! Serialization/Deserialization implemention for windows INPUT_RECORD_0.

#![deny(clippy::implicit_return)]
#![allow(clippy::needless_return, clippy::doc_overindented_list_items)]
#![warn(missing_docs)]

#[allow(missing_docs)]
pub mod deserialization;
#[allow(missing_docs)]
pub mod serialization;

/// Length of a serialized [INPUT_RECORD_0][1]
///
/// [1]: https://microsoft.github.io/windows-docs-rs/doc/windows/Win32/System/Console/union.INPUT_RECORD_0.html
pub const SERIALIZED_INPUT_RECORD_0_LENGTH: usize = 13;

/// Control sequence marker - first byte of all control sequences
pub const CONTROL_SEQUENCE_MARKER: u8 = 0xFE;

/// Control sequence: Set client state to ENABLED
pub const CONTROL_SEQ_STATE_ENABLED: [u8; SERIALIZED_INPUT_RECORD_0_LENGTH] = [
    CONTROL_SEQUENCE_MARKER,
    0x00,
    0,
    0,
    0,
    0,
    0,
    0,
    0,
    0,
    0,
    0,
    0,
];

/// Control sequence: Set client state to DISABLED
pub const CONTROL_SEQ_STATE_DISABLED: [u8; SERIALIZED_INPUT_RECORD_0_LENGTH] = [
    CONTROL_SEQUENCE_MARKER,
    0x01,
    0,
    0,
    0,
    0,
    0,
    0,
    0,
    0,
    0,
    0,
    0,
];

/// Control sequence: Set client state to SELECTED
pub const CONTROL_SEQ_STATE_SELECTED: [u8; SERIALIZED_INPUT_RECORD_0_LENGTH] = [
    CONTROL_SEQUENCE_MARKER,
    0x02,
    0,
    0,
    0,
    0,
    0,
    0,
    0,
    0,
    0,
    0,
    0,
];

/// Check if a byte sequence is a control sequence
pub fn is_control_sequence(packet: &[u8]) -> bool {
    return !packet.is_empty() && packet[0] == CONTROL_SEQUENCE_MARKER;
}

#[cfg(test)]
#[path = "../tests/serde/test_mod.rs"]
mod test_mod;
