pub mod deserialization;
pub mod serialization;

/// Lenght of a serialized [INPUT_RECORD_0][1]
///
/// [1]: https://microsoft.github.io/windows-docs-rs/doc/windows/Win32/System/Console/union.INPUT_RECORD_0.html
pub const SERIALIZED_INPUT_RECORD_0_LENGTH: usize = 18;
