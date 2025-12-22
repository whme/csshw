//! Shared constants.

/// Name of the package.
pub const PKG_NAME: &str = env!("CARGO_PKG_NAME");
/// Name of the Pipe used for interprocess comunication between daemon and clients.
///
/// <https://learn.microsoft.com/en-us/windows/win32/ipc/pipe-names>
pub const PIPE_NAME: &str = concat!(r"\\.\pipe\", env!("CARGO_PKG_NAME"), "-named-pipe-for-ipc");
/// Maximum expected length of window title of a client window.
///
/// Only used as fixed buffer size when reading the current window title
/// to check if we need to reset it.
/// If the actual window title exceeds this length it just be cut off at that point.
/// Dummy
pub const MAX_WINDOW_TITLE_LENGTH: usize = 2048;
