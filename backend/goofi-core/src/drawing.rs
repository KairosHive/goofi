//! A drawing: a script of atomic ops, which a paint pad's hand and the CLI both send, and
//! which the manager rasterizes onto the pad's array. The text form is one op per line (or `;`):
//! `stroke [ink] [width w] [soft s] [cap c] [dash d] : M x y L x y C x y x y x y Z`,
//! `fill [ink] : path` and `clear`; ink is `#rgb`, `#rrggbb`, `#rrggbbaa` or `erase`.
//! Coordinates, width and softness span [`SPAN`] whatever the pad's size; the origin is the top
//! left, y runs down.

/// The square that text coordinates, widths and softness span.
pub const SPAN: f32 = 1000.0;
/// The quantum coordinates are held in: 64 to one text unit, so whole text values stay exact.
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

#[derive(Clone, Debug, PartialEq)]
pub enum Kind {
    Clear,
    Stroke(Stroke, Vec<Seg>),
    Fill(Ink, Vec<Seg>),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Op {
    pub kind: Kind,
}

const CAPS: [(Cap, &str); 3] = [(Cap::Round, "round"), (Cap::Butt, "butt"), (Cap::Square, "square")];
const DASHES: [(Dash, &str); 3] = [(Dash::Solid, "solid"), (Dash::Dash, "dash"), (Dash::Dot, "dot")];
fn check_path(path: &[Seg]) -> Result<(), String> {
    match path.first() {
        Some(Seg::Move(_)) => Ok(()),
        _ => Err("a path starts with `M x y`".into()),
    }
}

fn quantize(v: &str, what: &str) -> Result<u16, String> {
    let x: f32 = v.parse().map_err(|_| format!("`{v}` is not a {what}"))?;
    if !(0.0..=SPAN).contains(&x) {
        return Err(format!("{what} `{v}` is outside 0..{SPAN}"));
    }
    Ok((x / SPAN * Q).round() as u16)
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

fn parse_op(words: &[&str]) -> Result<Op, String> {
    let mut w = words.iter().copied().peekable();
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
    Ok(Op { kind })
}

fn parse_path(words: &[&str]) -> Result<Vec<Seg>, String> {
    let mut path = Vec::new();
    let mut w = words.iter().copied();
    while let Some(word) = w.next() {
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
        path.push(seg);
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

/// `ops` drawn onto `sheet`, an `[height, width, 4]` straight RGBA array in 0..1, row 0 the top:
/// the array after the strokes, in the same form.
pub fn raster(sheet: &[f32], width: u32, height: u32, ops: &[Op]) -> Result<Vec<f32>, String> {
    use tiny_skia::{BlendMode, ColorU8, FillRule, LineCap, LineJoin, Paint, PathBuilder, Pixmap, PixmapPaint, StrokeDash, Transform};
    let texels = (width as usize) * (height as usize);
    if sheet.len() != texels * 4 {
        return Err(format!("a {width}x{height} pad holds {} numbers, not {}", texels * 4, sheet.len()));
    }
    let byte = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    let data: Vec<u8> = sheet
        .chunks_exact(4)
        .flat_map(|t| {
            let c = ColorU8::from_rgba(byte(t[0]), byte(t[1]), byte(t[2]), byte(t[3])).premultiply();
            [c.red(), c.green(), c.blue(), c.alpha()]
        })
        .collect();
    let mut sheet = Pixmap::from_vec(data, tiny_skia::IntSize::from_wh(width, height).ok_or("a drawing wants a size above zero")?)
        .ok_or("a drawing wants a size above zero")?;
    let (sx, sy) = (width as f32 / Q, height as f32 / Q);
    let scale = (sx + sy) / 2.0;
    let build = |path: &[Seg]| {
        let f = |p: Point| (p[0] as f32 * sx, p[1] as f32 * sy);
        let mut b = PathBuilder::new();
        for s in path {
            match *s {
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
        [c.red(), c.green(), c.blue(), c.alpha()].map(|v| f32::from(v) / 255.0)
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
