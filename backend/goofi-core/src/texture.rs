//! The texture frame: a picture as any engine makes it and any engine reads it.
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

    /// The pixels as tight-packed 8-bit RGBA, row 0 the top: what a video encoder takes.
    pub fn to_rgba8(&self) -> Vec<u8> {
        let (channels, unit) = (self.format.channels(), self.format.bytes_per_channel());
        let row = self.width as usize * channels * unit;
        let mut rgba = Vec::with_capacity(self.width as usize * self.height as usize * 4);
        for line in self.bytes.chunks_exact(self.stride) {
            for pixel in line[..row].chunks_exact(channels * unit) {
                for channel in 0..4 {
                    rgba.push(match (channel < channels, unit) {
                        (false, _) => 255,
                        (true, 1) => pixel[channel],
                        (true, _) => {
                            let value = f32::from_le_bytes(pixel[channel * 4..channel * 4 + 4].try_into().expect("four bytes"));
                            (value.clamp(0.0, 1.0) * 255.0).round() as u8
                        }
                    });
                }
            }
        }
        rgba
    }

    /// The pixels as the `[H, W, C]` array of values in 0..1 a reader of numbers takes, under `meta`.
    pub fn to_array(&self, meta: crate::Meta) -> Result<crate::Data, String> {
        let (channels, unit) = (self.format.channels(), self.format.bytes_per_channel());
        let row = self.width as usize * channels * unit;
        let mut buf = Vec::with_capacity(self.width as usize * self.height as usize * channels * 4);
        for line in self.bytes.chunks_exact(self.stride) {
            match unit {
                1 => buf.extend(line[..row].iter().flat_map(|v| (f32::from(*v) / 255.0).to_le_bytes())),
                _ => buf.extend_from_slice(&line[..row]),
            }
        }
        let shape = vec![self.height as usize, self.width as usize, channels];
        crate::Data::array_f32(shape, buf, meta).map_err(|e| e.to_string())
    }
}

/// Where a texture frame's picture is held. The CPU form is the one form so far.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum Texture {
    Pixels(Pixels),
}

impl Texture {
    pub fn size(&self) -> (u32, u32) {
        match self {
            Self::Pixels(p) => (p.width, p.height),
        }
    }
    pub fn validate(&self) -> Result<(), String> {
        match self {
            Self::Pixels(p) => p.validate(),
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
