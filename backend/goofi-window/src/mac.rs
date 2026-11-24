//! AppKit, on the main thread only: the application's event queue pumped by hand, so a wait can
//! carry a deadline. A window the user closed is one that is no longer visible.

use std::ffi::c_void;
use std::ptr::NonNull;
use std::sync::Arc;
use std::time::Instant;

use objc2::rc::{Allocated, Retained};
use objc2::runtime::AnyObject;
use objc2::{class, msg_send, MainThreadMarker};
use objc2_foundation::{NSDate, NSDefaultRunLoopMode, NSPoint, NSRect, NSSize, NSString};

use super::{rwh, Handles, Id, Monitor, Pumped, Screen, Wake};

// AppKit, linked by hand: `objc2-app-kit` is not in the offline registry, so nothing else in the
// graph pulls the framework in — and `class!(NSApplication)` on a class the process never loaded
// panics rather than answering, which took the whole server down at start.
#[link(name = "AppKit", kind = "framework")]
extern "C" {}

pub struct Platform {
    app: Retained<AnyObject>,
    windows: Vec<(Id, Retained<AnyObject>, bool)>,
    /// The image view a presented window's frames go into, made on its first frame.
    views: Vec<(Id, Retained<AnyObject>)>,
}

/// The application, to post a wake-up event to from any thread — the one call AppKit allows there.
pub struct Waker(usize);

unsafe impl Send for Waker {}
unsafe impl Sync for Waker {}

const APPLICATION_DEFINED: usize = 15;
const TITLED_CLOSABLE_MINIATURIZABLE: usize = 1 | 2 | 4;
const BORDERLESS: usize = 0;
/// One above `NSMainMenuWindowLevel`, so a window that covers its screen covers the menu bar too.
const ABOVE_MENU: isize = 25;
const BUFFERED: usize = 2;
const REGULAR: isize = 0;

impl Wake for Waker {
    fn wake(&self) {
        // A pool of its own: this runs on ANY thread, and the event below is autoreleased. Off the
        // main thread there is no run loop to drain one, so each wake would leak an NSEvent.
        objc2::rc::autoreleasepool(|_| unsafe {
            let app = self.0 as *mut AnyObject;
            let event: *mut AnyObject = msg_send![
                class!(NSEvent),
                otherEventWithType: APPLICATION_DEFINED,
                location: NSPoint::ZERO,
                modifierFlags: 0usize,
                timestamp: 0f64,
                windowNumber: 0isize,
                context: std::ptr::null::<AnyObject>(),
                subtype: 0i16,
                data1: 0isize,
                data2: 0isize
            ];
            let _: () = msg_send![app, postEvent: event, atStart: true];
        })
    }
}

impl Platform {
    pub fn open() -> Result<Platform, String> {
        MainThreadMarker::new().ok_or("the window loop needs the main thread")?;
        unsafe {
            let app: Retained<AnyObject> = msg_send![class!(NSApplication), sharedApplication];
            let _: bool = msg_send![&*app, setActivationPolicy: REGULAR];
            let _: () = msg_send![&*app, finishLaunching];
            Ok(Platform { app, windows: Vec::new(), views: Vec::new() })
        }
    }

}

impl Screen for Platform {
    fn waker(&self) -> Arc<dyn Wake> {
        Arc::new(Waker(Retained::as_ptr(&self.app) as usize))
    }

    /// `NSScreen`'s screens in points, the one with the menu bar first and the rest left to right.
    fn monitors(&self) -> Vec<Monitor> {
        let mut found: Vec<Monitor> = unsafe {
            let screens: *mut AnyObject = msg_send![class!(NSScreen), screens];
            if screens.is_null() {
                return Vec::new();
            }
            let count: usize = msg_send![screens, count];
            (0..count)
                .map(|i| {
                    let screen: *mut AnyObject = msg_send![screens, objectAtIndex: i];
                    let f: NSRect = msg_send![screen, frame];
                    Monitor { at: (f.origin.x as i32, f.origin.y as i32), size: (f.size.width as u32, f.size.height as u32) }
                })
                .collect()
        };
        if found.len() > 1 {
            found[1..].sort_by_key(|m| (m.at.0, m.at.1));
        }
        found
    }

