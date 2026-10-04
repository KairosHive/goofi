//! The MIDI devices on the variable bus. A grabbed device is a group of variables the manager
//! writes from the device's own callback thread, through the store and nothing else: no graph
//! lock, no settle, no undo. Which device a group reads is the document's; the port is the host's.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use goofi_core::variables::{Midi as Device, VariableStore, MIDI_ENTRIES};
use goofi_core::Data;
use goofi_supervisor::sync::Mutex;
use serde::Serialize;

/// One open port, leased as the device it is, named for the group it writes.
struct Grab {
    device: Device,
    _link: goofi_supervisor::scope::Leased<midir::MidiInputConnection<()>>,
}

pub struct Midi {
    store: Arc<Mutex<VariableStore>>,
    /// Locked AFTER the store: the callback takes the store alone, the manager both in that order.
    open: Mutex<HashMap<String, Grab>>,
    /// The groups tried since the last probe, so a gone port is not looked up on every settle.
    tried: Mutex<HashSet<String>>,
}

/// Ports closed by a resync, to be dropped once the caller has released the store.
#[must_use = "dropped under the store lock, a closing port can wait on its own callback"]
pub struct Closed(Vec<Grab>);

/// A port as `midi list` answers it: the host's name, and the group that reads it if one does.
#[derive(Serialize)]
pub struct Port {
    pub port: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub group: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub channel: Option<u8>,
    /// `present` when the host lists the port, `gone` when only the document names it.
    pub state: &'static str,
}

/// The device's state as the group's entries carry it.
struct State {
    cc: [f32; 128],
    notes: [f32; 128],
    bend: f32,
    pressure: f32,
}

impl State {
    /// Fold one message in; answer the entry it moved, or none for a message that is not state.
    fn take(&mut self, bytes: &[u8], channel: Option<u8>) -> Option<&'static str> {
        let (status, rest) = bytes.split_first()?;
        if channel.is_some_and(|c| status & 0x0f != c - 1) {
            return None;
        }
        let at = |i: usize| rest.get(i).copied().unwrap_or(0);
        Some(match status & 0xf0 {
            0x90 if at(1) > 0 => {
                self.notes[at(0) as usize & 127] = at(1) as f32 / 127.0;
                "notes"
            }
            0x80 | 0x90 => {
                self.notes[at(0) as usize & 127] = 0.0;
                "notes"
            }
            0xb0 => {
                self.cc[at(0) as usize & 127] = at(1) as f32 / 127.0;
                "cc"
            }
            0xd0 => {
                self.pressure = at(0) as f32 / 127.0;
                "pressure"
            }
            // The wheel is 14-bit and asymmetric, -8192 down and 8191 up: each side has its own end.
            0xe0 => {
                let pitch = (at(0) as i32 | (at(1) as i32) << 7) - 8192;
                self.bend = pitch as f32 / if pitch >= 0 { 8191.0 } else { 8192.0 };
                "bend"
            }
            _ => return None,
        })
    }

    fn frame(&self, entry: &str) -> Data {
        match entry {
            "cc" => Data::numbers(self.cc.iter().map(|v| *v as f64)),
            "notes" => Data::numbers(self.notes.iter().map(|v| *v as f64)),
            "bend" => Data::number(self.bend as f64),
            _ => Data::number(self.pressure as f64),
        }
    }
}

impl Midi {
    pub fn new(store: Arc<Mutex<VariableStore>>) -> Midi {
        Midi { store, open: Mutex::new(HashMap::new()), tried: Mutex::new(HashSet::new()) }
    }

    /// The host's input ports by name, or why it has none.
    fn host() -> Result<(midir::MidiInput, Vec<(String, midir::MidiInputPort)>), String> {
        let mut input = midir::MidiInput::new("goofi").map_err(|e| format!("midi: {e}"))?;
        input.ignore(midir::Ignore::All);
        let ports = input.ports().into_iter().filter_map(|p| Some((input.port_name(&p).ok()?, p))).collect();
        Ok((input, ports))
    }

