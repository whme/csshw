//! Wire protocol used between the daemon and clients.
//!
//! Defines the tagged-envelope message framing for the daemon-to-client
//! direction over the named pipe, the lightweight message enum used by
//! both ends, and the byte-level (de)serialization of the payloads.
//!
//! Each daemon-to-client message on the wire has the form
//! `[1 byte tag][payload of tag-specific length]`. The client-to-daemon
//! direction (4-byte PID handshake) is handled separately and does not
//! use this envelope.

#![deny(clippy::implicit_return)]
#![allow(clippy::needless_return, clippy::doc_overindented_list_items)]
#![warn(missing_docs)]

use windows::Win32::System::Console::INPUT_RECORD_0;

#[allow(missing_docs)]
pub mod deserialization;
#[allow(missing_docs)]
pub mod serialization;

/// Length of a serialized [INPUT_RECORD_0][1]
///
/// [1]: https://microsoft.github.io/windows-docs-rs/doc/windows/Win32/System/Console/union.INPUT_RECORD_0.html
pub const SERIALIZED_INPUT_RECORD_0_LENGTH: usize = 13;

/// Length of a serialized process id exchanged during the named-pipe PID
/// handshake. Matches the size of a `u32` on all supported platforms.
pub const SERIALIZED_PID_LENGTH: usize = 4;

/// Tag byte identifying an input-record message on the daemon-to-client pipe.
///
/// The tag byte is followed by the
/// [`SERIALIZED_INPUT_RECORD_0_LENGTH`]-byte serialized payload produced
/// by [`crate::protocol::serialization::serialize_input_record_0`].
pub const TAG_INPUT_RECORD: u8 = 0x00;

/// Tag byte reserved for client state-change messages.
///
/// Not yet emitted by the daemon. Reserved here to lock in the wire-format
/// numbering used by the issue #179 follow-up PR that wires the
/// [`ClientState`] enum onto the wire.
pub const TAG_STATE_CHANGE: u8 = 0x01;

/// Tag byte identifying a zero-payload keep-alive message on the
/// daemon-to-client pipe.
///
/// Used by the daemon's pipe server to detect early when the client end of
/// the pipe is closed.
pub const TAG_KEEP_ALIVE: u8 = 0xFF;

/// Length on the wire of a framed input-record message: the tag byte plus
/// the existing [`SERIALIZED_INPUT_RECORD_0_LENGTH`]-byte payload.
pub const FRAMED_INPUT_RECORD_LENGTH: usize = 1 + SERIALIZED_INPUT_RECORD_0_LENGTH;

/// Length on the wire of a framed keep-alive message: just the tag byte.
pub const FRAMED_KEEP_ALIVE_LENGTH: usize = 1;

/// Runtime state of a single client.
///
/// Authoritative state value tracked per client by the daemon. The daemon
/// consults this value to gate input forwarding for each client. Hosted in
/// the protocol module ahead of the wire-format follow-up that will push
/// transitions to the client over the named pipe.
///
/// `#[repr(u8)]` so the enum can round-trip through a single byte once the
/// wire format is added.
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClientState {
    /// Client receives and replays all input records broadcast by the
    /// daemon.
    Active = 0,
    /// Daemon suppresses input forwarding for this client.
    Disabled = 1,
}

/// Daemon-to-client message variants exchanged over the named pipe.
///
/// Each variant maps to a distinct tag byte at the start of the wire
/// representation; see [`TAG_INPUT_RECORD`] and [`TAG_KEEP_ALIVE`].
#[derive(Clone, Copy)]
pub enum DaemonToClientMessage {
    /// Carries an [`INPUT_RECORD_0`] (`KeyEvent`) to be replayed to the
    /// client's console input buffer.
    InputRecord(INPUT_RECORD_0),
    /// Empty payload sent on idle by the daemon's pipe server to detect a
    /// closed client pipe.
    KeepAlive,
}

#[cfg(test)]
#[path = "../tests/protocol/test_mod.rs"]
mod test_mod;
