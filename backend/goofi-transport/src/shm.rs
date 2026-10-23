//! One named shared-memory segment, made by one process and opened by another: POSIX shared
//! memory on Unix, a pagefile-backed section on Windows. The maker unlinks it; a crash's
//! leftover carries the session's prefix, which the session sweep removes.

use std::ptr::NonNull;

/// A mapped segment. `len` is the whole mapping, header included.
pub(crate) struct Segment {
    ptr: NonNull<u8>,
    len: usize,
    /// The name an owner unlinks; a Windows section goes with its last handle, unnamed.
    #[cfg(unix)]
    name: String,
    /// Whether this process made it, and so unlinks it.
    #[cfg(unix)]
    owner: bool,
    #[cfg(windows)]
    handle: windows_sys::Win32::Foundation::HANDLE,
}

// SAFETY: the mapping is shared by design; the seqlock in `exchange` orders every access.
unsafe impl Send for Segment {}

impl Segment {
    pub(crate) fn len(&self) -> usize {
        self.len
    }

    pub(crate) fn as_ptr(&self) -> *mut u8 {
        self.ptr.as_ptr()
    }
}

#[cfg(unix)]
mod imp {
    use super::Segment;
    use std::ffi::CString;
    use std::ptr::NonNull;

    fn cname(name: &str) -> Result<CString, String> {
        CString::new(name).map_err(|_| format!("a segment name with a NUL: {name:?}"))
    }

    fn map(fd: libc::c_int, len: usize) -> Result<NonNull<u8>, String> {
        // SAFETY: a shared, read-write mapping of a descriptor sized to `len` by the caller.
        let ptr = unsafe { libc::mmap(std::ptr::null_mut(), len, libc::PROT_READ | libc::PROT_WRITE, libc::MAP_SHARED, fd, 0) };
        if ptr == libc::MAP_FAILED {
            return Err(format!("mmap: {}", std::io::Error::last_os_error()));
        }
        NonNull::new(ptr as *mut u8).ok_or_else(|| "mmap answered null".to_string())
    }

    impl Segment {
        /// Make `name` of `len` bytes, zeroed, refusing one that exists.
        pub(crate) fn create(name: &str, len: usize) -> Result<Segment, String> {
            let c = cname(name)?;
            // SAFETY: a valid C string; the descriptor is closed below whatever happens.
            let fd = unsafe { libc::shm_open(c.as_ptr(), libc::O_CREAT | libc::O_EXCL | libc::O_RDWR, 0o600) };
            if fd < 0 {
                return Err(format!("shm_open {name}: {}", std::io::Error::last_os_error()));
            }
            // SAFETY: a plain ftruncate on the descriptor just opened.
            let sized = unsafe { libc::ftruncate(fd, len as libc::off_t) };
            let mapped = if sized == 0 { map(fd, len) } else { Err(format!("ftruncate {name}: {}", std::io::Error::last_os_error())) };
            // SAFETY: closing what this function opened; the mapping outlives the descriptor.
            unsafe { libc::close(fd) };
            match mapped {
                Ok(ptr) => Ok(Segment { ptr, len, name: name.to_string(), owner: true }),
                Err(e) => {
                    // SAFETY: a valid C string.
                    unsafe { libc::shm_unlink(c.as_ptr()) };
                    Err(e)
                }
            }
        }

        /// Open `name`, made by another process, whole.
        pub(crate) fn open(name: &str) -> Result<Segment, String> {
            let c = cname(name)?;
            // SAFETY: a valid C string; the descriptor is closed below whatever happens.
            let fd = unsafe { libc::shm_open(c.as_ptr(), libc::O_RDWR, 0) };
            if fd < 0 {
                return Err(format!("shm_open {name}: {}", std::io::Error::last_os_error()));
            }
            // SAFETY: a zeroed stat buffer, filled by fstat on the open descriptor.
            let len = unsafe {
                let mut st: libc::stat = std::mem::zeroed();
                if libc::fstat(fd, &mut st) != 0 {
                    libc::close(fd);
                    return Err(format!("fstat {name}: {}", std::io::Error::last_os_error()));
                }
                st.st_size as usize
            };
            let mapped = map(fd, len);
            // SAFETY: closing what this function opened.
            unsafe { libc::close(fd) };
            Ok(Segment { ptr: mapped?, len, name: name.to_string(), owner: false })
        }
    }

