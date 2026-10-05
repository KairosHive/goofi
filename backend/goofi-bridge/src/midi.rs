//! The MIDI devices on the variable bus. Every input port the host lists is a group, named after
//! the port, that the session opens while the patch reads it — in a source, a feed or a followed
//! variable — and during a learn opens whole, so a controller never seen before can be mapped.
//! A device writes its group from its own callback thread, through the store and nothing else.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::{Duration, Instant};

use goofi_core::variables::{VariableStore, MIDI_ENTRIES};
use goofi_core::Data;
use goofi_graph::Graph;
use goofi_supervisor::sync::Mutex;
use serde::Serialize;

/// How long a learn keeps every port open with nothing mapped.
pub const LEARN: Duration = Duration::from_secs(30);

/// One open port: the group it writes, and the connection the host holds it by.
struct Grab {
    port: String,
    _link: goofi_supervisor::scope::Leased<midir::MidiInputConnection<()>>,
}

struct Inner {
    /// The host's ports as of the last probe, each with the group it is read as.
    listed: Vec<(String, String)>,
    /// The open ports, by group.
    open: HashMap<String, Grab>,
    /// When the learn under way ends; none outside a learn.
    learn: Option<Instant>,
}

pub struct Midi {
    store: Arc<Mutex<VariableStore>>,
    /// Locked BEFORE the store, as a callback locks nothing but the store.
    inner: Mutex<Inner>,
}

/// Ports closed by a resync, to be dropped once the caller has released the store.
#[must_use = "dropped under the store lock, a closing port can wait on its own callback"]
pub struct Closed {
    _ports: Vec<Grab>,
}

/// A port as `midi list` answers it: the host's name, its group, and whether it is open.
#[derive(Serialize)]
pub struct Port {
    pub port: String,
    pub group: String,
    pub open: bool,
}

/// The device's state as the group's entries carry it, and the names it is written under.
struct State {
    cc: Vec<f32>,
    notes: Vec<f32>,
    bend: [f32; 16],
    pressure: [f32; 16],
    names: [String; 4],
}

impl State {
    fn new(group: &str) -> State {
        let names = MIDI_ENTRIES.map(|(e, _)| format!("{group}.{e}"));
        State { cc: vec![0.0; 16 * 128], notes: vec![0.0; 16 * 128], bend: [0.0; 16], pressure: [0.0; 16], names }
    }

    /// Fold one message in and write the entry it moved through the store; a message that is
    /// not state writes nothing.
    fn feed(&mut self, bytes: &[u8], store: &Mutex<VariableStore>) {
        if let Some(entry) = self.take(bytes) {
            let i = MIDI_ENTRIES.iter().position(|(e, _)| *e == entry).expect("a named entry");
            store.lock().drive(&self.names[i], self.frame(entry));
        }
    }

    /// Fold one message in; answer the entry it moved, or none for a message that is not state.
    fn take(&mut self, bytes: &[u8]) -> Option<&'static str> {
        let (status, rest) = bytes.split_first()?;
        let channel = (status & 0x0f) as usize;
        let at = |i: usize| rest.get(i).copied().unwrap_or(0);
        let slot = channel * 128 + (at(0) as usize & 127);
        Some(match status & 0xf0 {
            0x90 if at(1) > 0 => {
                self.notes[slot] = at(1) as f32 / 127.0;
                "notes"
            }
            0x80 | 0x90 => {
                self.notes[slot] = 0.0;
                "notes"
            }
            0xb0 => {
                self.cc[slot] = at(1) as f32 / 127.0;
                "cc"
            }
            0xd0 => {
                self.pressure[channel] = at(0) as f32 / 127.0;
                "pressure"
            }
            // The wheel is 14-bit and asymmetric, -8192 down and 8191 up: each side has its own end.
            0xe0 => {
                let pitch = (at(0) as i32 | (at(1) as i32) << 7) - 8192;
                self.bend[channel] = pitch as f32 / if pitch >= 0 { 8191.0 } else { 8192.0 };
                "bend"
            }
            _ => return None,
        })
    }

    fn frame(&self, entry: &str) -> Data {
        let wide = |v: &[f32]| Data::numbers(v.iter().map(|v| *v as f64));
        match entry {
            "cc" => wide(&self.cc),
            "notes" => wide(&self.notes),
            "bend" => wide(&self.bend),
            _ => wide(&self.pressure),
        }
    }
}

impl Midi {
    pub fn new(store: Arc<Mutex<VariableStore>>) -> Midi {
        Midi { store, inner: Mutex::new(Inner { listed: Vec::new(), open: HashMap::new(), learn: None }) }
    }

    /// The host's input ports by name, or why it has none.
    fn host() -> Result<(midir::MidiInput, Vec<(String, midir::MidiInputPort)>), String> {
        let mut input = midir::MidiInput::new("goofi").map_err(|e| format!("midi: {e}"))?;
        input.ignore(midir::Ignore::All);
        let ports = input.ports().into_iter().filter_map(|p| Some((input.port_name(&p).ok()?, p))).collect();
        Ok((input, ports))
    }

