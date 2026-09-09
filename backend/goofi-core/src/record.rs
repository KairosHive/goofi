//! Per-output recording settings shared by the graph and encoders.

use serde::{Deserialize, Serialize};

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
