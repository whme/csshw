//! Wire protocol used between the daemon and clients.
//!
//! Defines the tagged-envelope message framing for the daemon→client
//! direction over the named pipe and the lightweight message enum used
//! by both ends.
//!
//! Each daemon→client message on the wire has the form
//! `[1 byte tag][payload of tag-specific length]`. The client→daemon
//! direction (4-byte PID handshake) is handled separately and does not
//! use this envelope.

#![deny(clippy::implicit_return)]
#![allow(clippy::needless_return, clippy::doc_overindented_list_items)]
#![warn(missing_docs)]

use windows::Win32::System::Console::INPUT_RECORD_0;

/// Tag byte identifying an input-record message on the daemon→client pipe.
///
/// The tag byte is followed by the
/// [`crate::serde::SERIALIZED_INPUT_RECORD_0_LENGTH`]-byte serialized payload
/// produced by [`crate::serde::serialization::serialize_input_record_0`].
pub const TAG_INPUT_RECORD: u8 = 0x00;

/// Tag byte identifying a client state-change message on the daemon→client
/// pipe.
///
/// The tag byte is followed by the single-byte payload produced by
/// [`crate::serde::serialization::serialize_client_state`].
pub const TAG_STATE_CHANGE: u8 = 0x01;

/// Tag byte identifying a zero-payload keep-alive message on the
/// daemon→client pipe.
///
/// Used by the daemon's pipe server to detect early when the client end of
/// the pipe is closed.
pub const TAG_KEEP_ALIVE: u8 = 0xFF;

/// Length on the wire of a framed input-record message: the tag byte plus
/// the existing [`crate::serde::SERIALIZED_INPUT_RECORD_0_LENGTH`]-byte
/// payload.
pub const FRAMED_INPUT_RECORD_LENGTH: usize = 1 + crate::serde::SERIALIZED_INPUT_RECORD_0_LENGTH;

/// Length on the wire of a framed keep-alive message: just the tag byte.
pub const FRAMED_KEEP_ALIVE_LENGTH: usize = 1;

/// Length on the wire of a framed state-change message: the tag byte plus
/// a single-byte [`ClientState`] payload.
pub const FRAMED_STATE_CHANGE_LENGTH: usize = 2;

/// Runtime state of a single client.
///
/// Authoritative state value tracked per client by the daemon and pushed to
/// the corresponding client over the named pipe. Both ends consult this
/// value: the daemon to gate input forwarding, the client to know its own
/// state inside its main loop.
///
/// `#[repr(u8)]` so the enum round-trips through
/// [`crate::serde::serialization::serialize_client_state`] /
/// [`crate::serde::deserialization::deserialize_client_state`] using a single
/// byte.
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClientState {
    /// Client receives and replays all input records broadcast by the
    /// daemon.
    Active = 0,
}

/// Daemon→client message variants exchanged over the named pipe.
///
/// Each variant maps to a distinct tag byte at the start of the wire
/// representation; see [`TAG_INPUT_RECORD`], [`TAG_STATE_CHANGE`] and
/// [`TAG_KEEP_ALIVE`].
#[derive(Clone, Copy)]
pub enum DaemonToClientMessage {
    /// Carries an [`INPUT_RECORD_0`] (`KeyEvent`) to be replayed to the
    /// client's console input buffer.
    InputRecord(INPUT_RECORD_0),
    /// Carries the new [`ClientState`] the daemon assigned to this client.
    StateChange(ClientState),
    /// Empty payload sent on idle by the daemon's pipe server to detect a
    /// closed client pipe.
    KeepAlive,
}
