//! Python texture submissions use the same codec and validation in both execution tiers.
use goofi_core::texture::{PixelFormat, Pixels, Texture as Frame};
use pyo3::prelude::*;
use pyo3::types::PyBytes;

#[pyclass]
pub struct Texture {
    pub frame: goofi_core::Data,
}

#[pymethods]
impl Texture {
    /// Top-row-first RGB/RGBA uint8 or float32 pixels with straight alpha.
    #[new]
    fn new(py: Python<'_>, pixels: &Bound<'_, PyAny>) -> PyResult<Self> {
        let array = py.import("numpy")?.call_method1("ascontiguousarray", (pixels,))?;
        let shape: Vec<usize> = array.getattr("shape")?.extract()?;
        let [height, width, channels] = shape.as_slice() else {
            return Err(pyo3::exceptions::PyValueError::new_err("texture pixels need shape [height, width, 3 or 4]"));
        };
        let dtype: String = array.getattr("dtype")?.getattr("str")?.extract()?;
        let format = match (dtype.as_str(), channels) {
            ("|u1", 3) => PixelFormat::Rgb8,
            ("|u1", 4) => PixelFormat::Rgba8,
            ("<f4" | "=f4", 3) => PixelFormat::Rgb32Float,
            ("<f4" | "=f4", 4) => PixelFormat::Rgba32Float,
            _ => {
                return Err(pyo3::exceptions::PyValueError::new_err(
                    "texture pixels must be RGB/RGBA uint8 or little-endian float32",
                ));
            }
        };
        let width = u32::try_from(*width).map_err(|e| pyo3::exceptions::PyValueError::new_err(e.to_string()))?;
        let height = u32::try_from(*height).map_err(|e| pyo3::exceptions::PyValueError::new_err(e.to_string()))?;
        let bytes = array.call_method0("tobytes")?.cast::<PyBytes>()?.as_bytes().to_vec();
        let frame = Frame::Pixels(Pixels {
            width,
            height,
            stride: width as usize * channels * format.bytes_per_channel(),
            format,
            bytes,
        });
        Ok(Self {
            frame: goofi_core::Data::texture(frame, Default::default())
                .map_err(pyo3::exceptions::PyValueError::new_err)?,
        })
    }
}
