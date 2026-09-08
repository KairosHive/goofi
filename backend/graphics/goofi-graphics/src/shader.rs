//! A `.wgsl` node file: the header block that IS its manifest, the prelude the engine appends,
//! and naga's verdict on the two together.

use std::sync::atomic::{AtomicU64, Ordering};

use goofi_core::probe::Introspection;
use goofi_core::SlotType;
use goofi_node::{NodeManifest, ParamDecl, ParamSpec};

/// The names the prelude declares. A header that takes one is refused, rather than shadowing it.
const RESERVED: &[&str] =
    &["time", "frame", "resolution", "samp", "p", "Params", "Vs", "vs", "fs", "shade", "Frag"];

/// The one output every graphics node has.
const OUT: &str = "out";

/// Every input that arrives as a FRAME rather than as a stage of this engine's own plan, in the
/// order their uploads are numbered. What a frame's own range is carried against.
pub fn array_inputs(m: &NodeManifest) -> impl Iterator<Item = &'static str> + '_ {
    m.inputs.iter().filter(|s| s.kind != SlotType::Texture).map(|s| s.name)
}

/// The two fields the engine adds to `Params` for one ARRAY input: the range its last frame
/// spanned. A body cannot work that out for itself — a reduction over every texel, at every texel.
fn range_fields(input: &str) -> [String; 2] {
    [format!("{input}_lo"), format!("{input}_hi")]
}

/// The header's manifest, with the one output added. The file's WHOLE text stays the source that
/// naga reads, so a line number it reports is the line an author sees.
pub fn header(source: &str) -> Result<Introspection, String> {
    let start = source.find("/*").ok_or("no header: a node file opens with `/* goofi { … } */`")?;
    let end = source[start..].find("*/").map(|i| start + i).ok_or("the header block is not closed")?;
    let inside = source[start + 2..end].trim_start();
    let json = inside.strip_prefix("goofi").ok_or("the header block does not open with `goofi`")?;
    let mut intro = goofi_node::parse_introspection(json.trim()).map_err(|e| format!("header: {e}"))?;
    if !intro.outputs.is_empty() {
        return Err("the header lists outputs; a graphics node has the one output `out`".into());
    }
    intro.outputs.push(goofi_core::probe::OutSlot { name: OUT.to_string(), kind: SlotType::Texture.name().to_string() });
    for s in &intro.inputs {
        // A TEXTURE is a stage of this engine's own plan; everything else arrives as a frame and
        // is uploaded, which is what makes a crossing declare the plane it crosses FROM.
        match SlotType::from_name(&s.kind) {
            Some(SlotType::Texture | SlotType::Array | SlotType::Audio) => {}
            _ => return Err(format!(
                "input `{}` is `{}`; a graphics input is TEXTURE, ARRAY or AUDIO", s.name, s.kind
            )),
        }
    }
    let mut taken: Vec<String> = RESERVED.iter().map(|s| (*s).to_string()).collect();
    for buffer in &intro.state {
        if !goofi_core::globals::is_valid_name(buffer) {
            return Err(format!("state buffer `{buffer}`: a letter, then letters or digits"));
        }
        if taken.contains(buffer) {
            return Err(format!("state buffer `{buffer}` is already a name in this file"));
        }
        taken.push(buffer.clone());
        taken.push(writer(buffer));
    }
    for input in &intro.inputs {
        if SlotType::from_name(&input.kind) != Some(SlotType::Texture) {
            taken.extend(range_fields(&input.name));
        }
    }
    let clash = intro
        .inputs
        .iter()
        .map(|s| &s.name)
        .chain(intro.params.iter().map(|p| &p.name))
        .find(|n| taken.contains(n));
    if let Some(name) = clash {
        return Err(format!("`{name}` is the prelude's; choose another name"));
    }
    // A graphics node with no texture behind it makes its own frames, so it takes the patch's
    // default size rather than following anything.
    intro.producer = !intro.inputs.iter().any(|s| SlotType::from_name(&s.kind) == Some(SlotType::Texture));
    Ok(intro)
}

/// The function a body writes to fill one state buffer; the buffer's own name reads what the last
/// tick left there.
pub fn writer(buffer: &str) -> String {
    format!("next_{buffer}")
}

fn wgsl_type(spec: &ParamSpec) -> &'static str {
    match spec {
        ParamSpec::Float { .. } => "f32",
        ParamSpec::Int { .. } => "i32",
        ParamSpec::Bool { .. } | ParamSpec::Str { .. } | ParamSpec::Pulse => "u32",
    }
}

