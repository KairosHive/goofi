//! Which cpal host a device comes from, and the name a patch stores for it.
//!
//! cpal builds one host per audio API a platform has — ALSA, PipeWire, PulseAudio and JACK on
//! Linux, WASAPI and ASIO on Windows, CoreAudio on macOS — and `available_hosts()` answers the ones
//! this machine is actually running. Every one of them is offered, because which API a card sounds
//! best through is the user's call and not goofi's: WASAPI publishes an interface as one stereo
//! endpoint per pair the driver chose to expose, where ASIO gives the same card as eighteen
//! channels on one device, and on Linux the pulse shim and native PipeWire differ in what they can
//! promise a real-time thread.
//!
//! A device's name CARRIES its host, so choosing a host is choosing a device — there is no second
//! param to keep in step with the first, and no ordering rule for which host wins a shared name.
//!
//! ASIO is the one host with rules of its own, and they are the SDK's rather than goofi's: it loads
//! ONE driver per process, takes seconds to answer, and stops enumerating once a stream holds a
//! driver. [`asio_driver`] and [`seen`] are what those three cost. It is compiled in by default and
//! is inert off Windows, where cpal target-gates it away; what it asks of a Windows build is
//! libclang, which `goofi-init` asks for, and nothing of the user — asio-sys downloads the
//! Steinberg SDK itself. `roadmap/audio-engine.md` carries what that SDK's licence still bars,
//! which is a redistributed binary rather than a build.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

use cpal::traits::{DeviceTrait, HostTrait};

/// The one host that loads a single driver per process.
const ASIO: &str = "ASIO";

/// Input or output, kept as a type because it picks the enumeration as well as wording a refusal.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum Kind {
    Input,
    Output,
}

impl Kind {
    /// The word a refusal is worded with.
    fn word(self) -> &'static str {
        match self {
            Kind::Input => "input",
            Kind::Output => "output",
        }
    }

    /// A host's devices of this kind. A host that refuses to enumerate contributes none rather
    /// than failing the list: every other host's devices are still openable, and a name that is
    /// missing reports itself at open.
    fn all(self, host: &cpal::Host) -> Vec<cpal::Device> {
        match self {
            Kind::Input => host.input_devices().map(|d| d.collect()).unwrap_or_default(),
            Kind::Output => host.output_devices().map(|d| d.collect()).unwrap_or_default(),
        }
    }

    fn default_of(self, host: &cpal::Host) -> Option<cpal::Device> {
        match self {
            Kind::Input => host.default_input_device(),
            Kind::Output => host.default_output_device(),
        }
    }
}

/// The audio hosts this build carries, comma separated — every prefix a device name can have.
/// Asked fresh, as [`named`] asks it: a daemon that starts while goofi runs is a host that appears,
/// and a list kept beside the one the devices come from would be the half that goes stale.
pub fn hosts() -> String {
    cpal::available_hosts().iter().map(|id| id.name()).collect::<Vec<_>>().join(", ")
}

/// What a Windows build that turned ASIO OFF owes the reader. Without it a machine holding an ASIO
/// card lists its WASAPI endpoints — the first stereo pair of each — and nothing at all says why
/// the driver's own view of the same card is absent.
#[cfg(all(windows, not(feature = "asio")))]
pub const NO_ASIO_NOTE: &str =
    " — ASIO is OFF in this build: it is a default feature, so something turned it off; \
`cargo run` with defaults is what brings it back";
#[cfg(not(all(windows, not(feature = "asio"))))]
pub const NO_ASIO_NOTE: &str = "";

/// The name a patch stores for one device of one host.
fn qualified(host: cpal::HostId, device: &str) -> String {
    format!("{}: {device}", host.name())
}

/// The host and the device a stored name is made of. `HostId` parses case-insensitively off the
/// same spelling [`qualified`] writes, so a name round-trips; a host this build has no support for
/// does not parse, and its devices are correctly unreachable.
fn split(name: &str) -> Option<(cpal::HostId, &str)> {
    let (host, device) = name.split_once(": ")?;
    Some((host.parse().ok()?, device))
}

