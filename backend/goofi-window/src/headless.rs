//! No screen: the suite's window host, as its clock has no device. A window is a number, nothing
//! reaches a desktop, and a descriptor is not watched — that needs a display's poll, and the
//! suite's plugin registers a timer.

use std::ffi::c_void;
use std::sync::Arc;
use std::time::Instant;

use goofi_supervisor::timing::Event;

use super::{Id, Pumped, Screen, Wake};

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

    fn create(&mut self, _title: &str, _size: (u32, u32)) -> Result<(Id, *mut c_void), String> {
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
