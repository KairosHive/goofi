//! The turtle language a drawing is written in, and the strokes it becomes. The op parses here and
//! hands the WIDGET strokes — the very things a mouse makes — so the CLI and a finger reach the
//! canvas by one path and only the widget knows how a stroke is painted.
//!
//! A script is lines of `verb args`, `//` to end of line is a comment — `#` could not be one,
//! because it opens every colour. Coordinates span
//! [`SPAN`] square whatever pixel size it renders at, the origin is the TOP-left and y runs down —
//! the engine's row order, so a drawing needs no flip anywhere. Heading 0 faces +x and `right`
//! turns clockwise, which is clockwise on the screen too.

/// The square a script's coordinates span. A render scales it to the pixels it was asked for.
pub const SPAN: f64 = 1000.0;

#[derive(Clone, Debug, PartialEq)]
pub enum Step {
    Forward(f64),
    Back(f64),
    Left(f64),
    Right(f64),
    Heading(f64),
    Goto(f64, f64),
    Home,
    Up,
    Down,
    /// `#rrggbb`, `#rrggbbaa`, or `erase` — what a canvas takes verbatim.
    Pen(String),
    /// The brush across, in canvas units.
    Width(f64),
    /// How far the brush fades at its edge, in canvas units. 0 is a hard edge.
    Soft(f64),
    /// A cubic bezier in the TURTLE's frame — +x along the heading, +y to its right — as two
    /// control points and an end. The turtle leaves along the curve's own exit tangent.
    Curve([f64; 6]),
    Clear,
}

/// Every verb, in the order `turtle help` lists them.
pub const VERBS: &[&str] = &[
    "forward", "back", "left", "right", "heading", "goto", "home", "up", "down", "curve", "pen", "width",
    "soft", "clear",
];

/// The script as steps, or the first line that is not one.
pub fn parse(script: &str) -> Result<Vec<Step>, String> {
    let mut steps = Vec::new();
    for (i, raw) in script.lines().enumerate() {
        let line = raw.split("//").next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }
        steps.push(step(line).map_err(|e| format!("line {}: {e}", i + 1))?);
    }
    Ok(steps)
}

/// A script's steps written back out, one per line — what the op stores after it appends.
pub fn write(steps: &[Step]) -> String {
    steps.iter().map(|s| format!("{}\n", line_of(s))).collect()
}

fn line_of(step: &Step) -> String {
    let n = |v: &f64| {
        let s = format!("{v:.3}");
        s.trim_end_matches('0').trim_end_matches('.').to_string()
    };
    match step {
        Step::Forward(d) => format!("forward {}", n(d)),
        Step::Back(d) => format!("back {}", n(d)),
        Step::Left(a) => format!("left {}", n(a)),
        Step::Right(a) => format!("right {}", n(a)),
        Step::Heading(a) => format!("heading {}", n(a)),
        Step::Goto(x, y) => format!("goto {} {}", n(x), n(y)),
        Step::Home => "home".into(),
        Step::Up => "up".into(),
        Step::Down => "down".into(),
        Step::Pen(ink) => format!("pen {ink}"),
        Step::Width(w) => format!("width {}", n(w)),
        Step::Soft(v) => format!("soft {}", n(v)),
        Step::Curve(p) => format!("curve {}", p.iter().map(n).collect::<Vec<_>>().join(" ")),
        Step::Clear => "clear".into(),
    }
}

fn step(line: &str) -> Result<Step, String> {
    let mut words = line.split_whitespace();
    let verb = words.next().expect("a non-empty line has a word");
    let rest: Vec<&str> = words.collect();
    let nums = |want: usize| -> Result<Vec<f64>, String> {
        if rest.len() != want {
            return Err(format!("`{verb}` takes {want} number{}, not {}", plural(want), rest.len()));
        }
        rest.iter()
            .map(|w| w.parse::<f64>().map_err(|_| format!("`{w}` is not a number")))
            .collect()
    };
    let one = |want: usize| nums(want).map(|v| v[0]);
    Ok(match verb {
        "forward" => Step::Forward(one(1)?),
        "back" => Step::Back(one(1)?),
        "left" => Step::Left(one(1)?),
        "right" => Step::Right(one(1)?),
        "heading" => Step::Heading(one(1)?),
        "goto" => {
            let v = nums(2)?;
            Step::Goto(v[0], v[1])
        }
        "home" => {
            nums(0)?;
            Step::Home
        }
        "up" => {
            nums(0)?;
            Step::Up
        }
        "down" => {
            nums(0)?;
            Step::Down
        }
        "clear" => {
            nums(0)?;
            Step::Clear
        }
        "curve" => {
            let v = nums(6)?;
            Step::Curve([v[0], v[1], v[2], v[3], v[4], v[5]])
        }
        "width" => Step::Width(one(1)?),
        "soft" => Step::Soft(one(1)?),
        "pen" => match rest.as_slice() {
            ["erase"] => Step::Pen(ERASE.to_string()),
            [hex] => Step::Pen(colour(hex)?),
            _ => return Err("`pen` takes one colour, or `erase`".into()),
        },
        other => return Err(format!("`{other}` is not a step; the steps are {}", VERBS.join(", "))),
    })
}

fn plural(n: usize) -> &'static str {
    if n == 1 {
        ""
    } else {
        "s"
    }
}

