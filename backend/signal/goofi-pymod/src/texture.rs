//! A texture frame in Python: made from pixels by a node that returns one, read as pixels by a
//! node that takes one. Both execution tiers use the same codec and validation.
use goofi_core::texture::{PixelFormat, Pixels, Texture as Frame};
use pyo3::prelude::*;
use pyo3::types::PyBytes;

impl Texture {
    fn pixels(&self) -> PyResult<&Pixels> {
        match self.frame.as_texture() {
            Ok(Frame::Pixels(p)) => Ok(p),
            Err(why) => Err(pyo3::exceptions::PyTypeError::new_err(why)),
        }
    }
}

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

    /// The pixels as a fresh numpy array `[height, width, 3 or 4]`, `uint8` or `float32` as held.
    fn numpy<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let p = self.pixels()?;
        let (channels, unit) = (p.format.channels(), p.format.bytes_per_channel());
        let row = p.width as usize * channels * unit;
        let dtype = if unit == 1 { "|u1" } else { "<f4" };
        let packed: Vec<u8>;
        let bytes = if p.stride == row {
            &p.bytes
        } else {
            packed = p.bytes.chunks_exact(p.stride).flat_map(|line| &line[..row]).copied().collect();
            &packed
        };
        let flat = py.import("numpy")?.getattr("frombuffer")?.call1((PyBytes::new(py, bytes), dtype))?;
        flat.call_method1("reshape", ((p.height as usize, p.width as usize, channels),))
    }

    /// `(height, width, channels)`.
    #[getter]
    fn shape(&self) -> PyResult<(usize, usize, usize)> {
        let p = self.pixels()?;
        Ok((p.height as usize, p.width as usize, p.format.channels()))
    }

    /// numpy's protocol, so `np.asarray(texture)` is `texture.numpy()`.
    #[pyo3(signature = (dtype=None, copy=None))]
    fn __array__<'py>(&self, py: Python<'py>, dtype: Option<&Bound<'py, PyAny>>, copy: Option<bool>) -> PyResult<Bound<'py, PyAny>> {
        let _ = copy;
        let array = self.numpy(py)?;
        match dtype {
            Some(dtype) => array.call_method1("astype", (dtype,)),
            None => Ok(array),
        }
    }
}
