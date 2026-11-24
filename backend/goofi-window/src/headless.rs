//! No screen: the suite's window host, as its clock has no device. A window is a number, nothing
//! reaches a desktop, and a descriptor is not watched — that needs a display's poll, and the
//! suite's plugin registers a timer.

use std::ffi::c_void;
use std::sync::Arc;
use std::time::Instant;

use goofi_supervisor::timing::Event;

use super::{Id, Monitor, Pumped, Screen, Wake};

#[derive(Default)]
pub struct Platform {
    wake: Arc<Signal>,
    next: Id,
}

/// The pump's door: a wake rings it, and the pump parks on it until its deadline.
#[derive(Default)]
struct Signal(Event);

impl Wake for Signal {
    fn wake(&self) {
        self.0.ring(0);
    }
}

impl Screen for Platform {
    fn waker(&self) -> Arc<dyn Wake> {
        self.wake.clone()
    }

    /// One monitor, so a placement has a screen to name and a fullscreen window a size to take.
    fn monitors(&self) -> Vec<Monitor> {
        vec![Monitor { at: (0, 0), size: (1280, 720) }]
    }

    fn create(&mut self, _title: &str, _size: (u32, u32), _on: Option<Monitor>, _cover: bool) -> Result<(Id, *mut c_void), String> {
        self.next += 1;
        Ok((self.next, self.next as usize as *mut c_void))
    }

    fn resize(&mut self, _id: Id, _size: (u32, u32)) {}

    fn destroy(&mut self, _id: Id) {}

    fn pump(&mut self, until: Option<Instant>, _fds: &[i32]) -> Pumped {
        match until {
            Some(until) => self.wake.0.wait_until(until),
            None => self.wake.0.wait(),
        };
        Pumped { closed: Vec::new(), ready: Vec::new(), dead: false }
    }
}