    fn create(&mut self, title: &str, (w, h): (u32, u32), on: Option<Monitor>, cover: bool) -> Result<(Id, *mut c_void), String> {
        unsafe {
            // AppKit's origin is the bottom-left corner of the desktop, in points.
            let rect = match on {
                Some(m) if cover => NSRect::new(NSPoint::new(m.at.0 as f64, m.at.1 as f64), NSSize::new(m.size.0 as f64, m.size.1 as f64)),
                Some(m) => {
                    let centre = |origin: i32, span: u32, own: u32| origin as f64 + (span as f64 - own as f64).max(0.0) / 2.0;
                    NSRect::new(NSPoint::new(centre(m.at.0, m.size.0, w), centre(m.at.1, m.size.1, h)), NSSize::new(w as f64, h as f64))
                }
                None => NSRect::new(NSPoint::ZERO, NSSize::new(w as f64, h as f64)),
            };
            let window: Allocated<AnyObject> = msg_send![class!(NSWindow), alloc];
            let window: Retained<AnyObject> = msg_send![
                window,
                initWithContentRect: rect,
                styleMask: if cover { BORDERLESS } else { TITLED_CLOSABLE_MINIATURIZABLE },
                backing: BUFFERED,
                defer: false
            ];
            let _: () = msg_send![&*window, setReleasedWhenClosed: false];
            let _: () = msg_send![&*window, setTitle: &*NSString::from_str(title)];
            if cover {
                let _: () = msg_send![&*window, setLevel: ABOVE_MENU];
            } else if on.is_none() {
                let _: () = msg_send![&*window, center];
            }
            let _: () = msg_send![&*window, makeKeyAndOrderFront: std::ptr::null::<AnyObject>()];
            let _: () = msg_send![&*self.app, activateIgnoringOtherApps: true];
            let view: *mut AnyObject = msg_send![&*window, contentView];
            if view.is_null() {
                return Err("the window has no content view".into());
            }
            let id = Retained::as_ptr(&window) as usize as Id;
            self.windows.push((id, window, true));
            Ok((id, view as *mut c_void))
        }
    }

    fn retitle(&mut self, id: Id, title: &str) {
        if let Some((_, window, _)) = self.windows.iter().find(|(i, ..)| *i == id) {
            let _: () = unsafe { msg_send![&**window, setTitle: &*NSString::from_str(title)] };
        }
    }

    fn resize(&mut self, id: Id, (w, h): (u32, u32)) {
        if let Some((_, window, _)) = self.windows.iter().find(|(i, ..)| *i == id) {
            let _: () = unsafe { msg_send![&**window, setContentSize: NSSize::new(w as f64, h as f64)] };
        }
    }

    fn handles(&self, id: Id) -> Option<Handles> {
        let (_, window, _) = self.windows.iter().find(|(i, ..)| *i == id)?;
        let view: *mut AnyObject = unsafe { msg_send![&**window, contentView] };
        Some(Handles {
            display: rwh::RawDisplayHandle::AppKit(rwh::AppKitDisplayHandle::new()),
            window: rwh::RawWindowHandle::AppKit(rwh::AppKitWindowHandle::new(NonNull::new(view as *mut c_void)?)),
        })
    }

    fn destroy(&mut self, id: Id) {
        self.views.retain(|(i, _)| *i != id);
        if let Some(at) = self.windows.iter().position(|(i, ..)| *i == id) {
            let (_, window, _) = self.windows.remove(at);
            let _: () = unsafe { msg_send![&*window, close] };
        }
    }

