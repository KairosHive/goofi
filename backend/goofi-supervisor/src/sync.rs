//! A mutex that outlives a panic under its guard: the lock is taken over and the poison is
//! logged once, by the type it guards. It hands out std's guard, so a `Condvar` waits on it.

use std::sync::atomic::{AtomicBool, AtomicPtr, Ordering};
use std::sync::{MutexGuard, PoisonError, TryLockError};

use crate::log::{self, Level};

pub struct Mutex<T: ?Sized> {
    poisoned: AtomicBool,
    inner: std::sync::Mutex<T>,
}

impl<T> Mutex<T> {
    pub const fn new(value: T) -> Self {
        Mutex { poisoned: AtomicBool::new(false), inner: std::sync::Mutex::new(value) }
    }

    pub fn into_inner(self) -> T {
        self.inner.into_inner().unwrap_or_else(PoisonError::into_inner)
    }
}

impl<T: ?Sized> Mutex<T> {
    pub fn lock(&self) -> MutexGuard<'_, T> {
        self.inner.lock().unwrap_or_else(|e| self.taken_over(e))
    }

    /// The guard now, or `None` while another thread holds it.
    pub fn try_lock(&self) -> Option<MutexGuard<'_, T>> {
        match self.inner.try_lock() {
            Ok(guard) => Some(guard),
            Err(TryLockError::Poisoned(e)) => Some(self.taken_over(e)),
            Err(TryLockError::WouldBlock) => None,
        }
    }

    pub fn get_mut(&mut self) -> &mut T {
        self.inner.get_mut().unwrap_or_else(PoisonError::into_inner)
    }

    fn taken_over<'a>(&self, e: PoisonError<MutexGuard<'a, T>>) -> MutexGuard<'a, T> {
        if !self.poisoned.swap(true, Ordering::Relaxed) {
            let name = std::any::type_name::<T>();
            log::record(log::source(), Level::Warning, None, format!("a thread panicked holding the {name} lock; later holders continue"));
        }
        e.into_inner()
    }
}

impl<T: Default> Default for Mutex<T> {
    fn default() -> Self { Mutex::new(T::default()) }
}

impl<T> From<T> for Mutex<T> {
    fn from(value: T) -> Self { Mutex::new(value) }
}

impl<T: ?Sized + std::fmt::Debug> std::fmt::Debug for Mutex<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result { self.inner.fmt(f) }
}

/// std's condition variable with the poison folded away, as [`Mutex`] folds it.
#[derive(Default, Debug)]
pub struct Condvar(std::sync::Condvar);

impl Condvar {
    pub const fn new() -> Self { Condvar(std::sync::Condvar::new()) }
    pub fn notify_one(&self) { self.0.notify_one() }
    pub fn notify_all(&self) { self.0.notify_all() }

    pub fn wait<'a, T>(&self, guard: MutexGuard<'a, T>) -> MutexGuard<'a, T> {
        self.0.wait(guard).unwrap_or_else(PoisonError::into_inner)
    }

    pub fn wait_while<'a, T>(&self, guard: MutexGuard<'a, T>, condition: impl FnMut(&mut T) -> bool) -> MutexGuard<'a, T> {
        self.0.wait_while(guard, condition).unwrap_or_else(PoisonError::into_inner)
    }

    pub fn wait_timeout<'a, T>(&self, guard: MutexGuard<'a, T>, dur: std::time::Duration) -> MutexGuard<'a, T> {
        self.0.wait_timeout(guard, dur).unwrap_or_else(PoisonError::into_inner).0
    }

    pub fn wait_timeout_while<'a, T>(&self, guard: MutexGuard<'a, T>, dur: std::time::Duration, condition: impl FnMut(&mut T) -> bool) -> MutexGuard<'a, T> {
        self.0.wait_timeout_while(guard, dur, condition).unwrap_or_else(PoisonError::into_inner).0
    }
}

/// A flag set once, which any number of threads can wait on until a deadline.
#[derive(Default, Debug)]
pub struct Latch {
    set: Mutex<bool>,
    cv: Condvar,
}

impl Latch {
    pub fn open(&self) {
        *self.set.lock() = true;
        self.cv.notify_all();
    }

    pub fn is_open(&self) -> bool {
        *self.set.lock()
    }

    /// Park until open or `deadline`; whether it is open.
    pub fn wait_until(&self, deadline: std::time::Instant) -> bool {
        let left = deadline.saturating_duration_since(std::time::Instant::now());
        *self.cv.wait_timeout_while(self.set.lock(), left, |set| !*set)
    }
}

