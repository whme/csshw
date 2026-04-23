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

/// Length of a serialized process id exchanged during the named-pipe PID
/// handshake. Matches the size of a `u32` on all supported platforms.
pub const SERIALIZED_PID_LENGTH: usize = 4;

#[cfg(test)]
#[path = "../tests/serde/test_mod.rs"]
mod test_mod;