    /// An `NSImageView` holding one `NSBitmapImageRep` a frame. The rep is allocated with NULL
    /// planes and filled after, so IT owns the pixels — passing our own pointer would hand AppKit
    /// a buffer the next frame overwrites.
    fn present(&mut self, id: Id, (w, h): (u32, u32), texels: &[u8]) {
        unsafe {
            let view = match self.views.iter().find(|(i, _)| *i == id) {
                Some((_, v)) => v.clone(),
                None => {
                    let Some((_, window, _)) = self.windows.iter().find(|(i, ..)| *i == id) else { return };
                    let frame: NSRect = msg_send![&**window, contentLayoutRect];
                    let view: Allocated<AnyObject> = msg_send![class!(NSImageView), alloc];
                    let view: Retained<AnyObject> = msg_send![view, initWithFrame: frame];
                    // Axes independently: the window is always the frame's own size, so this only
                    // matters while a resize is in flight.
                    // 1 is `NSImageScaleAxesIndependently`; the window is the frame's own size,
                    // so this only shows while a resize is in flight.
                    let _: () = msg_send![&*view, setImageScaling: 1isize];
                    // A SUBVIEW, never the content view: `setContentView:` releases the old one,
                    // which is the very pointer `create` handed out for a plugin to embed into.
                    let _: () = msg_send![&*view, setAutoresizingMask: 18usize];
                    let content: *mut AnyObject = msg_send![&**window, contentView];
                    let _: () = msg_send![content, addSubview: &*view];
                    self.views.push((id, view.clone()));
                    view
                }
            };
            let rep: Allocated<AnyObject> = msg_send![class!(NSBitmapImageRep), alloc];
            let rep: Option<Retained<AnyObject>> = msg_send![
                rep,
                initWithBitmapDataPlanes: std::ptr::null_mut::<*mut u8>(),
                pixelsWide: w as isize,
                pixelsHigh: h as isize,
                bitsPerSample: 8isize,
                samplesPerPixel: 4isize,
                hasAlpha: true,
                isPlanar: false,
                colorSpaceName: &*NSString::from_str("NSDeviceRGBColorSpace"),
                bytesPerRow: w as isize * 4,
                bitsPerPixel: 32isize
            ];
            let Some(rep) = rep else { return };
            let data: *mut u8 = msg_send![&*rep, bitmapData];
            if data.is_null() {
                return;
            }
            std::ptr::copy_nonoverlapping(texels.as_ptr(), data, w as usize * h as usize * 4);
            let image: Allocated<AnyObject> = msg_send![class!(NSImage), alloc];
            let image: Retained<AnyObject> = msg_send![image, initWithSize: NSSize::new(w as f64, h as f64)];
            let _: () = msg_send![&*image, addRepresentation: &*rep];
            let _: () = msg_send![&*view, setImage: &*image];
        }
    }

    fn pump(&mut self, until: Option<Instant>, fds: &[i32]) -> Pumped {
        // The run loop pops its own pool before `nextEventMatchingMask:` answers, so every event
        // it hands back would leak without one of ours around the whole turn.
        objc2::rc::autoreleasepool(|_| self.pump_pooled(until, fds))
    }
}

impl Platform {
    fn pump_pooled(&mut self, until: Option<Instant>, _fds: &[i32]) -> Pumped {
        let first: Retained<NSDate> = match until {
            Some(t) => NSDate::dateWithTimeIntervalSinceNow(t.saturating_duration_since(Instant::now()).as_secs_f64()),
            None => NSDate::distantFuture(),
        };
        let rest: Retained<NSDate> = NSDate::distantPast();
        let mut date = &first;
        unsafe {
            loop {
                let event: *mut AnyObject = msg_send![
                    &*self.app,
                    nextEventMatchingMask: usize::MAX,
                    untilDate: &**date,
                    inMode: NSDefaultRunLoopMode,
                    dequeue: true
                ];
                if event.is_null() {
                    break;
                }
                let _: () = msg_send![&*self.app, sendEvent: event];
                date = &rest;
            }
            let _: () = msg_send![&*self.app, updateWindows];
        }
        let closed = self
            .windows
            .iter_mut()
            .filter_map(|(id, window, shown)| {
                let visible: bool = unsafe { msg_send![&**window, isVisible] };
                let gone = *shown && !visible;
                *shown = visible;
                gone.then_some(*id)
            })
            .collect();
        Pumped { closed, ready: Vec::new(), dead: false }
    }
}
