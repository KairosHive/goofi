//! Per-output recording settings shared by the graph and encoders.

use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicU64, Ordering};

/// One recording's audio interval, in engine blocks. Only the audio clock opens
/// and closes it; all tracks use these same inclusive/exclusive boundaries.
pub struct FrameWindow {
    first: AtomicU64,
    end: AtomicU64,
}

impl Default for FrameWindow {
    fn default() -> Self {
        Self { first: AtomicU64::new(0), end: AtomicU64::new(u64::MAX) }
    }
}

impl FrameWindow {
    pub fn prepare(&self) {
        self.first.store(u64::MAX, Ordering::Release);
        self.end.store(0, Ordering::Release);
    }

    pub fn begin(&self, block: u64) {
        self.end.store(u64::MAX, Ordering::Release);
        self.first.store(block, Ordering::Release);
    }

    pub fn finish(&self, block: u64) {
        self.end.store(block, Ordering::Release);
    }

    pub fn contains(&self, block: u64) -> bool {
        block >= self.first.load(Ordering::Acquire) && block < self.end.load(Ordering::Acquire)
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VideoQuality {
    Small,
    #[default]
    High,
    VeryHigh,
}

impl VideoQuality {
    pub fn quantizer(self) -> u8 {
        match self {
            Self::Small => 28,
            Self::High => 23,
            Self::VeryHigh => 18,
        }
    }

    pub fn apple_quality(self) -> u8 {
        match self {
            Self::Small => 50,
            Self::High => 65,
            Self::VeryHigh => 80,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecordedOutput {
    pub slot: String,
    pub quality: VideoQuality,
}
