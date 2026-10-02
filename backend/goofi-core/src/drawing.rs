//! A drawing: a script of timed atomic ops, stored as compact byte code. A pad's hand and the CLI
//! both append to it; a node rasterizes it. The byte code is the one stored form.
//!
//! Byte code, a run of ops. Every number is a LEB128 varint; `dt` is milliseconds since the
//! previous timestamp in the drawing, op or segment.
//! - op: `dt << 2 | code`, code 0 clear, 1 stroke, 2 fill.
//! - stroke: `flags` (bits 0-1 cap round/butt/square, bits 2-3 dash solid/dash/dot, bit 4 erase),
//!   `r g b a` unless erase, `width`, `soft`, then a path.
//! - fill: `flags` (bit 4 erase), `r g b a` unless erase, then a path.
//! - path: segment count, then per segment `dt << 2 | tag` (0 move, 1 line, 2 cubic, 3 close) and
//!   its points as zigzag deltas from the previous point of the path, which starts at 0,0.
//!
//! Coordinates, width and softness are u16, 64 to a unit of [`SPAN`]; the origin is the top left, y runs down.
//! The variable holds the byte code as base64. The text form is one op per line (or `;`):
//! `[+ms] stroke [ink] [width w] [soft s] [cap c] [dash d] : [+ms] M x y L x y C x y x y x y Z`,
//! `[+ms] fill [ink] : path` and `[+ms] clear`; ink is `#rgb`, `#rrggbb`, `#rrggbbaa` or `erase`.

use base64::Engine;

