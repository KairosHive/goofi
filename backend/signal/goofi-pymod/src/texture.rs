//! A texture frame in Python: made from pixels by a node that returns one, read as pixels by a
//! node that takes one. Both execution tiers use the same codec and validation. `goofi.gpu`
//! (`gpu.py`) is the CUDA side: a picture made from CUDA memory, and read as CUDA memory.
use goofi_core::texture::{PixelFormat, Pixels, Shared, SharedTexel, Texture as Frame};
use pyo3::prelude::*;
use pyo3::types::PyByteArray;

fn gpu<'py>(py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
    py.import("goofi")?.getattr("gpu")
}

impl Texture {
    fn held(&self) -> PyResult<&Frame> {
        self.frame.as_texture().map_err(pyo3::exceptions::PyTypeError::new_err)
    }

    fn of(frame: Frame) -> PyResult<Self> {
        Ok(Self { frame: goofi_core::Data::texture(frame, Default::default()).map_err(pyo3::exceptions::PyValueError::new_err)? })
    }

    /// What `goofi.gpu` answers where it put a picture into memory the graphics engine shares.
    fn shared_of(delivered: &Bound<'_, PyAny>) -> Option<Frame> {
        let (id, width, height, pitch, bytes) = delivered.extract::<(u64, u32, u32, u32, u32)>().ok()?;
        Some(Frame::Shared(Shared { id, width, height, pitch, texel: SharedTexel::of(bytes)? }))
    }
}

#[pyclass]
pub struct Texture {
    pub frame: goofi_core::Data,
}

#[pymethods]
impl Texture {
    /// Top-row-first RGB/RGBA uint8 or float32 pixels with straight alpha: a numpy array, or an
    /// array in CUDA memory (a torch tensor, anything with `__cuda_array_interface__`). In a node
    /// the graphics tick calls, a CUDA picture stays in device memory.
    #[new]
    fn new(py: Python<'_>, pixels: &Bound<'_, PyAny>) -> PyResult<Self> {
        let gpu = gpu(py)?;
        let delivered;
        let pixels = if gpu.call_method1("on_cuda", (pixels,))?.extract()? {
            delivered = gpu.call_method1("deliver", (pixels,))?;
            if let Some(shared) = Self::shared_of(&delivered) {
                return Self::of(shared);
            }
            &delivered
        } else {
            pixels
        };
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
        // In a node the graphics tick calls, 8-bit RGBA goes into memory the engine shares.
        if format == PixelFormat::Rgba8 {
            if let Some(shared) = Self::shared_of(&gpu.call_method1("host", (&array,))?) {
                return Self::of(shared);
            }
        }
        let (at, len): (usize, usize) = (array.getattr("ctypes")?.getattr("data")?.extract()?, array.getattr("nbytes")?.extract()?);
        // SAFETY: `array` is contiguous, holds `nbytes` bytes at that address, and lives past the copy.
        let bytes = unsafe { std::slice::from_raw_parts(at as *const u8, len) }.to_vec();
        Self::of(Frame::Pixels(Pixels {
            width,
            height,
            stride: width as usize * channels * format.bytes_per_channel(),
            format,
            bytes,
        }))
    }

    /// The pixels as a numpy array of its own, `[height, width, 3 or 4]`, `uint8` or `float32`
    /// as held. A picture in device memory is copied out as RGBA, `float32` or `uint8`.
    fn numpy<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let p = match self.held()? {
            Frame::Pixels(p) => p,
            Frame::Shared(s) => return gpu(py)?.call_method1("download", (s.id,)),
        };
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
        let flat = py.import("numpy")?.getattr("frombuffer")?.call1((PyByteArray::new(py, bytes), dtype))?;
        flat.call_method1("reshape", ((p.height as usize, p.width as usize, channels),))
    }

    /// The picture in CUDA memory, as an object with `__cuda_array_interface__`. A picture in
    /// device memory is that memory, RGBA `float16` rows at a pitch, with no copy; pixels are
    /// copied up as they are held.
    fn cuda<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        match self.held()? {
            Frame::Shared(s) => gpu(py)?.call_method1("view", (s.id,)),
            Frame::Pixels(_) => gpu(py)?.call_method1("cuda", (self.numpy(py)?,)),
        }
    }

    /// The picture as a torch tensor `[height, width, channels]`: on its device memory with no
    /// copy (RGBA `float16`, on CUDA), else of its pixels on the CPU. `device` moves it.
    #[pyo3(signature = (device=None))]
    fn torch<'py>(&self, py: Python<'py>, device: Option<&Bound<'py, PyAny>>) -> PyResult<Bound<'py, PyAny>> {
        let source = match self.held()? {
            Frame::Shared(s) => gpu(py)?.call_method1("view", (s.id,))?,
            Frame::Pixels(_) => self.numpy(py)?,
        };
        gpu(py)?.call_method1("tensor", (source, device))
    }

    /// Whether the picture is in device memory this node shares with the graphics engine.
    #[getter]
    fn shared(&self) -> PyResult<bool> {
        Ok(matches!(self.held()?, Frame::Shared(_)))
    }

    /// `(height, width, channels)`.
    #[getter]
    fn shape(&self) -> PyResult<(usize, usize, usize)> {
        Ok(match self.held()? {
            Frame::Pixels(p) => (p.height as usize, p.width as usize, p.format.channels()),
            Frame::Shared(s) => (s.height as usize, s.width as usize, 4),
        })
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