/// The ASIO driver `name` asks for, or `None` for every other host.
///
/// Read off the NAME rather than the parsed host, so a patch written on a machine with ASIO still
/// says so on one without it. A driver loads once per process and a second asks answers
/// `DriverAlreadyExists` rather than degrading, so a patch may name many devices but only ever one
/// ASIO driver, and [`crate::AudioEngine`] refuses the rest by this.
pub(crate) fn asio_driver(name: &str) -> Option<&str> {
    let (host, device) = name.split_once(": ")?;
    host.eq_ignore_ascii_case(ASIO).then_some(device)
}

/// Every ASIO device seen while its driver could still be loaded, by the name a patch stores.
///
/// Enumerating ASIO stops at the FIRST driver that is not the one already loaded — cpal returns
/// `None` there rather than spinning through the rest — so once any stream holds a driver the list
/// is empty, and even that driver's own device cannot be found again. An input opened after an
/// output would therefore fail with `no input device`, naming the very device that is playing.
///
/// A `Device` is a handle and not a session: it stays valid while its driver is loaded, and one
/// ASIO device serves input and output alike. So the handle seen before the stream opened is the
/// one to reuse, and this is where it is kept. Only ASIO is remembered — a hot-pluggable device
/// that is gone must report itself gone, not answer from here.
///
/// Keyed by KIND as well as name, because the two directions of one card are not the same device
/// to cpal and a driver need not offer both: a name remembered as an output must not be offered as
/// an input on the strength of having been seen at all.
fn seen() -> &'static Mutex<HashMap<(Kind, String), cpal::Device>> {
    static SEEN: OnceLock<Mutex<HashMap<(Kind, String), cpal::Device>>> = OnceLock::new();
    SEEN.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Remember `list`'s ASIO devices for `kind`. Called for BOTH directions whenever either is
/// enumerated, so the first dropdown a user opens is enough to make the other direction reachable
/// once a stream holds the driver — the case that had an ASIO output leave `AudioIn` all WASAPI.
fn remember(kind: Kind, list: &[(String, cpal::Device)]) {
    if let Ok(mut seen) = seen().lock() {
        seen.extend(
            list.iter().filter(|(n, _)| asio_driver(n).is_some()).map(|(n, d)| ((kind, n.clone()), d.clone())),
        );
    }
}

/// The remembered ASIO devices for `kind`, by stored name.
fn remembered(kind: Kind) -> Vec<(String, cpal::Device)> {
    seen()
        .lock()
        .map(|s| s.iter().filter(|((k, _), _)| *k == kind).map(|((_, n), d)| (n.clone(), d.clone())).collect())
        .unwrap_or_default()
}

/// Learn every ASIO device, in BOTH directions, ONCE per process and as early as possible.
///
/// The cache above can only ever be filled while no stream holds a driver, and by the time a list
/// is asked for that may already be false: a patch loaded with an ASIO `AudioIn` in it opens the
/// capture stream before any dropdown exists, and from then on the OUTPUT list has no ASIO in it
/// and no way to get any — the driver that is recording cannot be enumerated, and it was never
/// seen while it could be. That is the same defect as the one `seen` was written for, reached from
/// the other end, and filling the cache lazily at each list cannot close it.
///
/// So both directions are learned at the FIRST audio interaction of the process, whether that is a
/// dropdown or a device being resolved to open — resolving is what every stream does first, so
/// this runs while the driver is still free. Once, because a handle does not go stale while its
/// driver is loadable and enumerating ASIO loads every driver on the machine and costs seconds.
///
/// A driver another APPLICATION already holds at that moment is genuinely unavailable, and no
/// bookkeeping here can make it otherwise.
///
/// It runs at ENGINE CONSTRUCTION and nowhere else. The obvious place — resolving a name, just
/// before the stream opens — is the latest possible moment and also a fatal one: it enumerates
/// ASIO on the clock thread, which loads every ASIO driver on the machine into a process that is
/// at that moment instantiating VST3 plugins, and this machine died with STATUS_HEAP_CORRUPTION
/// on a patch holding two of them. Engine construction is before any plugin is instantiated,
/// before any stream exists, and on the thread that builds the engine.
fn warm(walk: &dyn Fn(Kind, Option<cpal::HostId>) -> Vec<(String, cpal::Device)>) {
    static WARMED: OnceLock<()> = OnceLock::new();
    if WARMED.get().is_some() {
        return;
    }
    if let Some(asio) = cpal::available_hosts().into_iter().find(|id| id.name().eq_ignore_ascii_case(ASIO)) {
        for kind in [Kind::Input, Kind::Output] {
            remember(kind, &walk(kind, Some(asio)));
        }
    }
    let _ = WARMED.set(());
}

