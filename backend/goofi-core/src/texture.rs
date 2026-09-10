//! Host submissions for engine-owned textures. No GPU object crosses a process or ABI boundary.
use serde::{Deserialize, Serialize};

pub const MAX_SIZE: u32 = 8192;

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum PixelFormat {
    Rgb8,
    Rgba8,
    Rgb32Float,
    Rgba32Float,
}
impl PixelFormat {
    pub fn channels(self) -> usize {
        match self {
            Self::Rgb8 | Self::Rgb32Float => 3,
            _ => 4,
        }
    }
    pub fn bytes_per_channel(self) -> usize {
        match self {
            Self::Rgb8 | Self::Rgba8 => 1,
            _ => 4,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Pixels {
    pub width: u32,
    pub height: u32,
    pub stride: usize,
    pub format: PixelFormat,
    /// Top row first, straight alpha, RGB values in the patch's working space.
    #[serde(with = "bytes")]
    pub bytes: Vec<u8>,
}

impl Pixels {
    pub fn validate(&self) -> Result<(), String> {
        dimensions(self.width, self.height)?;
        let row = self.width as usize * self.format.channels() * self.format.bytes_per_channel();
        if self.stride < row {
            return Err("texture stride is shorter than a row".into());
        }
        let length = self.stride.checked_mul(self.height as usize).ok_or("texture byte length overflow")?;
        if self.bytes.len() != length {
            return Err(format!("texture has {} bytes; expected {length}", self.bytes.len()));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum Texture {
    Pixels(Pixels),
    /// GPU work encoded against the engine-owned output. The source defines `fn shade(uv: vec2f) -> vec4f`.
    Render {
        width: u32,
        height: u32,
        source: String,
    },
}

impl Texture {
    pub fn size(&self) -> (u32, u32) {
        match self {
            Self::Pixels(p) => (p.width, p.height),
            Self::Render { width, height, .. } => (*width, *height),
        }
    }
    pub fn validate(&self) -> Result<(), String> {
        match self {
            Self::Pixels(p) => p.validate(),
            Self::Render { width, height, source } => {
                dimensions(*width, *height)?;
                if source.is_empty() || source.len() > 1024 * 1024 {
                    return Err("texture program must contain 1..1048576 bytes".into());
                }
                Ok(())
            }
        }
    }
}

fn dimensions(width: u32, height: u32) -> Result<(), String> {
    if width == 0 || height == 0 || width > MAX_SIZE || height > MAX_SIZE {
        return Err(format!("texture dimensions {width}x{height} must be within 1..{MAX_SIZE}"));
    }
    Ok(())
}

mod bytes {
    use serde::{Deserializer, Serializer};

    pub fn serialize<S: Serializer>(bytes: &[u8], serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_bytes(bytes)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Vec<u8>, D::Error> {
        struct Bytes;
        impl serde::de::Visitor<'_> for Bytes {
            type Value = Vec<u8>;
            fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str("texture pixel bytes")
            }
            fn visit_bytes<E: serde::de::Error>(self, value: &[u8]) -> Result<Self::Value, E> {
                Ok(value.to_vec())
            }
            fn visit_byte_buf<E: serde::de::Error>(self, value: Vec<u8>) -> Result<Self::Value, E> {
                Ok(value)
            }
        }
        deserializer.deserialize_byte_buf(Bytes)
    }
}
