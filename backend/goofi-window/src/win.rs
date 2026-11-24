//! Win32: a window class of goofi's own, and the thread's message queue pumped for every window
//! on it — the plugin's child windows included, which is what a plugin's timers ride on.

use std::cell::RefCell;
use std::ffi::c_void;
use std::num::NonZeroIsize;
use std::sync::Arc;
use std::time::Instant;

use windows_sys::core::BOOL;
use windows_sys::Win32::Foundation::{HANDLE, HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows_sys::Win32::Graphics::Gdi::{
    EnumDisplayMonitors, GetDC, GetMonitorInfoW, ReleaseDC, SetDIBitsToDevice, BITMAPINFO, BITMAPINFOHEADER, BI_RGB,
    DIB_RGB_COLORS, HDC, HMONITOR, MONITORINFO,
};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::System::Threading::{CreateEventW, SetEvent, INFINITE};
use windows_sys::Win32::UI::WindowsAndMessaging::*;

use super::{rwh, Handles, Id, Monitor, Pumped, Screen, Wake};

pub struct Platform {
    class: Vec<u16>,
    event: HANDLE,
}

/// An auto-reset event the pump waits on beside the queue; setting it is thread-safe.
pub struct Waker(isize);

unsafe impl Send for Waker {}
unsafe impl Sync for Waker {}

impl Wake for Waker {
    fn wake(&self) {
        unsafe { SetEvent(self.0 as HANDLE) };
    }
}

thread_local! {
    static CLOSED: RefCell<Vec<Id>> = const { RefCell::new(Vec::new()) };
}

/// `WS_CLIPCHILDREN`: a plugin's editor is a CHILD window, and a parent paint without this
/// flickers over it once a frame.
const STYLE: u32 = (WS_OVERLAPPEDWINDOW | WS_CLIPCHILDREN) & !WS_MAXIMIZEBOX & !WS_THICKFRAME;
/// A window that covers its monitor: no frame to adjust for.
const COVER: u32 = WS_POPUP | WS_CLIPCHILDREN;

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// A close is REPORTED, never performed here: the window goes once the plugin's view is off it.
unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    if msg == WM_CLOSE {
        CLOSED.with(|c| c.borrow_mut().push(hwnd as usize as Id));
        return 0;
    }
    DefWindowProcW(hwnd, msg, wp, lp)
}

/// The outer size of a window of `style` for a client area the plugin asked for.
fn framed((w, h): (u32, u32), style: u32) -> (i32, i32) {
    let mut r = RECT { left: 0, top: 0, right: w.min(i32::MAX as u32) as i32, bottom: h.min(i32::MAX as u32) as i32 };
    unsafe { AdjustWindowRect(&mut r, style, 0) };
    (r.right - r.left, r.bottom - r.top)
}

/// One monitor's rectangle, and whether it is the primary, gathered into the list `lp` points to.
unsafe extern "system" fn gather(monitor: HMONITOR, _dc: HDC, _rect: *mut RECT, lp: LPARAM) -> BOOL {
    let mut info: MONITORINFO = std::mem::zeroed();
    info.cbSize = std::mem::size_of::<MONITORINFO>() as u32;
    if GetMonitorInfoW(monitor, &mut info) != 0 {
        let r = info.rcMonitor;
        let found = &mut *(lp as *mut Vec<(bool, Monitor)>);
        let size = ((r.right - r.left).max(0) as u32, (r.bottom - r.top).max(0) as u32);
        found.push((info.dwFlags & MONITORINFOF_PRIMARY == 0, Monitor { at: (r.left, r.top), size }));
    }
    1
}

impl Platform {
    pub fn open() -> Result<Platform, String> {
        let class = wide("goofiWindow");
        unsafe {
            let mut wc: WNDCLASSW = std::mem::zeroed();
            wc.lpfnWndProc = Some(wndproc);
            wc.hInstance = GetModuleHandleW(std::ptr::null());
            wc.lpszClassName = class.as_ptr();
            wc.hCursor = LoadCursorW(std::ptr::null_mut(), IDC_ARROW);
            RegisterClassW(&wc);
            let event = CreateEventW(std::ptr::null(), 0, 0, std::ptr::null());
            if event.is_null() {
                return Err("no wake event".into());
            }
            Ok(Platform { class, event })
        }
    }

}

impl Screen for Platform {
    fn waker(&self) -> Arc<dyn Wake> {
        Arc::new(Waker(self.event as isize))
    }