fn name_of(d: &cpal::Device) -> Option<String> {
    d.description().ok().map(|d| d.name().to_string())
}

/// Learn the ASIO devices once, at engine construction — see [`warm`] for why it is there and
/// nowhere later.
pub(crate) fn prewarm() {
    warm(&|kind, only| {
        let mut out = Vec::new();
        for id in cpal::available_hosts().iter().copied().filter(|id| only.is_none_or(|o| o == *id)) {
            let Ok(host) = cpal::host_from_id(id) else { continue };
            out.extend(kind.all(&host).into_iter().filter_map(|d| name_of(&d).map(|n| (qualified(id, &n), d))));
        }
        out
    });
}

/// Every device goofi offers for `kind`, under the name a patch stores. The walk is
/// `available_hosts()`, so the list is exactly what this machine runs.
///
/// Enumerating ASIO briefly LOADS each driver to read its metadata, and a driver another stream
/// holds open cannot be loaded again — so this list is shorter while an ASIO stream runs than it is
/// while the engine is idle. That is the SDK's one-driver rule seen from the other end, and nothing
/// here can widen it.
pub(crate) fn named(kind: Kind) -> Vec<(String, cpal::Device)> {
    // `only` narrows the walk to one host. Enumerating ASIO briefly LOADS every driver on the
    // machine and costs seconds; the other direction is wanted for ASIO alone, and walking every
    // host again to get it would put that cost on a dropdown twice over.
    let walk = |kind: Kind, only: Option<cpal::HostId>| {
        let mut out: Vec<(String, cpal::Device)> = Vec::new();
        let mut taken = std::collections::HashSet::new();
        for id in cpal::available_hosts().iter().copied().filter(|id| only.is_none_or(|o| o == *id)) {
            let Ok(host) = cpal::host_from_id(id) else { continue };
            for device in kind.all(&host) {
                // A host may list one card many times under one name — ALSA lists this machine's
                // eight times — and a name resolves to the first, so the rest are entries nobody
                // can choose.
                let Some(name) = name_of(&device).map(|n| qualified(id, &n)).filter(|n| taken.insert(n.clone()))
                else {
                    continue;
                };
                out.push((name, device));
            }
        }
        out
    };

    warm(&walk);
    let mut out = walk(kind, None);
    remember(kind, &out);

    // What the walk could not see this time. Once a stream holds an ASIO driver the host answers
    // with nothing, and the same card that is playing would otherwise vanish from the list a user
    // picks its capture side from. A remembered handle is the same device, so it is offered again.
    let live: std::collections::HashSet<String> = out.iter().map(|(n, _)| n.clone()).collect();
    let mut extra: Vec<(String, cpal::Device)> =
        remembered(kind).into_iter().filter(|(n, _)| !live.contains(n)).collect();
    extra.sort_by(|a, b| a.0.cmp(&b.0));
    out.extend(extra);
    out
}

/// The device `name` names, `default` being the platform host's own default. The host in a name is
/// a CHOICE of host, so it is resolved in that host alone: several hosts see one card, and which of
/// them a patch meant is the whole of what the name records.
pub(crate) fn device(kind: Kind, name: &str) -> Result<cpal::Device, String> {
    if name == crate::DEFAULT_DEVICE {
        return kind.default_of(&cpal::default_host()).ok_or_else(|| format!("no default {} device", kind.word()));
    }
    if let Some((id, wanted)) = split(name) {
        if let Ok(host) = cpal::host_from_id(id) {
            if let Some(device) = kind.all(&host).into_iter().find(|d| name_of(d).as_deref() == Some(wanted)) {
                return Ok(device);
            }
        }
    }
    // Not in the list, and for an ASIO name that is the expected answer once a stream holds the
    // driver — see [`seen`]. A device remembered from before is the same device.
    if asio_driver(name).is_some() {
        if let Some(device) = seen().lock().ok().and_then(|s| s.get(&(kind, name.to_string())).cloned()) {
            return Ok(device);
        }
    }
    Err(format!("no {} device `{name}`", kind.word()))
}

