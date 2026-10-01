//! A mutex that outlives a panic under its guard: the lock is taken over and the poison is
//! logged once, by the type it guards. It hands out std's guard, so a `Condvar` waits on it.

use std::sync::atomic::{AtomicBool, Ordering};
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