    /// The desktop's monitors, the primary first and the rest left to right.
    fn monitors(&self) -> Vec<Monitor> {
        let mut found: Vec<(bool, Monitor)> = Vec::new();
        unsafe { EnumDisplayMonitors(std::ptr::null_mut(), std::ptr::null(), Some(gather), &mut found as *mut _ as LPARAM) };
        found.sort_by_key(|(secondary, m)| (*secondary, m.at.0, m.at.1));
        found.into_iter().map(|(_, m)| m).collect()
    }

    fn create(&mut self, title: &str, size: (u32, u32), on: Option<Monitor>, cover: bool) -> Result<(Id, *mut c_void), String> {
        let name = wide(title);
        let style = if cover { COVER } else { STYLE };
        let (w, h) = match on {
            Some(m) if cover => (m.size.0 as i32, m.size.1 as i32),
            _ => framed(size, style),
        };
        let (x, y) = match on {
            Some(m) if cover => m.at,
            Some(m) => (m.at.0 + (m.size.0 as i32 - w).max(0) / 2, m.at.1 + (m.size.1 as i32 - h).max(0) / 2),
            None => (CW_USEDEFAULT, CW_USEDEFAULT),
        };
        let hwnd = unsafe {
            CreateWindowExW(
                0,
                self.class.as_ptr(),
                name.as_ptr(),
                style,
                x,
                y,
                w,
                h,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                GetModuleHandleW(std::ptr::null()),
                std::ptr::null(),
            )
        };
        if hwnd.is_null() {
            return Err("could not open a window".into());
        }
        unsafe {
            ShowWindow(hwnd, SW_SHOWNORMAL);
            SetForegroundWindow(hwnd);
        }
        Ok((hwnd as usize as Id, hwnd))
    }

    fn retitle(&mut self, id: Id, title: &str) {
        unsafe { SetWindowTextW(id as usize as HWND, wide(title).as_ptr()) };
    }

    fn resize(&mut self, id: Id, size: (u32, u32)) {
        let style = unsafe { GetWindowLongW(id as usize as HWND, GWL_STYLE) } as u32;
        let (w, h) = framed(size, style);
        unsafe { SetWindowPos(id as usize as HWND, std::ptr::null_mut(), 0, 0, w, h, SWP_NOMOVE | SWP_NOZORDER) };
    }

    fn destroy(&mut self, id: Id) {
        unsafe { DestroyWindow(id as usize as HWND) };
    }

    fn handles(&self, id: Id) -> Option<Handles> {
        let mut window = rwh::Win32WindowHandle::new(NonZeroIsize::new(id as isize)?);
        window.hinstance = NonZeroIsize::new(unsafe { GetModuleHandleW(std::ptr::null()) } as isize);
        Some(Handles {
            display: rwh::RawDisplayHandle::Windows(rwh::WindowsDisplayHandle::new()),
            window: rwh::RawWindowHandle::Win32(window),
        })
    }

    /// One `SetDIBitsToDevice` a frame. `biHeight` is NEGATIVE, which is how a DIB says its first
    /// row is the top one; a positive height would show the frame upside down.
    fn present(&mut self, id: Id, (w, h): (u32, u32), texels: &[u8]) {
        let hwnd = id as usize as HWND;
        let mut info: BITMAPINFO = unsafe { std::mem::zeroed() };
        info.bmiHeader = BITMAPINFOHEADER {
            biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: w as i32,
            biHeight: -(h as i32),
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB,
            biSizeImage: 0,
            biXPelsPerMeter: 0,
            biYPelsPerMeter: 0,
            biClrUsed: 0,
            biClrImportant: 0,
        };
        unsafe {
            let dc = GetDC(hwnd);
            if dc.is_null() {
                return;
            }
            SetDIBitsToDevice(
                dc,
                0,
                0,
                w,
                h,
                0,
                0,
                0,
                h,
                texels.as_ptr() as *const std::ffi::c_void,
                &info,
                DIB_RGB_COLORS,
            );
            ReleaseDC(hwnd, dc);
        }
    }

    fn pump(&mut self, until: Option<Instant>, _fds: &[i32]) -> Pumped {
        let timeout = until.map_or(INFINITE, |t| t.saturating_duration_since(Instant::now()).as_millis().min(u32::MAX as u128) as u32);
        unsafe {
            MsgWaitForMultipleObjects(1, &self.event, 0, timeout, QS_ALLINPUT);
            let mut msg: MSG = std::mem::zeroed();
            while PeekMessageW(&mut msg, std::ptr::null_mut(), 0, 0, PM_REMOVE) != 0 {
                TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }
        Pumped { closed: CLOSED.with(|c| std::mem::take(&mut *c.borrow_mut())), ready: Vec::new(), dead: false }
    }
}
