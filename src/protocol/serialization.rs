use windows::Win32::System::Console::{INPUT_RECORD_0, KEY_EVENT_RECORD, KEY_EVENT_RECORD_0};

use crate::protocol::{
    ClientState, DaemonToClientMessage, FRAMED_HIGHLIGHT_LENGTH, FRAMED_INPUT_RECORD_LENGTH,
    FRAMED_KEEP_ALIVE_LENGTH, FRAMED_STATE_CHANGE_LENGTH, SERIALIZED_INPUT_RECORD_0_LENGTH,
    SERIALIZED_PID_LENGTH, TAG_HIGHLIGHT, TAG_INPUT_RECORD, TAG_KEEP_ALIVE, TAG_STATE_CHANGE,
};

/// Serialize a [KEY_EVENT_RECORD_0] into a `Vec<u8>` using custom binary format.
///
/// Returns the u16 `UnicodeChar` as `Vec<u8>`in little-endian format.
pub fn serialize_key_event_record_0(record: &KEY_EVENT_RECORD_0) -> Vec<u8> {
    return unsafe { record.UnicodeChar }.to_le_bytes().to_vec();
}

/// Serialize a [KEY_EVENT_RECORD] into a `Vec<u8>`using custom binary format.
///
/// Layout: [1 byte KeyDown][2 bytes RepeatCount][2 bytes VirtualKeyCode]
///         [2 bytes VirtualScanCode][2 bytes UnicodeChar][4 bytes ControlKeyState]
pub fn serialize_key_event_record(record: &KEY_EVENT_RECORD) -> Vec<u8> {
    let mut buf = Vec::with_capacity(SERIALIZED_INPUT_RECORD_0_LENGTH);

    // KeyDown as u8 (1 byte)
    buf.push(if record.bKeyDown.as_bool() { 1u8 } else { 0u8 });

    // RepeatCount as u16 LE (2 bytes)
    buf.extend_from_slice(&record.wRepeatCount.to_le_bytes());

    // VirtualKeyCode as u16 LE (2 bytes)
    buf.extend_from_slice(&record.wVirtualKeyCode.to_le_bytes());

    // VirtualScanCode as u16 LE (2 bytes)
    buf.extend_from_slice(&record.wVirtualScanCode.to_le_bytes());

    // UnicodeChar as u16 LE (2 bytes)
    buf.extend_from_slice(&unsafe { record.uChar.UnicodeChar }.to_le_bytes());

    // ControlKeyState as u32 LE (4 bytes)
    buf.extend_from_slice(&record.dwControlKeyState.to_le_bytes());

    return buf;
}

/// Serialize an [INPUT_RECORD_0].`KeyEvent` into a `Vec<u8>`using custom binary format.
///
/// Panics if the [INPUT_RECORD_0] is not a `KeyEvent`.
pub fn serialize_input_record_0(record: &INPUT_RECORD_0) -> Vec<u8> {
    return serialize_key_event_record(&unsafe { record.KeyEvent });
}

/// Serialize a process id into its little-endian byte representation used by
/// the named-pipe PID handshake.
pub fn serialize_pid(pid: u32) -> [u8; SERIALIZED_PID_LENGTH] {
    return pid.to_le_bytes();
}

/// Serialize a [`ClientState`] into its single-byte wire representation.
///
/// # Arguments
///
/// * `state` - The client state to serialize.
///
/// # Returns
///
/// The state's `#[repr(u8)]` discriminant, used as the payload of a tagged
/// [`crate::protocol::TAG_STATE_CHANGE`] frame.
pub fn serialize_client_state(state: ClientState) -> u8 {
    return state as u8;
}

/// Serialize a highlight flag into its single-byte wire representation.
///
/// # Arguments
///
/// * `highlighted` - `true` if the client is the daemon's currently
///                   selected submenu client, `false` otherwise.
///
/// # Returns
///
/// `1` for `true`, `0` for `false`. Used as the payload of a tagged
/// [`crate::protocol::TAG_HIGHLIGHT`] frame.
pub fn serialize_highlight(highlighted: bool) -> u8 {
    return if highlighted { 1 } else { 0 };
}

/// Serialize a [`DaemonToClientMessage`] into its tagged-envelope wire
/// representation.
///
/// The first byte of the returned vector is the tag identifying the variant;
/// the remaining bytes (if any) are the variant's payload.
///
/// # Arguments
///
/// * `msg` - The message to serialize.
///
/// # Returns
///
/// A vector containing the framed wire bytes ready to be written to the
/// daemon's named pipe.
pub fn serialize_daemon_to_client_message(msg: &DaemonToClientMessage) -> Vec<u8> {
    match msg {
        DaemonToClientMessage::InputRecord(record) => {
            let mut buf = Vec::with_capacity(FRAMED_INPUT_RECORD_LENGTH);
            buf.push(TAG_INPUT_RECORD);
            buf.extend_from_slice(&serialize_input_record_0(record));
            return buf;
        }
        DaemonToClientMessage::StateChange(state) => {
            let mut buf = Vec::with_capacity(FRAMED_STATE_CHANGE_LENGTH);
            buf.push(TAG_STATE_CHANGE);
            buf.push(serialize_client_state(*state));
            return buf;
        }
        DaemonToClientMessage::Highlight(highlighted) => {
            let mut buf = Vec::with_capacity(FRAMED_HIGHLIGHT_LENGTH);
            buf.push(TAG_HIGHLIGHT);
            buf.push(serialize_highlight(*highlighted));
            return buf;
        }
        DaemonToClientMessage::KeepAlive => {
            let mut buf = Vec::with_capacity(FRAMED_KEEP_ALIVE_LENGTH);
            buf.push(TAG_KEEP_ALIVE);
            return buf;
        }
    }
}
