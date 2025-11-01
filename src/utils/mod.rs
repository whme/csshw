//! Utilities shared by daemon and client.

#![deny(clippy::implicit_return)]
#![allow(
    clippy::needless_return,
    clippy::doc_overindented_list_items,
    rustdoc::private_intra_doc_links
)]

pub mod config;
pub mod constants;
pub mod debug;
pub mod windows;

#[cfg(test)]
#[path = "../tests/utils/test_mod.rs"]
mod test_mod;
