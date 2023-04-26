pub const PKG_NAME: &str = env!("CARGO_PKG_NAME");
// https://learn.microsoft.com/en-us/windows/win32/ipc/pipe-namess
pub const PIPE_NAME: &str = concat!(r"\\.\pipe\", env!("CARGO_PKG_NAME"), "-named-pipe-for-ipc");
pub const DEFAULT_SSH_USERNAME_KEY: &str =
    concat!(env!("CARGO_PKG_NAME"), "VerySpecialAndUniqueUsername");