#[cfg(test)]
mod tests {
    use super::{asio_driver, named, qualified, split, Kind};

    /// The bug this file's [`seen`] exists for, end to end and on real hardware: an ASIO OUTPUT
    /// holding the driver must not empty the INPUT list. Before the cache was consulted at
    /// enumeration the input list fell back to WASAPI alone, and the card that was playing could
    /// not be chosen to capture from.
    ///
    /// Ignored because it needs an ASIO card: `cargo test -p goofi-audio --features asio
    /// asio_input_survives -- --ignored --nocapture`.
    #[test]
    #[ignore = "needs an ASIO device"]
    fn asio_input_survives_an_output_holding_the_driver() {
        use cpal::traits::{DeviceTrait, StreamTrait};

        let ins = named(Kind::Input);
        let asio_in: Vec<String> = ins.iter().map(|(n, _)| n.clone()).filter(|n| asio_driver(n).is_some()).collect();
        assert!(!asio_in.is_empty(), "no ASIO input to test with");
        println!("idle ASIO inputs: {asio_in:#?}");

        // Hold one ASIO driver open on the OUTPUT side, as an `AudioOut` does. Each driver speaks
        // its own word — a Focusrite's is `i32`, the DirectX shim's `i16` — so the format is read
        // off the device rather than assumed, exactly as `open_output` reads it.
        let outs = named(Kind::Output);
        let held = outs
            .iter()
            .filter(|(n, _)| asio_driver(n).is_some())
            .find_map(|(name, dev)| {
                let supported = dev.default_output_config().ok()?;
                let cfg = supported.config();
                let silence = |e| eprintln!("{e}");
                let stream = match supported.sample_format() {
                    cpal::SampleFormat::I16 => {
                        dev.build_output_stream::<i16, _, _>(cfg, |d: &mut [i16], _| d.fill(0), silence, None)
                    }
                    cpal::SampleFormat::I32 => {
                        dev.build_output_stream::<i32, _, _>(cfg, |d: &mut [i32], _| d.fill(0), silence, None)
                    }
                    cpal::SampleFormat::F32 => {
                        dev.build_output_stream::<f32, _, _>(cfg, |d: &mut [f32], _| d.fill(0.0), silence, None)
                    }
                    f => {
                        eprintln!("`{name}`: skipping, sample format {f}");
                        return None;
                    }
                };
                match stream {
                    Ok(s) => s.play().ok().map(|()| (name.clone(), s)),
                    Err(e) => {
                        eprintln!("`{name}`: {e}");
                        None
                    }
                }
            });
        let (out_name, _stream) = held.expect("no ASIO output could be opened to hold a driver with");
        println!("holding `{out_name}`");

        let after: Vec<String> =
            named(Kind::Input).into_iter().map(|(n, _)| n).filter(|n| asio_driver(n).is_some()).collect();
        println!("ASIO inputs while it plays: {after:#?}");
        for name in &asio_in {
            assert!(after.contains(name), "`{name}` vanished from the input list while an ASIO output held the driver");
        }
    }

    /// The host is the whole of the choice, so reading it back must be exact: a device merely
    /// CALLED something ASIO-ish belongs to the host that listed it and stays there.
    #[test]
    fn a_driver_is_read_back_from_the_host_alone() {
        assert_eq!(asio_driver("ASIO: Focusrite USB ASIO"), Some("Focusrite USB ASIO"));
        assert_eq!(asio_driver("WASAPI: Analogue 1 + 2 (Focusrite Usb Audio)"), None);
        // Named for the standard, not listed by it: a WASAPI endpoint, and no driver claim.
        assert_eq!(asio_driver("WASAPI: Generic Low Latency ASIO Driver"), None);
        assert_eq!(asio_driver("default"), None);
    }

    /// Whatever hosts this build has, a name written for one is read back as that one — the
    /// contract every stored patch depends on.
    #[test]
    fn a_stored_name_round_trips_through_its_host() {
        for id in cpal::available_hosts() {
            let name = qualified(id, "Analogue 1 + 2");
            assert_eq!(split(&name), Some((id, "Analogue 1 + 2")), "`{name}` did not read back");
        }
    }
}
