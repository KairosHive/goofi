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
//! driver. [`asio_driver`] and [`seen`] are what those three cost. It is also a build the user
//! makes and never one that ships — the Steinberg SDK went GPLv3-or-proprietary in 2025 and cannot
//! travel inside this binary, which `roadmap/audio-engine.md` carries — so `--features asio`, with
//! `CPAL_ASIO_DIR` naming the unpacked SDK, is the whole of the opt-in.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

use cpal::traits::{DeviceTrait, HostTrait};

/// The one host that loads a single driver per process.
const ASIO: &str = "ASIO";

/// Input or output, kept as a type because it picks the enumeration as well as wording a refusal.
#[derive(Clone, Copy)]
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
fn seen() -> &'static Mutex<HashMap<String, cpal::Device>> {
    static SEEN: OnceLock<Mutex<HashMap<String, cpal::Device>>> = OnceLock::new();
    SEEN.get_or_init(|| Mutex::new(HashMap::new()))
}

fn name_of(d: &cpal::Device) -> Option<String> {
    d.description().ok().map(|d| d.name().to_string())
}

/// Every device goofi offers for `kind`, under the name a patch stores. The walk is
/// `available_hosts()`, so the list is exactly what this machine runs.
///
/// Enumerating ASIO briefly LOADS each driver to read its metadata, and a driver another stream
/// holds open cannot be loaded again — so this list is shorter while an ASIO stream runs than it is
/// while the engine is idle. That is the SDK's one-driver rule seen from the other end, and nothing
/// here can widen it.
pub(crate) fn named(kind: Kind) -> Vec<(String, cpal::Device)> {
    let mut out: Vec<(String, cpal::Device)> = Vec::new();
    let mut taken = std::collections::HashSet::new();
    for id in cpal::available_hosts() {
        let Ok(host) = cpal::host_from_id(id) else { continue };
        for device in kind.all(&host) {
            // A host may list one card many times under one name — ALSA lists this machine's eight
            // times — and a name resolves to the first, so the rest are entries nobody can choose.
            let Some(name) = name_of(&device).map(|n| qualified(id, &n)).filter(|n| taken.insert(n.clone())) else {
                continue;
            };
            out.push((name, device));
        }
    }
    if let Ok(mut seen) = seen().lock() {
        seen.extend(out.iter().filter(|(n, _)| asio_driver(n).is_some()).map(|(n, d)| (n.clone(), d.clone())));
    }
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
        if let Some(device) = seen().lock().ok().and_then(|s| s.get(name).cloned()) {
            return Ok(device);
        }
    }
    Err(format!("no {} device `{name}`", kind.word()))
}

#[cfg(test)]
mod tests {
    use super::{asio_driver, qualified, split};

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
