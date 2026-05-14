use windows::Win32::{
    Foundation::BOOL,
    System::Console::{INPUT_RECORD_0, KEY_EVENT_RECORD, KEY_EVENT_RECORD_0},
};

use crate::protocol::{
    ClientState, DaemonToClientMessage, SERIALIZED_INPUT_RECORD_0_LENGTH, SERIALIZED_PID_LENGTH,
    TAG_HIGHLIGHT, TAG_INPUT_RECORD, TAG_KEEP_ALIVE, TAG_STATE_CHANGE,
};

/// Deserialize a [KEY_EVENT_RECORD_0] from a u8 slice using custom binary format.
///
/// Tries to read a u16 from the given slice in little-endian format.
///
/// Panics if reconstruction fails.
pub fn deserialize_key_event_record_0(slice: &[u8]) -> KEY_EVENT_RECORD_0 {
    return KEY_EVENT_RECORD_0 {
        UnicodeChar: u16::from_le_bytes([slice[0], slice[1]]),
    };
}

/// Deserialize a [KEY_EVENT_RECORD] from a u8 slice using custom binary format.
/// The slice is expected to be 13 bytes long.
///
/// Layout: [1 byte KeyDown][2 bytes RepeatCount][2 bytes VirtualKeyCode]
///         [2 bytes VirtualScanCode][2 bytes UnicodeChar][4 bytes ControlKeyState]
///
/// Panics if reconstruction fails.
pub fn deserialize_key_event_record(slice: &[u8]) -> KEY_EVENT_RECORD {
    return KEY_EVENT_RECORD {
        // KeyDown (1 byte)
        bKeyDown: BOOL::from(slice[0] != 0),
        // RepeatCount (2 bytes LE)
        wRepeatCount: u16::from_le_bytes([slice[1], slice[2]]),
        // VirtualKeyCode (2 bytes LE)
        wVirtualKeyCode: u16::from_le_bytes([slice[3], slice[4]]),
        // VirtualScanCode (2 bytes LE)
        wVirtualScanCode: u16::from_le_bytes([slice[5], slice[6]]),
        // UnicodeChar (2 bytes LE)
        uChar: KEY_EVENT_RECORD_0 {
            UnicodeChar: u16::from_le_bytes([slice[7], slice[8]]),
        },
        // ControlKeyState (4 bytes LE)
        dwControlKeyState: u32::from_le_bytes([slice[9], slice[10], slice[11], slice[12]]),
    };
}

/// Deserialize an [INPUT_RECORD_0].`KeyEvent` from a u8 slice using custom binary format.
///
/// Panics if reconstruction fails.
pub fn deserialize_input_record_0(slice: &[u8]) -> INPUT_RECORD_0 {
    let key_event = deserialize_key_event_record(slice);
    return INPUT_RECORD_0 {
        KeyEvent: key_event,
    };
}

/// Deserialize a process id from its little-endian byte representation used
/// by the named-pipe PID handshake.
pub fn deserialize_pid(bytes: &[u8; SERIALIZED_PID_LENGTH]) -> u32 {
    return u32::from_le_bytes(*bytes);
}

/// Deserialize a single byte into a [`ClientState`] variant.
///
/// # Arguments
///
/// * `byte` - The single payload byte of a [`crate::protocol::TAG_STATE_CHANGE`]
///            frame, equal to a [`ClientState`]'s `#[repr(u8)]` discriminant.
///
/// # Returns
///
/// The decoded [`ClientState`].
///
/// # Panics
///
/// Panics if `byte` does not match a known [`ClientState`] discriminant. An
/// unknown value indicates either a protocol-version mismatch between the
/// daemon and client or corruption on the pipe - both unrecoverable, matching
/// the codebase's "broken bookkeeping -> panic" convention.
pub fn deserialize_client_state(byte: u8) -> ClientState {
    match byte {
        x if x == ClientState::Active as u8 => return ClientState::Active,
        x if x == ClientState::Disabled as u8 => return ClientState::Disabled,
        other => panic!("Unknown ClientState byte: 0x{other:02X}"),
    }
}