    impl Drop for Segment {
        fn drop(&mut self) {
            // SAFETY: the mapping this value made, unmapped once; an owner's name unlinked once.
            unsafe {
                libc::munmap(self.ptr.as_ptr() as *mut libc::c_void, self.len);
                if self.owner {
                    if let Ok(c) = cname(&self.name) {
                        libc::shm_unlink(c.as_ptr());
                    }
                }
            }
        }
    }
}

#[cfg(windows)]
mod imp {
    use super::Segment;
    use std::ptr::NonNull;
    use windows_sys::Win32::Foundation::{CloseHandle, HANDLE, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::System::Memory::{
        CreateFileMappingW, MapViewOfFile, OpenFileMappingW, UnmapViewOfFile, FILE_MAP_ALL_ACCESS, PAGE_READWRITE,
    };

    fn wide(name: &str) -> Vec<u16> {
        name.encode_utf16().chain(std::iter::once(0)).collect()
    }

    fn view(handle: HANDLE, len: usize) -> Result<NonNull<u8>, String> {
        // SAFETY: a valid section handle; `len` 0 maps the whole section.
        let ptr = unsafe { MapViewOfFile(handle, FILE_MAP_ALL_ACCESS, 0, 0, len) };
        NonNull::new(ptr.Value as *mut u8).ok_or_else(|| format!("MapViewOfFile: {}", std::io::Error::last_os_error()))
    }

    impl Segment {
        pub(crate) fn create(name: &str, len: usize) -> Result<Segment, String> {
            let wide = wide(name);
            // SAFETY: a pagefile-backed section of `len` bytes under a NUL-terminated name.
            let handle = unsafe {
                CreateFileMappingW(INVALID_HANDLE_VALUE, std::ptr::null(), PAGE_READWRITE, (len as u64 >> 32) as u32, len as u32, wide.as_ptr())
            };
            if handle.is_null() {
                return Err(format!("CreateFileMapping {name}: {}", std::io::Error::last_os_error()));
            }
            match view(handle, len) {
                Ok(ptr) => Ok(Segment { ptr, len, handle }),
                Err(e) => {
                    // SAFETY: the handle just made.
                    unsafe { CloseHandle(handle) };
                    Err(e)
                }
            }
        }

        /// Open `name` whole; the length is read back from the mailbox header by the caller, so
        /// the view is the section's own size.
        pub(crate) fn open(name: &str) -> Result<Segment, String> {
            let wide = wide(name);
            // SAFETY: a NUL-terminated name.
            let handle = unsafe { OpenFileMappingW(FILE_MAP_ALL_ACCESS, 0, wide.as_ptr()) };
            if handle.is_null() {
                return Err(format!("OpenFileMapping {name}: {}", std::io::Error::last_os_error()));
            }
            let ptr = match view(handle, 0) {
                Ok(ptr) => ptr,
                Err(e) => {
                    // SAFETY: the handle just opened.
                    unsafe { CloseHandle(handle) };
                    return Err(e);
                }
            };
            // The first word of every mailbox segment is its capacity, written by the maker.
            // SAFETY: the maker zeroed the section and wrote its header before naming it.
            let capacity = unsafe { std::ptr::read_volatile(ptr.as_ptr() as *const u64) } as usize;
            Ok(Segment { ptr, len: super::super::exchange::mailbox_len(capacity), handle })
        }
    }

    impl Drop for Segment {
        fn drop(&mut self) {
            // SAFETY: the view and handle this value holds, released once. The section goes with
            // its last handle; there is nothing to unlink.
            unsafe {
                let view = windows_sys::Win32::System::Memory::MEMORY_MAPPED_VIEW_ADDRESS { Value: self.ptr.as_ptr() as *mut _ };
                UnmapViewOfFile(view);
                CloseHandle(self.handle);
            }
        }
    }
}
