//! The render threads' grant: real-time priority above a plugin's own threads, and a seat on the
//! performance cores of a hybrid CPU. A clock thread the scheduler parks on an efficiency core
//! renders a heavy plugin late in every period; a thread outranked by the plugin's GUI threads
//! loses its deadline to a repaint.

/// The `SCHED_RR` priority the clock thread and its helpers ask for directly, when the user's
/// limits allow it: above a plugin's own real-time threads (Vital's run at 50 and 69), below the
/// sound server's (PipeWire's data thread runs at 88). The rtkit fallback grants its own.
#[cfg(target_os = "linux")]
const PRIORITY: libc::c_int = 80;

/// The callback thread's real-time grant, asked for on its first run: cpal asks only under ALSA
/// and WASAPI, and a callback left at ordinary priority loses its deadline to any busy thread.
pub(crate) enum Realtime {
    Unasked,
    /// Granted or refused once: the grant outlives its handle, and a refusal is logged once.
    Asked,
}

impl Realtime {
    /// Promote and seat the calling thread once, for `frames` per callback at `rate`; a refusal is
    /// logged once and the stream plays on at ordinary priority, which is the deadline lost.
    pub(crate) fn claim(&mut self, frames: usize, rate: u32) {
        if matches!(self, Realtime::Asked) {
            return;
        }
        promote(frames as u32, rate);
        *self = Realtime::Asked;
    }
}

/// Promote the calling thread to real time: directly where the limits allow, else through rtkit.
pub(crate) fn promote(frames: u32, rate: u32) {
    seat();
    #[cfg(target_os = "linux")]
    {
        let param = libc::sched_param { sched_priority: PRIORITY };
        // SAFETY: a plain syscall on the calling thread with a valid param.
        if unsafe { libc::sched_setscheduler(0, libc::SCHED_RR | libc::SCHED_RESET_ON_FORK, &param) } == 0 {
            return;
        }
    }
    if let Err(e) = audio_thread_priority::promote_current_thread_to_real_time(frames, rate) {
        goofi_supervisor::log::record(goofi_supervisor::log::Source::component("audio"), goofi_supervisor::log::Level::Error, None, format!("audio: a render thread was refused real-time priority: {e}"));
    }
}

/// Seat the calling thread on the performance cores, where the machine has a slower kind too.
pub(crate) fn seat() {
    #[cfg(target_os = "linux")]
    if let Some(cores) = performance_cores() {
        // SAFETY: a zeroed cpu_set_t is the empty set, and every index is below CPU_SETSIZE.
        unsafe {
            let mut set: libc::cpu_set_t = std::mem::zeroed();
            for &c in &cores {
                if c < libc::CPU_SETSIZE as usize {
                    libc::CPU_SET(c, &mut set);
                }
            }
            libc::sched_setaffinity(0, std::mem::size_of::<libc::cpu_set_t>(), &set);
        }
    }
}

/// The CPUs a render thread is seated on: the performance cores of a hybrid machine, in the
/// kernel's own list spelling (`0-15`), or `None` where every core is the same.
pub fn performance_cores() -> Option<Vec<usize>> {
    #[cfg(target_os = "linux")]
    {
        // Intel names its core kinds; an Arm big.LITTLE machine states each CPU's capacity.
        if let Ok(list) = std::fs::read_to_string("/sys/devices/cpu_core/cpus") {
            return Some(parse_cpulist(list.trim())).filter(|c| !c.is_empty());
        }
        let n = std::thread::available_parallelism().map_or(0, |n| n.get());
        let capacity: Vec<(usize, u64)> = (0..n)
            .filter_map(|c| {
                let text = std::fs::read_to_string(format!("/sys/devices/system/cpu/cpu{c}/cpu_capacity")).ok()?;
                Some((c, text.trim().parse().ok()?))
            })
            .collect();
        let most = capacity.iter().map(|(_, cap)| *cap).max()?;
        if capacity.iter().all(|(_, cap)| *cap == most) {
            return None;
        }
        return Some(capacity.into_iter().filter(|(_, cap)| *cap == most).map(|(c, _)| c).collect());
    }
    #[allow(unreachable_code)]
    None
}

/// A kernel CPU list — `0-3,8,10-11` — as the CPUs it names.
#[cfg(target_os = "linux")]
fn parse_cpulist(list: &str) -> Vec<usize> {
    list.split(',')
        .filter(|part| !part.is_empty())
        .flat_map(|part| match part.split_once('-') {
            Some((lo, hi)) => {
                let (lo, hi) = (lo.trim().parse().unwrap_or(0), hi.trim().parse().unwrap_or(0));
                (lo..=hi).collect::<Vec<usize>>()
            }
            None => part.trim().parse().ok().into_iter().collect(),
        })
        .collect()
}

/// The seat as `session status` names it: the kernel's list spelling of [`performance_cores`].
pub(crate) fn seat_name() -> Option<String> {
    let cores = performance_cores()?;
    let mut out = Vec::new();
    let mut i = 0;
    while i < cores.len() {
        let start = cores[i];
        while i + 1 < cores.len() && cores[i + 1] == cores[i] + 1 {
            i += 1;
        }
        out.push(if cores[i] == start { start.to_string() } else { format!("{start}-{}", cores[i]) });
        i += 1;
    }
    Some(out.join(","))
}
