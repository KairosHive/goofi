//! The patch document, typed once: the archive body, a copied fragment, and what a load or a
//! paste admits. Every spelling the `.gfi` and the clipboard carry is a field here.

use goofi_core::record::RecordedOutput;
use goofi_core::variables::{Control, Lock, VariableSource};
use goofi_core::Data;
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use ts_rs::TS;

use crate::{Mode, SourceState};

/// The `.gfi` version this build writes and reads.
pub const MANIFEST_VERSION: i64 = 1;

/// One patch's worth of nodes, links, variables and chrome. A fragment is one with no variables
/// and no arrangement; the archive body is one with both.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, TS)]
#[ts(optional_fields)]
pub struct PatchDoc {
    /// Keyed by uid spelling; a key that is not one is reminted on the way in.
    #[serde(default)]
    pub nodes: IndexMap<String, NodeRecord>,
    /// Keyed `out.slot>in.slot`, which is derived from the link and never read back.
    #[serde(default, skip_serializing_if = "IndexMap::is_empty")]
    pub links: IndexMap<String, Link>,
    #[serde(default, skip_serializing_if = "IndexMap::is_empty")]
    pub variables: IndexMap<String, VariableRecord>,
    #[serde(default, skip_serializing_if = "IndexMap::is_empty")]
    pub variable_groups: IndexMap<String, Group>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub arrangement: Option<Value>,
}

/// One variable as the document carries it: `{control?, source?, lock?}`. The replica carries no
/// value — a value is a frame on the data plane. The manifest carries a narrow one as a literal;
/// a wide array is a file beside it.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, TS)]
#[ts(optional_fields)]
pub struct VariableRecord {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(type = "Literal")]
    pub value: Option<Data>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub control: Option<Control>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<VariableSource>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lock: Option<Lock>,
}

/// A leaf, a sub-patch facade or a boundary port: one record kind, told apart by `type`.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, TS)]
#[ts(optional_fields)]
pub struct NodeRecord {
    #[serde(rename = "type")]
    pub type_id: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub pos: [f64; 2],
    /// The scope this record is a member of; absent is ROOT.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scope: Option<String>,
    #[serde(default, skip_serializing_if = "IndexMap::is_empty")]
    pub params: IndexMap<String, IndexMap<String, ParamEntry>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub viewers: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub baseline: Option<Value>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub record: Vec<RecordedOutput>,
}

/// One param's literal and, when it has one, its source record inline.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, TS)]
#[ts(optional_fields)]
pub struct ParamEntry {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(type = "Literal")]
    pub value: Option<Data>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mode: Option<Mode>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expression: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reference: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub triggers: Option<bool>,
}

impl ParamEntry {
    /// The source record an entry carries, if it carries one; `mode` is what says so.
    pub fn source(&self) -> Option<SourceState> {
        Some(SourceState {
            mode: self.mode?,
            expression: self.expression.clone().unwrap_or_default(),
            reference: self.reference.clone().unwrap_or_default(),
            triggers: self.triggers.unwrap_or(false),
        })
    }

    pub fn with_source(mut self, s: &SourceState) -> ParamEntry {
        self.mode = Some(s.mode);
        self.expression = (!s.expression.is_empty()).then(|| s.expression.clone());
        self.reference = (!s.reference.is_empty()).then(|| s.reference.clone());
        self.triggers = s.triggers.then_some(true);
        self
    }
}

/// Every param's literal, by group and name: what the patch holds of a node's params. Bounds,
/// options and types are the class's, derived from the catalog on every read.
pub type Values = IndexMap<String, IndexMap<String, Data>>;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
pub struct Link {
    pub node_out: String,
    pub slot_out: String,
    pub node_in: String,
    pub slot_in: String,
}

impl Link {
    pub fn key(&self) -> String {
        link_key(&self.node_out, &self.slot_out, &self.node_in, &self.slot_in)
    }
}

/// The key a link is stored under: both ends, so the map holds each wire once.
pub(crate) fn link_key(out: impl std::fmt::Display, slot_out: &str, inp: impl std::fmt::Display, slot_in: &str) -> String {
    format!("{out}.{slot_out}>{inp}.{slot_in}")
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, TS)]
pub struct Group {
    #[serde(default)]
    pub lock: Lock,
}

/// The `.gfi` manifest: the document with the version that says how to read it.
#[derive(Clone, Debug, Serialize, Deserialize, TS)]
#[ts(optional_fields)]
pub struct Archive {
    pub version: i64,
    pub goofi: String,
    pub patch: PatchDoc,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub viewpoint: Option<Value>,
}

impl Archive {
    /// Parse a manifest, refusing a version this build does not read and naming its writer.
    pub fn parse(text: &str) -> Result<Archive, String> {
        let raw: Value = serde_yaml_ng::from_str(text).map_err(|e| e.to_string())?;
        if raw.get("version").and_then(Value::as_i64) != Some(MANIFEST_VERSION) {
            let writer = raw
                .get("goofi")
                .and_then(Value::as_str)
                .map(|w| format!(" — the file was written by goofi {w}"))
                .unwrap_or_default();
            return Err(format!("unsupported .gfi version (this build reads version {MANIFEST_VERSION}){writer}"));
        }
        serde_json::from_value(raw).map_err(|e| e.to_string())
    }
}
