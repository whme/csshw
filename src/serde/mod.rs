//! Serialization/Deserialization implemention for windows [INPUT_RECORD_0].

#![deny(clippy::implicit_return)]
#![allow(clippy::needless_return)]
#![warn(missing_docs)]

#[allow(missing_docs)]
pub mod deserialization;
#[allow(missing_docs)]
pub mod serialization;

/// Lenght of a serialized [INPUT_RECORD_0][1]
///
/// [1]: https://microsoft.github.io/windows-docs-rs/doc/windows/Win32/System/Console/union.INPUT_RECORD_0.html
pub const SERIALIZED_INPUT_RECORD_0_LENGTH: usize = 18;