/// A latest-wins cell between threads, with no lock on either side: one atomic swap puts, one
/// takes. The writer gets back what it displaced, so a value nobody took in time is freed on the
/// writer's thread — and a value is boxed once by whoever makes it, never again on the way.
///
/// This is how a producer hands its reader what it made: the reader takes it on its own time, and
/// a reader that is slow or gone costs the producer one swap, never a wait.
pub struct Mailbox<T> {
    slot: AtomicPtr<T>,
}

// SAFETY: the cell holds a `Box<T>` by raw pointer and hands it out whole, so it moves a `T`
// between threads exactly as a channel would.
unsafe impl<T: Send> Send for Mailbox<T> {}
unsafe impl<T: Send> Sync for Mailbox<T> {}

impl<T> Default for Mailbox<T> {
    fn default() -> Self {
        Mailbox::new()
    }
}

impl<T> Mailbox<T> {
    pub const fn new() -> Self {
        Mailbox { slot: AtomicPtr::new(std::ptr::null_mut()) }
    }

    /// Put `value` in, and get back what it displaced: the value the reader did not take.
    pub fn put(&self, value: Box<T>) -> Option<Box<T>> {
        let old = self.slot.swap(Box::into_raw(value), Ordering::AcqRel);
        // SAFETY: a non-null pointer here came from `Box::into_raw` in a `put`, and the swap took it out.
        (!old.is_null()).then(|| unsafe { Box::from_raw(old) })
    }

    /// Take what is in, leaving the cell empty.
    pub fn take(&self) -> Option<Box<T>> {
        let held = self.slot.swap(std::ptr::null_mut(), Ordering::AcqRel);
        // SAFETY: as in `put`.
        (!held.is_null()).then(|| unsafe { Box::from_raw(held) })
    }

    pub fn is_empty(&self) -> bool {
        self.slot.load(Ordering::Acquire).is_null()
    }
}

impl<T> Drop for Mailbox<T> {
    fn drop(&mut self) {
        drop(self.take());
    }
}

/// A pile anything may push onto and one thread takes whole, with no lock on either side: a push
/// is one compare-and-swap, a drain is one swap. What is drained comes out newest first.
///
/// This is how a reader gives a producer's buffers back: the producer takes them all at the top
/// of its tick, whatever threads pushed them.
pub struct Stack<T> {
    head: AtomicPtr<Node<T>>,
}

struct Node<T> {
    value: T,
    next: *mut Node<T>,
}

// SAFETY: nodes are boxed `T`s handed between threads whole, as a channel hands them.
unsafe impl<T: Send> Send for Stack<T> {}
unsafe impl<T: Send> Sync for Stack<T> {}

impl<T> Default for Stack<T> {
    fn default() -> Self {
        Stack::new()
    }
}

impl<T> Stack<T> {
    pub const fn new() -> Self {
        Stack { head: AtomicPtr::new(std::ptr::null_mut()) }
    }

    pub fn push(&self, value: T) {
        let node = Box::into_raw(Box::new(Node { value, next: std::ptr::null_mut() }));
        let mut head = self.head.load(Ordering::Acquire);
        loop {
            // SAFETY: `node` is ours until the exchange below publishes it.
            unsafe { (*node).next = head };
            match self.head.compare_exchange_weak(head, node, Ordering::AcqRel, Ordering::Acquire) {
                Ok(_) => return,
                Err(seen) => head = seen,
            }
        }
    }

    pub fn is_empty(&self) -> bool {
        self.head.load(Ordering::Acquire).is_null()
    }

    /// Everything pushed so far, taken in one swap; newest first.
    pub fn drain(&self) -> Drain<T> {
        Drain { next: self.head.swap(std::ptr::null_mut(), Ordering::AcqRel) }
    }
}

impl<T> Drop for Stack<T> {
    fn drop(&mut self) {
        drop(self.drain());
    }
}

/// What one [`Stack::drain`] took.
pub struct Drain<T> {
    next: *mut Node<T>,
}

// SAFETY: a drained chain is owned by whoever holds the `Drain`.
unsafe impl<T: Send> Send for Drain<T> {}

impl<T> Iterator for Drain<T> {
    type Item = T;

    fn next(&mut self) -> Option<T> {
        if self.next.is_null() {
            return None;
        }
        // SAFETY: every node in the chain came from `Box::into_raw` in `push`, and the swap that
        // made this drain is the one owner of the chain.
        let node = unsafe { Box::from_raw(self.next) };
        self.next = node.next;
        Some(node.value)
    }
}

impl<T> Drop for Drain<T> {
    fn drop(&mut self) {
        for _ in self.by_ref() {}
    }
}
