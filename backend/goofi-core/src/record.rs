//! What a recording is made of: the frames an armed output hands over, and the per-output
//! settings the graph keeps.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// What an armed output hands the recorder: the frame as the node made it, or a texture the
/// graphics engine read back at eight bits a channel, which is what a video takes and what a
/// `Data` of floats would only cost four times the memory to say.
#[derive(Clone)]
pub enum Frame {
    Data(crate::Data),
    Texels(std::sync::Arc<Texels>),
}

/// One readback: `width * height` tight-packed RGBA texels, row 0 the top, and the stamps every
/// frame carries — its instant, its number and the rate it was rendered at.
pub struct Texels {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
    pub meta: crate::Meta,
}

impl Frame {
    pub fn meta(&self) -> &crate::Meta {
        match self {
            Frame::Data(d) => d.meta(),
            Frame::Texels(t) => &t.meta,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
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

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct RecordedOutput {
    pub slot: String,
    pub quality: VideoQuality,
    /// Which arming this is, minted by the graph: a slot armed again records on a fresh service.
    #[serde(skip)]
    pub serial: u64,
}