    /// Every port the host lists, grabbed or not, and every grabbed group whose port is gone.
    pub fn list(&self, store: &VariableStore) -> (Vec<Port>, Option<String>) {
        let (listed, reason) = match Midi::host() {
            Ok((_, ports)) => (ports.into_iter().map(|(name, _)| name).collect::<Vec<_>>(), None),
            Err(e) => (Vec::new(), Some(e)),
        };
        let mut ports: Vec<Port> = listed.iter().map(|name| Port { port: name.clone(), group: None, channel: None, state: "present" }).collect();
        for (group, device) in store.midi_groups() {
            match ports.iter_mut().find(|p| p.port == device.port && p.group.is_none()) {
                Some(p) => {
                    p.group = Some(group.to_string());
                    p.channel = device.channel;
                }
                None => ports.push(Port { port: device.port.clone(), group: Some(group.to_string()), channel: device.channel, state: "gone" }),
            }
        }
        (ports, reason)
    }

    /// Match the open ports to the document's MIDI groups: close what no group reads, open what
    /// one does. `probe` retries the groups whose port was gone; a settle tries each group once.
    /// The closed ports come back to be dropped once the caller has let the store go.
    pub fn resync(&self, store: &VariableStore, probe: bool) -> Closed {
        let wanted: HashMap<String, Device> = store.midi_groups().map(|(g, d)| (g.to_string(), d.clone())).collect();
        let mut open = self.open.lock();
        let mut tried = self.tried.lock();
        // A closed port's callback may be waiting on the store: it is dropped after the store.
        let gone: Vec<String> = open.iter().filter(|(g, grab)| wanted.get(*g) != Some(&grab.device)).map(|(g, _)| g.clone()).collect();
        let mut closed = Closed(gone.iter().filter_map(|g| open.remove(g)).collect());
        if probe {
            tried.clear();
        }
        tried.retain(|g| wanted.contains_key(g));
        let missing: Vec<(&String, &Device)> = wanted.iter().filter(|(g, _)| !open.contains_key(*g) && !tried.contains(*g)).collect();
        if missing.is_empty() {
            return closed;
        }
        let Ok((_, ports)) = Midi::host() else { return closed };
        for (group, device) in missing {
            tried.insert(group.clone());
            let Some((_, port)) = ports.iter().find(|(name, _)| *name == device.port) else { continue };
            let Ok((input, _)) = Midi::host() else { return closed };
            let (store, channel, names) = (self.store.clone(), device.channel, MIDI_ENTRIES.map(|(e, _)| format!("{group}.{e}")));
            let mut state = State { cc: [0.0; 128], notes: [0.0; 128], bend: 0.0, pressure: 0.0 };
            let link = input.connect(
                port,
                "goofi-in",
                move |_, bytes, _| {
                    if let Some(entry) = state.take(bytes, channel) {
                        let i = MIDI_ENTRIES.iter().position(|(e, _)| *e == entry).expect("a named entry");
                        store.lock().drive(&names[i], state.frame(entry));
                    }
                },
                (),
            );
            match link {
                Ok(link) => {
                    let link = goofi_supervisor::scope::leased(goofi_supervisor::scope::Kind::Device, format!("midi in {}", device.port), link);
                    if let Some(old) = open.insert(group.clone(), Grab { device: device.clone(), _link: link }) {
                        closed.0.push(old);
                    }
                }
                Err(e) => trouble(&format!("`{}` did not open for `{group}`: {e}", device.port)),
            }
        }
        closed
    }

    /// Close every port; the groups keep their flags for the next boot.
    pub fn release_all(&self) {
        self.open.lock().clear();
    }
}

fn trouble(why: &str) {
    goofi_supervisor::log::record(goofi_supervisor::log::Source::component("midi"), goofi_supervisor::log::Level::Error, None, why.to_string());
}
