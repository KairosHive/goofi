//! A `.wgsl` node file: its header block, `/* goofi { … } */`, is its declaration.

use std::path::Path;
use std::sync::Arc;

use goofi_core::node::{Body, Read};

pub(crate) fn read(path: &Path) -> Result<Read, super::Failed> {
    let graphics = || Some("graphics".to_string());
    let source = std::fs::read_to_string(path).map_err(|e| (format!("{}: {e}", path.display()), graphics()))?;
    let mut intro = header(&source).map_err(|why| (why, graphics()))?;
    intro.engine = "graphics".into();
    Ok(Read { intro: Arc::new(intro), body: Body::Shader { source } })
}

fn header(source: &str) -> Result<goofi_core::probe::Introspection, String> {
    let start = source.find("/*").ok_or("no header: a node file opens with `/* goofi { … } */`")?;
    let end = source[start..].find("*/").map(|i| start + i).ok_or("the header block is not closed")?;
    let inside = source[start + 2..end].trim_start();
    let json = inside.strip_prefix("goofi").ok_or("the header block does not open with `goofi`")?;
    goofi_core::node::parse_introspection(json.trim()).map_err(|e| format!("header: {e}"))
}