/// The pen that takes colour away rather than laying it down.
pub const ERASE: &str = "erase";

/// `#rgb`, `#rrggbb` or `#rrggbbaa`, widened to the `#rrggbb[aa]` a canvas takes verbatim.
fn colour(text: &str) -> Result<String, String> {
    let hex = text.strip_prefix('#').unwrap_or(text);
    let bad = || format!("`{text}` is not a colour: #rgb, #rrggbb or #rrggbbaa, or `erase`");
    if !hex.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(bad());
    }
    match hex.len() {
        3 => Ok(format!("#{}", hex.chars().flat_map(|c| [c, c]).collect::<String>())),
        6 | 8 => Ok(format!("#{}", hex.to_ascii_lowercase())),
        _ => Err(bad()),
    }
}

/// The turtle as a step leaves it. `Default` is where a fresh script starts: the middle of the
/// canvas, facing +x, pen down, one hairline of white.
#[derive(Clone, Debug)]
struct Turtle {
    x: f64,
    y: f64,
    /// Degrees, clockwise from +x.
    heading: f64,
    down: bool,
    ink: String,
    width: f64,
    soft: f64,
}

impl Default for Turtle {
    fn default() -> Turtle {
        Turtle {
            x: SPAN / 2.0,
            y: SPAN / 2.0,
            heading: 0.0,
            down: true,
            ink: "#ffffff".to_string(),
            width: 24.0,
            soft: 0.0,
        }
    }
}

/// What the widget is asked to do, in the widget's own terms: exactly what a hand at the pad
/// makes, so the drawing code has one caller shape and does not know which one it was.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "mark", rename_all = "lowercase")]
pub enum Mark {
    Clear,
    Stroke {
        from: [f64; 2],
        to: [f64; 2],
        /// `#rrggbb[aa]`, or `erase`.
        ink: String,
        width: f64,
        soft: f64,
    },
}

/// The pen carried from where the turtle is to `to`, and the turtle left there.
fn moved(t: &mut Turtle, to: (f64, f64), out: &mut Vec<Mark>) {
    if t.down {
        out.push(Mark::Stroke {
            from: [t.x, t.y],
            to: [to.0, to.1],
            ink: t.ink.clone(),
            width: t.width,
            soft: t.soft,
        });
    }
    (t.x, t.y) = to;
}

/// What a script asks the pad to do, in order — the steps walked into the strokes they make.
pub fn marks(steps: &[Step]) -> Vec<Mark> {
    let mut t = Turtle::default();
    let mut out: Vec<Mark> = Vec::new();
    for step in steps {
        match *step {
            Step::Forward(d) | Step::Back(d) => {
                let d = if matches!(step, Step::Back(_)) { -d } else { d };
                let (s, c) = t.heading.to_radians().sin_cos();
                let to = (t.x + c * d, t.y + s * d);
                moved(&mut t, to, &mut out);
            }
            Step::Left(a) => t.heading -= a,
            Step::Right(a) => t.heading += a,
            Step::Heading(a) => t.heading = a,
            Step::Goto(x, y) => moved(&mut t, (x, y), &mut out),
            Step::Home => {
                let fresh = Turtle::default();
                moved(&mut t, (fresh.x, fresh.y), &mut out);
                t.heading = fresh.heading;
            }
            Step::Up => t.down = false,
            Step::Down => t.down = true,
            Step::Pen(ref ink) => t.ink = ink.clone(),
            Step::Width(w) => t.width = w.max(0.0),
            Step::Soft(v) => t.soft = v.max(0.0),
            Step::Curve(p) => {
                let (s, c) = t.heading.to_radians().sin_cos();
                // The turtle's frame: +x along the heading, +y to its right.
                let world = |lx: f64, ly: f64| (t.x + c * lx - s * ly, t.y + s * lx + c * ly);
                let a = (t.x, t.y);
                let (b, cc, d) = (world(p[0], p[1]), world(p[2], p[3]), world(p[4], p[5]));
                for point in flatten(a, b, cc, d) {
                    moved(&mut t, point, &mut out);
                }
                let tail = (d.0 - cc.0, d.1 - cc.1);
                if tail.0.hypot(tail.1) > 1e-9 {
                    t.heading = tail.1.atan2(tail.0).to_degrees();
                }
            }
            Step::Clear => out.push(Mark::Clear),
        }
    }
    out
}

/// A cubic bezier as the points a polyline visits, fine enough that the curve reads as one.
fn flatten(a: (f64, f64), b: (f64, f64), c: (f64, f64), d: (f64, f64)) -> Vec<(f64, f64)> {
    let leg = |p: (f64, f64), q: (f64, f64)| (q.0 - p.0).hypot(q.1 - p.1);
    let rough = leg(a, b) + leg(b, c) + leg(c, d);
    let n = (rough / 4.0).ceil().clamp(4.0, 256.0) as usize;
    (1..=n)
        .map(|i| {
            let t = i as f64 / n as f64;
            let (u, tt) = (1.0 - t, t);
            let (w0, w1, w2, w3) = (u * u * u, 3.0 * u * u * tt, 3.0 * u * tt * tt, tt * tt * tt);
            (
                w0 * a.0 + w1 * b.0 + w2 * c.0 + w3 * d.0,
                w0 * a.1 + w1 * b.1 + w2 * c.1 + w3 * d.1,
            )
        })
        .collect()
}

