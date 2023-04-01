use std::env;

pub const PKG_NAME: &str = env!("CARGO_PKG_NAME");
pub const PIPE_NAME: &str = concat!(r"\\.\pipe\", env!("CARGO_PKG_NAME"), "-named-pipe-for-ipc");