/// Deserialize a single byte into a highlight flag.
///
/// # Arguments
///
/// * `byte` - The single payload byte of a [`crate::protocol::TAG_HIGHLIGHT`]
///            frame: `0` for not highlighted, `1` for highlighted.
///
/// # Returns
///
/// `true` if the byte signals a highlighted client, `false` otherwise.
///
/// # Panics
///
/// Panics if `byte` is anything other than `0` or `1`. An unknown value
/// indicates either a protocol-version mismatch between the daemon and
/// client or corruption on the pipe - both unrecoverable, matching the
/// codebase's "broken bookkeeping -> panic" convention.
pub fn deserialize_highlight(byte: u8) -> bool {
    match byte {
        0 => return false,
        1 => return true,
        other => panic!("Unknown highlight byte: 0x{other:02X}"),
    }
}

/// Parse as many complete [`DaemonToClientMessage`]s as possible from `buffer`.
///
/// The parser walks `buffer` from the start, decoding one tag-prefixed frame
/// at a time. Parsing stops when fewer bytes remain than are needed to
/// complete the next frame; the unconsumed tail is returned so the caller
/// can prepend it to the next read.
///
/// # Arguments
///
/// * `buffer` - Bytes received from the daemon's named pipe, possibly
///              including a partial trailing frame.
///
/// # Returns
///
/// A tuple of `(messages, remainder)` where `messages` are the fully decoded
/// frames in arrival order and `remainder` holds the unconsumed bytes (an
/// empty `Vec` if the buffer ended on a frame boundary).
///
/// # Panics
///
/// Panics if `buffer` contains a tag byte that is not part of the documented
/// daemon-to-client protocol (see [`crate::protocol`]). An unknown tag
/// indicates either a protocol-version mismatch between the daemon and
/// client or corruption on the pipe -- both unrecoverable, matching the
/// codebase's "broken bookkeeping -> panic" convention.
pub fn parse_daemon_to_client_messages(buffer: &[u8]) -> (Vec<DaemonToClientMessage>, Vec<u8>) {
    let mut messages: Vec<DaemonToClientMessage> = Vec::new();
    let mut pos = 0usize;
    while pos < buffer.len() {
        let tag = buffer[pos];
        match tag {
            TAG_INPUT_RECORD => {
                let payload_start = pos + 1;
                let payload_end = payload_start + SERIALIZED_INPUT_RECORD_0_LENGTH;
                if buffer.len() < payload_end {
                    // Trailing partial frame; stop and return it as remainder.
                    break;
                }
                let record = deserialize_input_record_0(&buffer[payload_start..payload_end]);
                messages.push(DaemonToClientMessage::InputRecord(record));
                pos = payload_end;
            }
            TAG_STATE_CHANGE => {
                let payload_index = pos + 1;
                if buffer.len() <= payload_index {
                    // Trailing partial frame; stop and return it as remainder.
                    break;
                }
                let state = deserialize_client_state(buffer[payload_index]);
                messages.push(DaemonToClientMessage::StateChange(state));
                pos = payload_index + 1;
            }
            TAG_HIGHLIGHT => {
                let payload_index = pos + 1;
                if buffer.len() <= payload_index {
                    // Trailing partial frame; stop and return it as remainder.
                    break;
                }
                let highlighted = deserialize_highlight(buffer[payload_index]);
                messages.push(DaemonToClientMessage::Highlight(highlighted));
                pos = payload_index + 1;
            }
            TAG_KEEP_ALIVE => {
                messages.push(DaemonToClientMessage::KeepAlive);
                pos += 1;
            }
            _ => {
                panic!("Unknown daemon-to-client message tag: 0x{tag:02X}");
            }
        }
    }
    return (messages, buffer[pos..].to_vec());
}
