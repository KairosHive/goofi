//! The client half of the one interface: resolve WHICH server, send command lines to its
//! `/exec`, print what comes back. Zero op knowledge lives here — parsing, help and rendering
//! are the server's, shared verbatim with the MCP tool.

use std::io::{Read, Write};
use std::net::TcpStream;
use std::time::Duration;

use goofi_core::session::{self, Session};
use serde_json::{json, Value};

/// A connect is short: a listener answers a SYN at once or not at all.
const CONNECT: Duration = Duration::from_secs(2);
/// Generous: a `session load` provisions nodes, a `library refresh` restarts them.
const EXEC: Duration = Duration::from_secs(300);

/// Every alive session. A session is alive while its process holds its lock — the one aliveness
/// answer — and a dead record is swept as it is met.
pub fn list() -> Vec<Session> {
    session::sessions(|p| {
        let _ = std::fs::remove_dir_all(p);
    })
}

/// The server this command drives: `GOOFI_SESSION` names one; unset, exactly one candidate is
/// unambiguous. Anything else is refused by naming what there is.
pub fn resolve_target() -> Result<Session, String> {
    let mut rows = list();
    if let Ok(id) = std::env::var("GOOFI_SESSION") {
        return rows
            .into_iter()
            .find(|s| s.id == id)
            .ok_or_else(|| format!("GOOFI_SESSION={id} names no running goofi — `goofi session list` shows them"));
    }
    match rows.len() {
        0 => Err("no running goofi — start one with `goofi`".into()),
        1 => Ok(rows.remove(0)),
        _ => {
            let named: Vec<String> =
                rows.iter().map(|s| format!("{} ({})", s.id, s.url)).collect();
            Err(format!(
                "several goofis are running — set GOOFI_SESSION to one of: {}",
                named.join(", ")
            ))
        }
    }
}

/// Send `lines` to `url`'s `/exec`. `actor` names the undo stack when the caller has one —
/// absent, the server's own `"default"` stands. One line executes directly, several are one
/// batch; a refusal is the server's own message. Each entry is the wire's own `{result, text}`.
pub fn exec(url: &str, lines: &[String], actor: Option<&str>) -> Result<Vec<Value>, String> {
    let mut body = json!({ "commands": lines });
    if let Some(actor) = actor {
        body["actor"] = json!(actor);
    }
    let (status, reply) = http_post(url, "/exec", &body.to_string(), EXEC)
        .map_err(|e| format!("{url} did not answer: {e}"))?;
    let mut reply: Value =
        serde_json::from_str(&reply).map_err(|_| format!("{url} is not a goofi /exec door"))?;
    match (status, reply["results"].take()) {
        (200, Value::Array(entries)) => Ok(entries),
        (200, _) => Err(format!("{url} is not a goofi /exec door")),
        _ => Err(reply["error"].as_str().unwrap_or("refused").to_string()),
    }
}

/// What `goofi` writes for one entry: the decoded NPY when the result carries one — bytes for a
/// pipe — else the rendered text and a newline.
pub fn rendered(entry: &Value) -> Vec<u8> {
    use base64::Engine;
    if let Some(b64) = entry["result"]["npy_b64"].as_str() {
        if let Ok(bytes) = base64::engine::general_purpose::STANDARD.decode(b64) {
            return bytes;
        }
    }
    let mut out = entry["text"].as_str().unwrap_or_default().as_bytes().to_vec();
    out.push(b'\n');
    out
}

/// The one distinction a caller acts on: a connect nothing answered is DEFINITIVE, anything after
/// the connect proves nothing about the server.
enum HttpErr {
    NoListener,
    After(String),
}

impl std::fmt::Display for HttpErr {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            HttpErr::NoListener => write!(f, "nothing is listening"),
            HttpErr::After(e) => write!(f, "{e}"),
        }
    }
}

/// A minimal HTTP/1.1 POST over one blocking loopback socket — no TLS, no pooling, one answer.
fn http_post(url: &str, path: &str, body: &str, timeout: Duration) -> Result<(u16, String), HttpErr> {
    let after = |e: std::io::Error| {
        HttpErr::After(match e.kind() {
            std::io::ErrorKind::TimedOut | std::io::ErrorKind::WouldBlock => "timed out".into(),
            _ => e.to_string(),
        })
    };
    let host = url.strip_prefix("http://").unwrap_or(url);
    let addr = host
        .parse::<std::net::SocketAddr>()
        .map_err(|_| HttpErr::After(format!("`{url}` is not `http://ip:port`")))?;
    // Only the read may lawfully be slow (a `session load` provisions nodes). A refusal — the
    // host answered, and nothing listens there — plus, on Windows, the dropped SYN a closed port
    // gets, is one failure; everything else is the caller's OWN side saying it could not even ask
    // (an agent harness sandboxes shells with no network by default), and is named as such.
    let mut s = TcpStream::connect_timeout(&addr, CONNECT).map_err(|e| {
        use std::io::ErrorKind as K;
        match e.kind() {
            K::ConnectionRefused => HttpErr::NoListener,
            // A unix closed port answers the SYN with a reset at once, so a connect that timed
            // out was DROPPED — by a firewall, or a sandbox — and proves nothing there.
            K::TimedOut if !cfg!(unix) => HttpErr::NoListener,
            _ => HttpErr::After(
                "the connect was blocked on this side — a sandboxed shell does this; \
                 retry with network access allowed"
                    .into(),
            ),
        }
    })?;
    s.set_read_timeout(Some(timeout)).map_err(after)?;
    s.set_write_timeout(Some(timeout)).map_err(after)?;
    let req = format!(
        "POST {path} HTTP/1.1\r\nHost: {host}\r\nContent-Type: application/json\r\n\
         Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    s.write_all(req.as_bytes()).map_err(after)?;
    let mut raw = Vec::new();
    s.read_to_end(&mut raw).map_err(after)?;
    let split = raw
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .ok_or(HttpErr::After("a malformed HTTP reply".into()))?;
    let status = String::from_utf8_lossy(&raw[..split])
        .split_whitespace()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .ok_or(HttpErr::After("a malformed HTTP status line".into()))?;
    Ok((status, String::from_utf8_lossy(&raw[split + 4..]).into_owned()))
}
