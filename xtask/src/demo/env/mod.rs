//! Per-environment glue for `record-demo`.
//!
//! Each submodule is responsible for preparing the recording
//! environment (config override, fake homes, optional desktop
//! normalisation) and then handing control to
//! [`crate::demo::driver::run`]. v0 ships only [`local`].

pub mod local;
