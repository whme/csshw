//! Per-environment glue for `record-demo`.
//!
//! Each submodule is responsible for preparing the recording
//! environment (config override, fake homes, optional desktop
//! normalisation) and then handing control to
//! [`crate::demo::driver::run`] - directly (`local`) or through a
//! booted Windows Sandbox VM (`sandbox`). v2 will add a `ci_runner`
//! provider for GitHub-hosted `windows-2022`.

pub mod local;
pub mod sandbox;
