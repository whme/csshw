use std::{env, time::Duration};

pub const PKG_NAME: &str = env!("CARGO_PKG_NAME");
// https://learn.microsoft.com/en-us/windows/win32/ipc/pipe-namess
pub const PIPE_NAME: &str = concat!(r"\\.\pipe\", env!("CARGO_PKG_NAME"), "-named-pipe-for-ipc");
// https://www.ncbi.nlm.nih.gov/pmc/articles/PMC4456887
// (highlighted version: https://shorturl.at/tHIJO)
pub const HUMAN_VISUAL_STIMULUS_TIME: Duration = Duration::from_millis(20);
