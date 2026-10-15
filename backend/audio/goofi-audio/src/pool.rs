//! The audio thread's helpers: threads that run the stages of one plan level beside it, so nodes
//! that do not feed each other render on several cores within one block. Nothing here allocates
//! or locks while a block renders; a helper waits on its job word, spinning briefly, then parked.

use std::cell::UnsafeCell;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// The most helpers a pool starts, whatever the machine has.
const MOST: usize = 7;
/// How long an idle helper spins before it parks: a wake from a park costs tens of microseconds.
const SPIN: Duration = Duration::from_micros(200);

/// One level's items, run by index on whichever thread claims each.
pub trait Work: Sync {
    fn run(&self, item: usize);
}

struct Shared {
    /// The job's generation in the high half, the next unclaimed item in the low: a claim made
    /// against an earlier job's word fails, so a helper late from one job never runs the next.
    next: AtomicU64,
    count: AtomicUsize,
    done: AtomicUsize,
    /// Written only while no helper can claim an item: before the generation that publishes it,
    /// and after every item of it is done.
    job: UnsafeCell<Option<*const (dyn Work + 'static)>>,
    stop: AtomicBool,
    parked: Vec<AtomicBool>,
    /// Frames in the high half and rate in the low, once the clock's thread is real-time.
    realtime: AtomicU64,
    panicked: AtomicBool,
    /// How long the clock thread has waited for the helpers' items after its own, in all and at
    /// worst: what a helper late to its seat costs the block.
    pub waited_ns: AtomicU64,
    pub waited_max_ns: AtomicU64,
}

// SAFETY: `job` is the one non-Sync field, and the generation protocol above orders every access.
unsafe impl Sync for Shared {}
unsafe impl Send for Shared {}

impl Shared {
    /// Claim and run items of generation `generation` until none is left.
    fn drain(&self, generation: u32) {
        loop {
            let word = self.next.load(Ordering::Acquire);
            let item = (word & u64::from(u32::MAX)) as usize;
            if (word >> 32) as u32 != generation || item >= self.count.load(Ordering::Acquire) {
                return;
            }
            if self.next.compare_exchange_weak(word, word + 1, Ordering::AcqRel, Ordering::Acquire).is_err() {
                continue;
            }
            // SAFETY: the claim succeeded under this generation, so its job is published and
            // stays alive until `done` reaches `count`, which this item has not yet added to.
            let job = unsafe { (*self.job.get()).expect("a published job") };
            if catch_unwind(AssertUnwindSafe(|| unsafe { (*job).run(item) })).is_err() {
                self.panicked.store(true, Ordering::Relaxed);
            }
            self.done.fetch_add(1, Ordering::Release);
        }
    }
}

pub struct Pool {
    shared: Arc<Shared>,
    threads: Vec<std::thread::Thread>,
    workers: Vec<goofi_supervisor::worker::Worker>,
    generation: u32,
}

impl Pool {
    /// A pool of one helper per core beyond the caller's, at most [`MOST`].
    pub fn new() -> Pool {
        let helpers = std::thread::available_parallelism().map_or(0, |n| n.get().saturating_sub(1)).min(MOST);
        let shared = Arc::new(Shared {
            next: AtomicU64::new(0),
            count: AtomicUsize::new(0),
            done: AtomicUsize::new(0),
            job: UnsafeCell::new(None),
            stop: AtomicBool::new(false),
            parked: (0..helpers).map(|_| AtomicBool::new(false)).collect(),
            realtime: AtomicU64::new(0),
            panicked: AtomicBool::new(false),
            waited_ns: AtomicU64::new(0),
            waited_max_ns: AtomicU64::new(0),
        });
        let mut workers = Vec::new();
        for k in 0..helpers {
            let shared = shared.clone();
            match goofi_supervisor::worker::spawn(format!("goofi-audio-pool-{k}"), move || help(&shared, k)) {
                Ok(w) => workers.push(w),
                Err(e) => goofi_supervisor::log::record(goofi_supervisor::log::Source::component("audio"), goofi_supervisor::log::Level::Error, None, format!("audio: a render helper did not start: {e}")),
            }
        }
        let threads = workers.iter().map(|w| w.thread().clone()).collect();
        Pool { shared, threads, workers, generation: 0 }
    }

