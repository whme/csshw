use std::env;

pub const PKG_NAME: &str = env!("CARGO_PKG_NAME");
// https://learn.microsoft.com/en-us/windows/win32/ipc/pipe-namess
pub const PIPE_NAME: &str = concat!(r"\\.\pipe\", env!("CARGO_PKG_NAME"), "-named-pipe-for-ipc");
