//! Which cpal host a device comes from, and the name a patch stores for it. A device's name
//! CARRIES its host, so choosing a host is choosing a device, and every available host is offered.

use std::collections::HashMap;
use std::sync::OnceLock;
use goofi_supervisor::sync::Mutex;

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

    /// A host's devices of this kind. A host that refuses to enumerate contributes none: a name
    /// that is missing reports itself at open.
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

/// The audio hosts this build carries, comma separated: every prefix a device name can have.
/// Asked fresh, as [`named`] asks it, because a daemon that starts while goofi runs is a new host.
pub fn hosts() -> String {
    cpal::available_hosts().iter().map(|id| id.name()).collect::<Vec<_>>().join(", ")
}

/// What a Windows build that turned ASIO OFF owes the reader: why only the WASAPI endpoints of an
/// ASIO card are listed.
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

/// The host and the device a stored name is made of, round-tripping [`qualified`]. A host this
/// build does not support does not parse, so its devices are unreachable.
fn split(name: &str) -> Option<(cpal::HostId, &str)> {
    let (host, device) = name.split_once(": ")?;
    Some((host.parse().ok()?, device))
}

/// The ASIO driver `name` asks for, read off the NAME so it holds where ASIO is absent. One driver
/// loads per process, so [`crate::AudioEngine`] refuses a second by this.
pub(crate) fn asio_driver(name: &str) -> Option<&str> {
    let (host, device) = name.split_once(": ")?;
    host.eq_ignore_ascii_case(ASIO).then_some(device)
}

/// Every ASIO device seen while its driver could be loaded, by kind and stored name: enumeration
/// stops once a stream holds a driver, and the handle seen before stays valid.
fn seen() -> &'static Mutex<HashMap<(Kind, String), cpal::Device>> {
    static SEEN: OnceLock<Mutex<HashMap<(Kind, String), cpal::Device>>> = OnceLock::new();
    SEEN.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Remember `list`'s ASIO devices for `kind`.
fn remember(kind: Kind, list: &[(String, cpal::Device)]) {
    seen().lock().extend(
        list.iter().filter(|(n, _)| asio_driver(n).is_some()).map(|(n, d)| ((kind, n.clone()), d.clone())),
    );
}

/// The remembered ASIO devices for `kind`, by stored name.
fn remembered(kind: Kind) -> Vec<(String, cpal::Device)> {
    seen().lock().iter().filter(|((k, _), _)| *k == kind).map(|((_, n), d)| (n.clone(), d.clone())).collect()
}

/// Learn the ASIO devices in both directions once, at engine construction: before any plugin or
/// stream exists, and never on the clock thread.
pub(crate) fn warm() {
    static WARMED: OnceLock<()> = OnceLock::new();
    WARMED.get_or_init(|| {
        if let Some(asio) = cpal::available_hosts().into_iter().find(|id| id.name().eq_ignore_ascii_case(ASIO)) {
            for kind in [Kind::Input, Kind::Output] {
                remember(kind, &walk(kind, Some(asio)));
            }
        }
    });
}

fn name_of(d: &cpal::Device) -> Option<String> {
    d.description().ok().map(|d| d.name().to_string())
}

/// The devices of `kind` in every host, or in `only`, by stored name. A name listed twice keeps
/// its first device, the one [`device`] resolves to.
fn walk(kind: Kind, only: Option<cpal::HostId>) -> Vec<(String, cpal::Device)> {
    let mut out: Vec<(String, cpal::Device)> = Vec::new();
    let mut taken = std::collections::HashSet::new();
    for id in cpal::available_hosts().iter().copied().filter(|id| only.is_none_or(|o| o == *id)) {
        let Ok(host) = cpal::host_from_id(id) else { continue };
        for device in kind.all(&host) {
            if let Some(name) = name_of(&device).map(|n| qualified(id, &n)).filter(|n| taken.insert(n.clone())) {
                out.push((name, device));
            }
        }
    }
    out
}

/// Every device goofi offers for `kind`, by stored name, then the remembered ASIO devices the walk
/// could not see because a stream holds their driver.
pub(crate) fn named(kind: Kind) -> Vec<(String, cpal::Device)> {
    warm();
    let mut out = walk(kind, None);
    remember(kind, &out);
    let live: std::collections::HashSet<String> = out.iter().map(|(n, _)| n.clone()).collect();
    let mut extra: Vec<(String, cpal::Device)> =
        remembered(kind).into_iter().filter(|(n, _)| !live.contains(n)).collect();
    extra.sort_by(|a, b| a.0.cmp(&b.0));
    out.extend(extra);
    out
}

/// The device `name` names, `default` being the platform host's own default. The host in a name is
/// a choice, so the device is resolved in that host alone.
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
    // An ASIO name is missing once a stream holds its driver; the remembered handle is the device.
    if asio_driver(name).is_some() {
        if let Some(device) = seen().lock().get(&(kind, name.to_string())).cloned() {
            return Ok(device);
        }
    }
    Err(format!("no {} device `{name}`", kind.word()))
}
