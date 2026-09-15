//! An engine-local mailbox for the shared host lifecycle. GPU work never runs on this worker.
use crate::runtime::{Control, Envelope, EventId, ParamValue, ServiceName, Transport, WireStatus};
use goofi_core::Data;
use goofi_node::{ParamGroups, ParamKey};
use std::collections::HashMap;
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

#[derive(Default)]
struct Mail {
    controls: Vec<Envelope>,
    inputs: HashMap<String, Data>,
    wake: bool,
}

type Publish = dyn Fn(&str, &Data) + Send + Sync;

pub struct Local {
    mail: Mutex<Mail>,
    changed: Condvar,
    publish: Box<Publish>,
    report: Box<dyn Fn(WireStatus) + Send + Sync>,
}

impl Local {
    pub fn new(
        publish: impl Fn(&str, &Data) + Send + Sync + 'static,
        report: impl Fn(WireStatus) + Send + Sync + 'static,
    ) -> Arc<Self> {
        Arc::new(Self {
            mail: Mutex::new(Mail::default()),
            changed: Condvar::new(),
            publish: Box::new(publish),
            report: Box::new(report),
        })
    }
    pub fn wake(&self) {
        self.mail.lock().unwrap().wake = true;
        self.changed.notify_one();
    }
    pub fn control(&self, control: Control) {
        self.controls(vec![control]);
    }
    fn controls(&self, controls: Vec<Control>) {
        if controls.is_empty() {
            return;
        }
        let mut mail = self.mail.lock().unwrap();
        for control in controls {
            // Coalesce values only after the latest pulse, which must observe its own values.
            if let Control::SetParam { key, .. } = &control {
                let from = mail
                    .controls
                    .iter()
                    .rposition(|m| matches!(m.control, Control::PulseParam { .. }))
                    .map_or(0, |i| i + 1);
                let mut i = 0;
                mail.controls.retain(|m| {
                    let keep = i < from || !matches!(&m.control, Control::SetParam { key: k, .. } if k == key);
                    i += 1;
                    keep
                });
            }
            mail.controls.push(Envelope { seq: 0, control });
        }
        mail.wake = true;
        self.changed.notify_one();
    }
    pub fn params(&self, before: &ParamGroups, after: &ParamGroups, pulses: Vec<Control>) {
        let mut changes = Vec::new();
        for (group, entries) in after {
            for (name, value) in entries {
                if before.get(group).and_then(|g| g.get(name)) != Some(value) {
                    changes.push(Control::SetParam {
                        key: ParamKey::new(group, name),
                        value: ParamValue::Literal(value.clone()),
                    });
                }
            }
        }
        changes.extend(pulses);
        self.controls(changes);
    }
    pub fn input(&self, name: &str, frame: Data) {
        let mut mail = self.mail.lock().unwrap();
        mail.inputs.insert(name.into(), frame);
        mail.wake = true;
        self.changed.notify_one();
    }
}

impl Transport for Local {
    fn wait(&self, timeout: Option<Duration>) -> Vec<EventId> {
        let mail = self.mail.lock().unwrap();
        let mut mail = match timeout {
            Some(d) => self.changed.wait_timeout_while(mail, d, |m| !m.wake).unwrap().0,
            None => self.changed.wait_while(mail, |m| !m.wake).unwrap(),
        };
        mail.wake = false;
        Vec::new()
    }
    fn drain_control(&self) -> Vec<Envelope> {
        std::mem::take(&mut self.mail.lock().unwrap().controls)
    }
    fn drain_inputs(&self) -> Vec<(String, usize, Data)> {
        self.mail.lock().unwrap().inputs.drain().map(|(slot, d)| (slot, 0, d)).collect()
    }
    fn wire_in(&self, slot: &str, services: &[ServiceName]) -> Result<(), String> {
        if services.is_empty() {
            self.mail.lock().unwrap().inputs.remove(slot);
        }
        Ok(())
    }
    fn wire_out(&self, _: &str, _: &[(ServiceName, EventId)]) -> Result<(), String> {
        Ok(())
    }
    fn record_out(&self, _: &[(String, u64)]) -> Result<(), String> {
        Ok(())
    }
    fn record_trouble(&self) -> Option<String> {
        None
    }
    fn publish(&self, slot: &str, frame: &Data) {
        (self.publish)(slot, frame);
    }
    fn report(&self, status: WireStatus) {
        (self.report)(status);
    }
}
