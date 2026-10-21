//! The `zet` library: the composition root of the workspace.
//!
//! `mode` decides what a given argv means (CLI, window, usage) and
//! `window` runs the terminal window. The binary in `main.rs` is the
//! only other entry point.

pub mod mode;
pub mod window;