    /// Ask the host which ports it lists now, so a device plugged in or pulled is seen by the
    /// next resync; answers why the host lists nothing.
    pub fn probe(&self) -> Option<String> {
        let (ports, reason) = match Midi::host() {
            Ok((_, ports)) => (ports.into_iter().map(|(name, _)| name).collect::<Vec<_>>(), None),
            Err(e) => (Vec::new(), Some(e)),
        };
        let mut inner = self.inner.lock();
        let named = std::mem::take(&mut inner.listed);
        inner.listed = ports.into_iter().map(|p| {
            let group = named.iter().find(|(n, _)| *n == p).map(|(_, g)| g.clone()).unwrap_or_default();
            (p, group)
        }).collect();
        reason
    }

    /// Start or end a learn: thirty seconds of every port open, ended early by the mapping made.
    pub fn learn(&self, on: bool) {
        self.inner.lock().learn = on.then(|| Instant::now() + LEARN);
    }

    /// Every port the host listed at the last probe, and whether it is open.
    pub fn list(&self) -> (Vec<Port>, bool) {
        let inner = self.inner.lock();
        let ports = inner.listed.iter().map(|(port, group)| Port { port: port.clone(), group: group.clone(), open: inner.open.contains_key(group) }).collect();
        (ports, inner.learn.is_some())
    }

    /// The open groups, with their ports.
    pub fn open(&self) -> Vec<(String, String)> {
        self.inner.lock().open.iter().map(|(g, grab)| (g.clone(), grab.port.clone())).collect()
    }

    /// Open every listed port the patch reads, or every one under a learn, and close the rest.
    /// Answers the closed ports, to drop once the store is let go, and whether anything moved.
    pub fn resync(&self, g: &Graph) -> (Closed, bool) {
        let referenced = g.referenced_groups();
        let mut inner = self.inner.lock();
        if inner.learn.is_some_and(|end| Instant::now() >= end) {
            inner.learn = None;
        }
        let learning = inner.learn.is_some();
        let Inner { listed, open, .. } = &mut *inner;
        let mut taken: HashSet<String> = HashSet::new();
        for (port, group) in listed.iter_mut() {
            let held = open.iter().find(|(_, grab)| grab.port == *port).map(|(g, _)| g.clone());
            *group = held.unwrap_or_else(|| group_of(port, |n| taken.contains(n) || (!open.contains_key(n) && g.group_taken(n))));
            taken.insert(group.clone());
        }
        let wanted: HashSet<&str> = listed.iter().filter(|(_, g)| learning || referenced.contains(g)).map(|(_, g)| g.as_str()).collect();
        let gone: Vec<String> = open.keys().filter(|g| !wanted.contains(g.as_str())).cloned().collect();
        let mut changed = !gone.is_empty();
        let mut store = self.store.lock();
        for group in &gone {
            store.release(group);
        }
        let closed = Closed { _ports: gone.iter().filter_map(|g| open.remove(g)).collect() };
        let opening: Vec<(String, String)> = listed.iter().filter(|(_, g)| wanted.contains(g.as_str()) && !open.contains_key(g)).cloned().collect();
        for (port, group) in &opening {
            let grab = match self.connect(port, group) {
                Ok(grab) => grab,
                Err(e) => {
                    trouble(&format!("`{port}` did not open for `{group}`: {e}"));
                    continue;
                }
            };
            match store.grab_midi(group, port) {
                Ok(()) => {
                    open.insert(group.clone(), grab);
                    changed = true;
                }
                Err(e) => trouble(&format!("`{group}` is no group for `{port}`: {e}")),
            }
        }
        (closed, changed)
    }

    fn connect(&self, port: &str, group: &str) -> Result<Grab, String> {
        let (input, ports) = Midi::host()?;
        let (_, port_id) = ports.iter().find(|(name, _)| name == port).ok_or("the host no longer lists it")?;
        let (store, mut state) = (self.store.clone(), State::new(group));
        let link = input.connect(port_id, "goofi-in", move |_, bytes, _| state.feed(bytes, &store), ()).map_err(|e| e.to_string())?;
        let link = goofi_supervisor::scope::leased(goofi_supervisor::scope::Kind::Device, format!("midi in {port}"), link);
        Ok(Grab { port: port.to_string(), _link: link })
    }

    /// Close every port.
    pub fn release_all(&self) {
        self.inner.lock().open.clear();
    }
}

/// The identifier a port's name reads as: `Launch Control XL:1` is `launch_control_xl_1`. The
/// `client:port` numbers ALSA appends change with every plug, so they are not part of it.
fn group_of(port: &str, taken: impl Fn(&str) -> bool) -> String {
    let stable = port.rsplit_once(' ').filter(|(_, tail)| tail.split_once(':').is_some_and(|(c, p)| c.parse::<u32>().is_ok() && p.parse::<u32>().is_ok())).map_or(port, |(head, _)| head);
    let mut base = String::new();
    for c in stable.chars() {
        let c = if c.is_ascii_alphanumeric() { c.to_ascii_lowercase() } else { '_' };
        if c != '_' || !base.ends_with('_') {
            base.push(c);
        }
    }
    let base = base.trim_matches('_');
    let base = match base.chars().next() {
        Some(c) if c.is_ascii_alphabetic() => base.to_string(),
        _ => format!("midi_{base}"),
    };
    if taken(&base) { goofi_core::fresh_name(&format!("{base}_"), 2, taken) } else { base }
}

fn trouble(why: &str) {
    goofi_supervisor::log::record(goofi_supervisor::log::Source::component("midi"), goofi_supervisor::log::Level::Error, None, why.to_string());
}
