//! What a goofi process holds and lets go of: its session, its children, its threads, its
//! resource index, its log and the progress a binary shows beside it. Below the vocabulary.

/// Where a reader reports what goofi could not handle.
pub const ISSUES: &str = "https://github.com/KairosHive/goofi/issues";

pub mod child;
pub mod layout;
pub mod log;
pub mod process;
pub mod progress;
pub mod scope;
pub mod session;
pub mod sync;
pub mod worker;
