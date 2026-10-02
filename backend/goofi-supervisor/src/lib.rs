//! What a goofi process holds and lets go of: its session, its children, its threads, its
//! resource index, its log and the progress a binary shows beside it. Below the vocabulary.

pub mod child;
pub mod layout;
pub mod log;
pub mod progress;
pub mod scope;
pub mod session;
pub mod sync;
pub mod worker;
