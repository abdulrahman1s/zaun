//! The helper's pipes (ADR-0042): Hello and requests in on stdin, events out on stdout.

pub mod entry;
pub mod lines;
pub mod reader;
pub mod writer;

pub use entry::{run, run_with};
