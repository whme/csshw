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

#[cfg(test)]
#[path = "../tests/test_serde.rs"]
mod test_serde;