/// The square that text coordinates, widths and softness span.
pub const SPAN: f32 = 1000.0;
/// The byte code's units: 64 to one text unit, so whole text values stay exact.
const Q: f32 = 64000.0;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Ink {
    Rgba([u8; 4]),
    Erase,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Cap {
    Round,
    Butt,
    Square,
}

/// Dash patterns in multiples of the width: dash is 3 on 2 off, dot is 1 on 2 off.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Dash {
    Solid,
    Dash,
    Dot,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Stroke {
    pub ink: Ink,
    pub width: u16,
    pub soft: u16,
    pub cap: Cap,
    pub dash: Dash,
}

pub type Point = [u16; 2];

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Seg {
    Move(Point),
    Line(Point),
    Cubic(Point, Point, Point),
    Close,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Segment {
    pub dt: u32,
    pub seg: Seg,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Kind {
    Clear,
    Stroke(Stroke, Vec<Segment>),
    Fill(Ink, Vec<Segment>),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Op {
    pub dt: u32,
    pub kind: Kind,
}

const CAPS: [(Cap, &str); 3] = [(Cap::Round, "round"), (Cap::Butt, "butt"), (Cap::Square, "square")];
const DASHES: [(Dash, &str); 3] = [(Dash::Solid, "solid"), (Dash::Dash, "dash"), (Dash::Dot, "dot")];
const ERASE: u8 = 0x10;

fn put(out: &mut Vec<u8>, mut v: u64) {
    loop {
        let b = (v & 0x7f) as u8;
        v >>= 7;
        if v == 0 {
            out.push(b);
            return;
        }
        out.push(b | 0x80);
    }
}

fn put_ink(out: &mut Vec<u8>, flags: u8, ink: Ink) {
    match ink {
        Ink::Erase => out.push(flags | ERASE),
        Ink::Rgba(c) => {
            out.push(flags);
            out.extend_from_slice(&c);
        }
    }
}

impl Seg {
    fn tag(&self) -> u64 {
        match self {
            Seg::Move(_) => 0,
            Seg::Line(_) => 1,
            Seg::Cubic(..) => 2,
            Seg::Close => 3,
        }
    }

    fn points(&self) -> Vec<Point> {
        match *self {
            Seg::Move(p) | Seg::Line(p) => vec![p],
            Seg::Cubic(a, b, c) => vec![a, b, c],
            Seg::Close => vec![],
        }
    }
}

fn put_path(out: &mut Vec<u8>, path: &[Segment]) {
    put(out, path.len() as u64);
    let mut at = [0i64; 2];
    for s in path {
        put(out, (s.dt as u64) << 2 | s.seg.tag());
        for p in s.seg.points() {
            for i in 0..2 {
                let d = p[i] as i64 - at[i];
                put(out, ((d << 1) ^ (d >> 63)) as u64);
                at[i] = p[i] as i64;
            }
        }
    }
}

/// The byte code of `ops`.
pub fn encode(ops: &[Op]) -> Vec<u8> {
    let mut out = Vec::new();
    for op in ops {
        let code = match &op.kind {
            Kind::Clear => 0,
            Kind::Stroke(..) => 1,
            Kind::Fill(..) => 2,
        };
        put(&mut out, (op.dt as u64) << 2 | code);
        match &op.kind {
            Kind::Clear => {}
            Kind::Stroke(s, path) => {
                let cap = CAPS.iter().position(|c| c.0 == s.cap).unwrap() as u8;
                let dash = DASHES.iter().position(|d| d.0 == s.dash).unwrap() as u8;
                put_ink(&mut out, cap | dash << 2, s.ink);
                put(&mut out, s.width as u64);
                put(&mut out, s.soft as u64);
                put_path(&mut out, path);
            }
            Kind::Fill(ink, path) => {
                put_ink(&mut out, 0, *ink);
                put_path(&mut out, path);
            }
        }
    }
    out
}

struct Reader<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl Reader<'_> {
    fn byte(&mut self) -> Result<u8, String> {
        let b = *self.bytes.get(self.at).ok_or("the drawing ends inside an op")?;
        self.at += 1;
        Ok(b)
    }

    fn var(&mut self) -> Result<u64, String> {
        let mut v = 0u64;
        for shift in (0..64).step_by(7) {
            let b = self.byte()?;
            v |= ((b & 0x7f) as u64) << shift;
            if b & 0x80 == 0 {
                return Ok(v);
            }
        }
        Err("the drawing holds a number too long".into())
    }

    fn small(&mut self, max: u64, what: &str) -> Result<u64, String> {
        let v = self.var()?;
        if v > max {
            return Err(format!("the drawing holds a {what} out of range"));
        }
        Ok(v)
    }

    fn ink(&mut self) -> Result<(u8, Ink), String> {
        let flags = self.byte()?;
        if flags & ERASE != 0 {
            return Ok((flags, Ink::Erase));
        }
        Ok((flags, Ink::Rgba([self.byte()?, self.byte()?, self.byte()?, self.byte()?])))
    }

    fn path(&mut self) -> Result<Vec<Segment>, String> {
        let n = self.var()? as usize;
        let mut at = [0i64; 2];
        let mut path = Vec::with_capacity(n.min(self.bytes.len()));
        for _ in 0..n {
            let head = self.small(u32::MAX as u64 * 4 + 3, "time")?;
            let mut pt = || -> Result<Point, String> {
                let mut p = [0u16; 2];
                for i in 0..2 {
                    let z = self.var()?;
                    at[i] += ((z >> 1) as i64) ^ -((z & 1) as i64);
                    p[i] = u16::try_from(at[i]).map_err(|_| "the drawing holds a point off the sheet")?;
                }
                Ok(p)
            };
            let seg = match head & 3 {
                0 => Seg::Move(pt()?),
                1 => Seg::Line(pt()?),
                2 => Seg::Cubic(pt()?, pt()?, pt()?),
                _ => Seg::Close,
            };
            path.push(Segment { dt: (head >> 2) as u32, seg });
        }
        check_path(&path)?;
        Ok(path)
    }
}

fn check_path(path: &[Segment]) -> Result<(), String> {
    match path.first() {
        Some(Segment { seg: Seg::Move(_), .. }) => Ok(()),
        _ => Err("a path starts with `M x y`".into()),
    }
}

/// The ops in `bytes`, refused with a reason when it is not byte code.
pub fn decode(bytes: &[u8]) -> Result<Vec<Op>, String> {
    let mut r = Reader { bytes, at: 0 };
    let mut ops = Vec::new();
    while r.at < bytes.len() {
        let head = r.small(u32::MAX as u64 * 4 + 3, "time")?;
        let kind = match head & 3 {
            0 => Kind::Clear,
            1 => {
                let (flags, ink) = r.ink()?;
                let cap = CAPS.get((flags & 3) as usize).ok_or("the drawing holds an unknown cap")?.0;
                let dash = DASHES.get((flags >> 2 & 3) as usize).ok_or("the drawing holds an unknown dash")?.0;
                let width = r.small(u16::MAX as u64, "width")? as u16;
                let soft = r.small(u16::MAX as u64, "softness")? as u16;
                Kind::Stroke(Stroke { ink, width, soft, cap, dash }, r.path()?)
            }
            2 => Kind::Fill(r.ink()?.1, r.path()?),
            _ => return Err("the drawing holds an unknown op".into()),
        };
        ops.push(Op { dt: (head >> 2) as u32, kind });
    }
    Ok(ops)
}

/// The ops a variable value holds: base64 byte code, empty for a blank sheet.
pub fn from_value(value: &str) -> Result<Vec<Op>, String> {
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(value.trim())
        .map_err(|e| format!("the drawing is not base64: {e}"))?;
    decode(&bytes)
}

pub fn to_value(ops: &[Op]) -> String {
    base64::engine::general_purpose::STANDARD.encode(encode(ops))
}

/// `stored` with `more` appended. Ops before the last clear cannot show, so they are dropped.
pub fn append(stored: &str, more: Vec<Op>) -> Result<String, String> {
    let mut ops = from_value(stored)?;
    ops.extend(more);
    let from = ops.iter().rposition(|o| o.kind == Kind::Clear).unwrap_or(0);
    Ok(to_value(&ops[from..]))
}

fn quantize(v: &str, what: &str) -> Result<u16, String> {
    let x: f32 = v.parse().map_err(|_| format!("`{v}` is not a {what}"))?;
    if !(0.0..=SPAN).contains(&x) {
        return Err(format!("{what} `{v}` is outside 0..{SPAN}"));
    }
    Ok((x / SPAN * Q).round() as u16)
}

fn spell(q: u16) -> String {
    let s = format!("{:.2}", q as f32 * SPAN / Q);
    s.trim_end_matches('0').trim_end_matches('.').to_string()
}

fn parse_ink(word: &str) -> Option<Ink> {
    if word == "erase" {
        return Some(Ink::Erase);
    }
    let hex = word.strip_prefix('#')?;
    let digits: Vec<u8> = hex.chars().map(|c| c.to_digit(16).map(|d| d as u8)).collect::<Option<_>>()?;
    let c = match digits.len() {
        3 => [digits[0] * 17, digits[1] * 17, digits[2] * 17, 255],
        6 | 8 => {
            let mut c = [255u8; 4];
            for (i, pair) in digits.chunks(2).enumerate() {
                c[i] = pair[0] << 4 | pair[1];
            }
            c
        }
        _ => return None,
    };
    Some(Ink::Rgba(c))
}

fn spell_ink(ink: Ink) -> String {
    match ink {
        Ink::Erase => "erase".into(),
        Ink::Rgba([r, g, b, 255]) => format!("#{r:02x}{g:02x}{b:02x}"),
        Ink::Rgba([r, g, b, a]) => format!("#{r:02x}{g:02x}{b:02x}{a:02x}"),
    }
}

/// A `+ms` time word, or None for any other word.
fn time(word: &str) -> Option<Result<u32, String>> {
    let ms = word.strip_prefix('+')?;
    Some(ms.parse().map_err(|_| format!("`{word}` is not a time in ms")))
}

fn parse_op(words: &[&str]) -> Result<Op, String> {
    let mut w = words.iter().copied().peekable();
    let dt = match w.peek().and_then(|x| time(x)) {
        Some(t) => {
            w.next();
            t?
        }
        None => 0,
    };
    let verb = w.next().unwrap_or_default();
    let head: Vec<&str> = w.by_ref().take_while(|x| *x != ":").collect();
    let path: Vec<&str> = w.collect();
    let mut head = head.into_iter();
    let mut ink = Ink::Rgba([0, 0, 0, 255]);
    let mut stroke = Stroke { ink, width: quantize("10", "width")?, soft: 0, cap: Cap::Round, dash: Dash::Solid };
    while let Some(word) = head.next() {
        if let Some(i) = parse_ink(word) {
            ink = i;
            continue;
        }
        let value = head.next().ok_or(format!("`{word}` wants a value"))?;
        match (verb, word) {
            ("stroke", "width") => stroke.width = quantize(value, "width")?,
            ("stroke", "soft") => stroke.soft = quantize(value, "softness")?,
            ("stroke", "cap") => {
                stroke.cap = CAPS.iter().find(|c| c.1 == value).ok_or(format!("cap is round/butt/square, not `{value}`"))?.0
            }
            ("stroke", "dash") => {
                stroke.dash = DASHES.iter().find(|d| d.1 == value).ok_or(format!("dash is solid/dash/dot, not `{value}`"))?.0
            }
            _ => return Err(format!("`{verb}` takes no `{word}`")),
        }
    }
    stroke.ink = ink;
    let kind = match verb {
        "clear" if path.is_empty() => Kind::Clear,
        "stroke" => Kind::Stroke(stroke, parse_path(&path)?),
        "fill" => Kind::Fill(ink, parse_path(&path)?),
        "clear" => return Err("`clear` takes no path".into()),
        other => return Err(format!("`{other}` is not an op: clear, stroke or fill")),
    };
    Ok(Op { dt, kind })
}

fn parse_path(words: &[&str]) -> Result<Vec<Segment>, String> {
    let mut path = Vec::new();
    let mut w = words.iter().copied();
    let mut dt = 0;
    while let Some(word) = w.next() {
        if let Some(t) = time(word) {
            dt = t?;
            continue;
        }
        let mut pt = || -> Result<Point, String> {
            let mut p = [0u16; 2];
            for c in &mut p {
                *c = quantize(w.next().ok_or(format!("`{word}` wants more coordinates"))?, "coordinate")?;
            }
            Ok(p)
        };
        let seg = match word {
            "M" => Seg::Move(pt()?),
            "L" => Seg::Line(pt()?),
            "C" => Seg::Cubic(pt()?, pt()?, pt()?),
            "Z" => Seg::Close,
            other => return Err(format!("`{other}` is not a segment: M, L, C or Z")),
        };
        path.push(Segment { dt, seg });
        dt = 0;
    }
    check_path(&path)?;
    Ok(path)
}

/// The ops a text script spells, refused with the line that is wrong.
pub fn parse(text: &str) -> Result<Vec<Op>, String> {
    let mut ops = Vec::new();
    for (n, line) in text.lines().enumerate() {
        let line = line.split_once("//").map_or(line, |(code, _)| code);
        for part in line.split(';') {
            let words: Vec<&str> = part.split_whitespace().collect();
            if !words.is_empty() {
                ops.push(parse_op(&words).map_err(|e| format!("line {}: {e}", n + 1))?);
            }
        }
    }
    Ok(ops)
}

/// The text form of `ops`, one op per line, which [`parse`] reads back to the same ops.
pub fn print(ops: &[Op]) -> String {
    let at = |dt: u32| if dt == 0 { String::new() } else { format!("+{dt} ") };
    let path = |p: &[Segment]| {
        p.iter()
            .map(|s| {
                let pts: Vec<String> = s.seg.points().iter().map(|p| format!(" {} {}", spell(p[0]), spell(p[1]))).collect();
                let tag = ["M", "L", "C", "Z"][s.seg.tag() as usize];
                format!("{}{tag}{}", at(s.dt), pts.concat())
            })
            .collect::<Vec<_>>()
            .join(" ")
    };
    let lines: Vec<String> = ops
        .iter()
        .map(|o| match &o.kind {
            Kind::Clear => format!("{}clear", at(o.dt)),
            Kind::Stroke(s, p) => {
                let cap = CAPS.iter().find(|c| c.0 == s.cap).unwrap().1;
                let dash = DASHES.iter().find(|d| d.0 == s.dash).unwrap().1;
                format!(
                    "{}stroke {} width {} soft {} cap {cap} dash {dash} : {}",
                    at(o.dt), spell_ink(s.ink), spell(s.width), spell(s.soft), path(p)
                )
            }
            Kind::Fill(ink, p) => format!("{}fill {} : {}", at(o.dt), spell_ink(*ink), path(p)),
        })
        .collect();
    lines.join("\n")
}

/// The drawing as straight RGBA rows, row 0 the top, at `width` x `height` pixels.
pub fn raster(ops: &[Op], width: u32, height: u32) -> Result<Vec<u8>, String> {
    use tiny_skia::{BlendMode, FillRule, LineCap, LineJoin, Paint, PathBuilder, Pixmap, PixmapPaint, StrokeDash, Transform};
    let mut sheet = Pixmap::new(width, height).ok_or("a drawing wants a size above zero")?;
    let (sx, sy) = (width as f32 / Q, height as f32 / Q);
    let scale = (sx + sy) / 2.0;
    let build = |path: &[Segment]| {
        let f = |p: Point| (p[0] as f32 * sx, p[1] as f32 * sy);
        let mut b = PathBuilder::new();
        for s in path {
            match s.seg {
                Seg::Move(p) => b.move_to(f(p).0, f(p).1),
                Seg::Line(p) => b.line_to(f(p).0, f(p).1),
                Seg::Cubic(a, c, e) => b.cubic_to(f(a).0, f(a).1, f(c).0, f(c).1, f(e).0, f(e).1),
                Seg::Close => b.close(),
            }
        }
        b.finish()
    };
    let paint_of = |ink: Ink, blend: bool| {
        let mut paint = Paint { anti_alias: true, ..Paint::default() };
        let [r, g, b, a] = match ink {
            Ink::Rgba(c) => c,
            Ink::Erase => [0, 0, 0, 255],
        };
        paint.set_color_rgba8(r, g, b, a);
        if blend && ink == Ink::Erase {
            paint.blend_mode = BlendMode::DestinationOut;
        }
        paint
    };
    for op in ops {
        match &op.kind {
            Kind::Clear => sheet.fill(tiny_skia::Color::TRANSPARENT),
            Kind::Fill(ink, path) => {
                if let Some(p) = build(path) {
                    sheet.fill_path(&p, &paint_of(*ink, true), FillRule::Winding, Transform::identity(), None);
                }
            }
            Kind::Stroke(s, path) => {
                let Some(p) = build(path) else { continue };
                let w = (s.width as f32 * scale).max(0.5);
                let pen = tiny_skia::Stroke {
                    width: w,
                    line_cap: match s.cap {
                        Cap::Round => LineCap::Round,
                        Cap::Butt => LineCap::Butt,
                        Cap::Square => LineCap::Square,
                    },
                    line_join: LineJoin::Round,
                    dash: match s.dash {
                        Dash::Solid => None,
                        Dash::Dash => StrokeDash::new(vec![3.0 * w, 2.0 * w], 0.0),
                        Dash::Dot => StrokeDash::new(vec![w, 2.0 * w], 0.0),
                    },
                    ..Default::default()
                };
                let sigma = s.soft as f32 * scale;
                if sigma < 0.5 {
                    sheet.stroke_path(&p, &paint_of(s.ink, true), &pen, Transform::identity(), None);
                    continue;
                }
                // A soft stroke is a gaussian blur of its own layer, as the pad's canvas filter is.
                let mut layer = Pixmap::new(width, height).expect("the sheet's size");
                layer.stroke_path(&p, &paint_of(s.ink, false), &pen, Transform::identity(), None);
                blur(layer.data_mut(), width as usize, height as usize, sigma);
                let blend = if s.ink == Ink::Erase { BlendMode::DestinationOut } else { BlendMode::SourceOver };
                sheet.draw_pixmap(0, 0, layer.as_ref(), &PixmapPaint { blend_mode: blend, ..Default::default() }, Transform::identity(), None);
            }
        }
    }
    Ok(sheet.pixels().iter().flat_map(|p| {
        let c = p.demultiply();
        [c.red(), c.green(), c.blue(), c.alpha()]
    }).collect())
}

/// Three box passes each way, the CSS approximation of a gaussian of `sigma` pixels.
fn blur(data: &mut [u8], w: usize, h: usize, sigma: f32) {
    let r = (((sigma * 3.0 * (2.0 * std::f32::consts::PI).sqrt() / 4.0) + 0.5) as usize / 2).max(1);
    let mut line = Vec::new();
    for (len, count, step, stride) in [(w, h, 4, w * 4), (h, w, w * 4, 4)] {
        for k in 0..count {
            for _ in 0..3 {
                line.clear();
                line.extend((0..len).map(|i| {
                    let at = k * stride + i * step;
                    [data[at], data[at + 1], data[at + 2], data[at + 3]]
                }));
                let mut sum = [0u32; 4];
                let get = |i: isize| line[i.clamp(0, len as isize - 1) as usize];
                for i in -(r as isize)..=(r as isize) {
                    for (s, v) in sum.iter_mut().zip(get(i)) {
                        *s += v as u32;
                    }
                }
                let n = (2 * r + 1) as u32;
                for i in 0..len {
                    let at = k * stride + i * step;
                    for c in 0..4 {
                        data[at + c] = (sum[c] / n) as u8;
                        sum[c] = sum[c] + get(i as isize + r as isize + 1)[c] as u32 - get(i as isize - r as isize)[c] as u32;
                    }
                }
            }
        }
    }
}