/// What the engine appends after the file: the bindings a body reads, and the stages that call it.
/// `uv` is (0, 0) at the TOP-left, WGSL's own texture space.
pub fn prelude(manifest: &NodeManifest, state: &[String]) -> String {
    let mut s = String::from(
        "\n@group(0) @binding(0) var<uniform> time: f32;\n\
         @group(0) @binding(1) var<uniform> resolution: vec2f;\n\
         @group(0) @binding(2) var samp: sampler;\n",
    );
    let ranges: Vec<String> = array_inputs(manifest).flat_map(range_fields).collect();
    if !manifest.params.is_empty() || !ranges.is_empty() {
        s.push_str("struct Params {\n");
        for d in manifest.params {
            s.push_str(&format!("    {}: {},\n", d.name, wgsl_type(&d.spec)));
        }
        for field in &ranges {
            s.push_str(&format!("    {field}: f32,\n"));
        }
        s.push_str("}\n@group(0) @binding(3) var<uniform> p: Params;\n");
    }
    s.push_str("@group(0) @binding(4) var<uniform> frame: u32;\n");
    for (i, input) in manifest.inputs.iter().enumerate() {
        s.push_str(&format!("@group(1) @binding({i}) var {}: texture_2d<f32>;\n", input.name));
    }
    // A state buffer reads as the LAST tick left it and is written through its own function, so
    // the two never name one thing.
    for (i, buffer) in state.iter().enumerate() {
        s.push_str(&format!("@group(2) @binding({i}) var {buffer}: texture_2d<f32>;\n"));
    }
    s.push_str("struct Frag {\n    @location(0) colour: vec4f,\n");
    for (i, buffer) in state.iter().enumerate() {
        s.push_str(&format!("    @location({}) {buffer}: vec4f,\n", i + 1));
    }
    s.push_str(
        "}\n\
         struct Vs { @builtin(position) pos: vec4f, @location(0) uv: vec2f }\n\
         @vertex fn vs(@builtin(vertex_index) i: u32) -> Vs {\n\
         \x20   let x = f32(i32(i & 1u) * 4 - 1);\n\
         \x20   let y = f32(i32(i & 2u) * 2 - 1);\n\
         \x20   var o: Vs;\n\
         \x20   o.pos = vec4f(x, y, 0.0, 1.0);\n\
         \x20   o.uv = vec2f((x + 1.0) * 0.5, (1.0 - y) * 0.5);\n\
         \x20   return o;\n\
         }\n\
         @fragment fn fs(v: Vs) -> Frag {\n\
         \x20   var o: Frag;\n\
         \x20   o.colour = shade(v.uv);\n",
    );
    for buffer in state {
        s.push_str(&format!("    o.{buffer} = {}(v.uv);\n", writer(buffer)));
    }
    s.push_str("    return o;\n}\n");
    s
}

/// naga's verdict on the file plus its prelude, with the line and column in it.
pub fn validate(full: &str) -> Result<(), String> {
    let module = naga::front::wgsl::parse_str(full).map_err(|e| e.emit_to_string(full))?;
    naga::valid::Validator::new(naga::valid::ValidationFlags::all(), naga::valid::Capabilities::empty())
        .validate(&module)
        .map(|_| ())
        .map_err(|e| e.emit_to_string(full))
}

/// One stage's `Params` buffer: a 4-byte scalar per declared param, then the range each ARRAY
/// input's last frame spanned, padded to 16. Every field is a scalar, so the layout needs no
/// layouter.
pub fn uniform_bytes(decls: &[ParamDecl], atomics: &[AtomicU64], ranges: &[[f32; 2]]) -> Vec<u8> {
    let mut out = Vec::with_capacity((decls.len() + ranges.len() * 2) * 4 + 16);
    for (d, a) in decls.iter().zip(atomics) {
        let v = f64::from_bits(a.load(Ordering::Relaxed));
        match d.spec {
            ParamSpec::Float { .. } => out.extend_from_slice(&(v as f32).to_le_bytes()),
            ParamSpec::Int { .. } => out.extend_from_slice(&(v.round() as i32).to_le_bytes()),
            _ => out.extend_from_slice(&(v.round().max(0.0) as u32).to_le_bytes()),
        }
    }
    for [lo, hi] in ranges {
        out.extend_from_slice(&lo.to_le_bytes());
        out.extend_from_slice(&hi.to_le_bytes());
    }
    out.resize(out.len().next_multiple_of(16), 0);
    out
}