    /// What the clock thread has waited on the helpers so far: in all, and the longest once.
    pub fn waited(&self) -> (Duration, Duration) {
        (
            Duration::from_nanos(self.shared.waited_ns.load(Ordering::Relaxed)),
            Duration::from_nanos(self.shared.waited_max_ns.load(Ordering::Relaxed)),
        )
    }

    /// Ask every helper to take the clock thread's real-time grant, for `frames` at `rate`.
    pub fn realtime(&self, frames: u32, rate: u32) {
        self.shared.realtime.store((u64::from(frames) << 32) | u64::from(rate.max(1)), Ordering::Release);
    }

    /// Run `count` items of `work`, this thread among the helpers, and return once all are done.
    pub fn run(&mut self, work: &dyn Work, count: usize) {
        if self.workers.is_empty() || count < 2 {
            (0..count).for_each(|i| work.run(i));
            return;
        }
        let shared = &*self.shared;
        // SAFETY: no item of the previous generation is unfinished, so no helper reads `job`; the
        // lifetime is erased for as long as this call, which waits for every item.
        unsafe { *shared.job.get() = Some(std::mem::transmute::<*const dyn Work, *const (dyn Work + 'static)>(work)) };
        shared.count.store(count, Ordering::Relaxed);
        shared.done.store(0, Ordering::Relaxed);
        self.generation = self.generation.wrapping_add(1);
        shared.next.store(u64::from(self.generation) << 32, Ordering::SeqCst);
        for (k, thread) in self.threads.iter().enumerate().take(count - 1) {
            if shared.parked[k].load(Ordering::SeqCst) {
                thread.unpark();
            }
        }
        shared.drain(self.generation);
        let waited = Instant::now();
        while shared.done.load(Ordering::Acquire) < count {
            std::hint::spin_loop();
        }
        let ns = waited.elapsed().as_nanos() as u64;
        shared.waited_ns.fetch_add(ns, Ordering::Relaxed);
        shared.waited_max_ns.fetch_max(ns, Ordering::Relaxed);
        // SAFETY: every item is done, and a late helper's claim fails on the finished word.
        unsafe { *shared.job.get() = None };
        if shared.panicked.swap(false, Ordering::Relaxed) {
            panic!("a render helper panicked");
        }
    }
}

impl Drop for Pool {
    fn drop(&mut self) {
        self.shared.stop.store(true, Ordering::SeqCst);
        self.threads.iter().for_each(std::thread::Thread::unpark);
        for w in self.workers.drain(..) {
            let _ = w.join_within(Duration::from_secs(1));
        }
    }
}

/// A helper's life: wait for a generation it has not seen, run what it can claim of it. Helper `k`
/// joins only a level of more than `k + 1` items, so one the patch never needs stays parked.
fn help(shared: &Shared, k: usize) {
    crate::rt::seat();
    let (mut seen, mut promoted, mut idle) = (0u32, false, Instant::now());
    loop {
        if shared.stop.load(Ordering::SeqCst) {
            return;
        }
        let generation = (shared.next.load(Ordering::Acquire) >> 32) as u32;
        if generation != seen {
            seen = generation;
            if k + 1 < shared.count.load(Ordering::Acquire) {
                let realtime = shared.realtime.load(Ordering::Acquire);
                if !promoted && realtime != 0 {
                    promoted = true;
                    crate::rt::promote((realtime >> 32) as u32, realtime as u32);
                }
                shared.drain(generation);
                idle = Instant::now();
            }
            continue;
        }
        if idle.elapsed() < SPIN {
            std::hint::spin_loop();
            continue;
        }
        shared.parked[k].store(true, Ordering::SeqCst);
        if (shared.next.load(Ordering::SeqCst) >> 32) as u32 == seen && !shared.stop.load(Ordering::SeqCst) {
            std::thread::park();
        }
        shared.parked[k].store(false, Ordering::SeqCst);
        idle = Instant::now();
    }
}
