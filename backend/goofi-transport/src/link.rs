//! The exchange's doorbell: one loopback connection a child makes to the port its parent listens
//! on, carrying one byte per ring each way. A wait parks on the socket's readiness with a
//! deadline the kernel keeps; the socket's own timeout is never set.

use std::io::{Read, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpListener, TcpStream};
use std::time::Instant;

use goofi_supervisor::timing::wait_readable;

/// The hex a child must send first: proof it read the environment the parent wrote.
const TOKEN: usize = 32;

/// A connected doorbell, either end.
pub(crate) struct Line {
    stream: TcpStream,
}

impl Line {
    fn new(stream: TcpStream) -> std::io::Result<Line> {
        stream.set_nodelay(true)?;
        stream.set_nonblocking(true)?;
        Ok(Line { stream })
    }

    pub(crate) fn ring(&mut self, id: u8) -> std::io::Result<()> {
        loop {
            match self.stream.write_all(&[id]) {
                Ok(()) => return Ok(()),
                // A peer that stopped reading: wait for room rather than drop the ring.
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => std::thread::yield_now(),
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
                Err(e) => return Err(e),
            }
        }
    }

    /// Every id that rang by `deadline`, in order; empty on the deadline. `Err` is the peer gone,
    /// reported only once every ring before it has been answered: a peer that rang and left in
    /// one breath is heard first and found gone on the wait after.
    pub(crate) fn wait_until(&mut self, deadline: Instant) -> std::io::Result<Vec<u8>> {
        let mut ids = self.drain()?;
        if ids.is_empty() && wait_readable(&self.stream, deadline)? {
            ids = self.drain()?;
        }
        Ok(ids)
    }

    fn drain(&mut self) -> std::io::Result<Vec<u8>> {
        let mut ids = Vec::new();
        let mut buf = [0u8; 64];
        loop {
            match self.stream.read(&mut buf) {
                Ok(0) if ids.is_empty() => return Err(std::io::Error::new(std::io::ErrorKind::UnexpectedEof, "the doorbell's peer closed")),
                Ok(0) => return Ok(ids),
                Ok(n) => ids.extend_from_slice(&buf[..n]),
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => return Ok(ids),
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
                Err(e) => return Err(e),
            }
        }
    }
}

/// The parent's end before the child connects: the port, and the token the child must present.
pub(crate) struct Host {
    listener: TcpListener,
    token: String,
}

impl Host {
    pub(crate) fn open() -> Result<Host, String> {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).map_err(|e| format!("the doorbell's port: {e}"))?;
        listener.set_nonblocking(true).map_err(|e| format!("the doorbell's port: {e}"))?;
        let token = goofi_supervisor::session::nonce_hex(TOKEN / 2)?;
        Ok(Host { listener, token })
    }

    pub(crate) fn port(&self) -> u16 {
        self.listener.local_addr().map(|a| a.port()).unwrap_or(0)
    }

    pub(crate) fn token(&self) -> &str {
        &self.token
    }

    /// The child's connection, once it presents the token; `None` by `deadline`. A caller that
    /// cannot connect, or presents another token, is dropped and waited past.
    pub(crate) fn accept_until(&self, deadline: Instant) -> Result<Option<Line>, String> {
        loop {
            if !wait_readable(&self.listener, deadline).map_err(|e| format!("the doorbell's port: {e}"))? {
                return Ok(None);
            }
            let (stream, _) = match self.listener.accept() {
                Ok(accepted) => accepted,
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => continue,
                Err(e) => return Err(format!("the doorbell's accept: {e}")),
            };
            let mut line = Line::new(stream).map_err(|e| format!("the doorbell: {e}"))?;
            // The token gets a slice of its own: a connection accepted at the deadline is not
            // refused for arriving late.
            if self.presented(&mut line, Instant::now() + crate::exchange::SLICE)? {
                return Ok(Some(line));
            }
        }
    }

    fn presented(&self, line: &mut Line, deadline: Instant) -> Result<bool, String> {
        let mut got = Vec::new();
        while got.len() < TOKEN {
            match line.wait_until(deadline) {
                Ok(bytes) if bytes.is_empty() => return Ok(false),
                Ok(bytes) => got.extend(bytes),
                Err(_) => return Ok(false),
            }
        }
        Ok(got.starts_with(self.token.as_bytes()))
    }
}

/// The child's end: connect and present the token.
pub(crate) fn connect(port: u16, token: &str) -> Result<Line, String> {
    let addr = SocketAddr::from((Ipv4Addr::LOCALHOST, port));
    let mut stream = TcpStream::connect(addr).map_err(|e| format!("the doorbell at {addr}: {e}"))?;
    stream.write_all(token.as_bytes()).map_err(|e| format!("the doorbell's token: {e}"))?;
    Line::new(stream).map_err(|e| format!("the doorbell: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A child answers its last request and exits: its ring and its close reach the parent in
    /// one read. The ring is what the wait answers; the close is the wait after.
    #[test]
    fn a_ring_followed_by_the_peers_close_is_heard_before_the_close() {
        let host = Host::open().expect("a port");
        let (port, token) = (host.port(), host.token().to_string());
        // Rung once the parent holds the line, as a child's answer is: the token is presented
        // alone, and what follows it is a ring.
        let (accepted, held) = std::sync::mpsc::channel();
        let child = std::thread::spawn(move || {
            let mut line = connect(port, &token).expect("the child's end");
            held.recv().expect("the parent accepted");
            line.ring(7).expect("the ring");
            drop(line);
        });
        let mut line = host.accept_until(Instant::now() + std::time::Duration::from_secs(10)).expect("accept").expect("the child connected");
        accepted.send(()).expect("the child waits");
        child.join().expect("the child left");
        assert_eq!(line.wait_until(Instant::now()).expect("the ring, not the close"), vec![7]);
        let gone = line.wait_until(Instant::now() + std::time::Duration::from_secs(1));
        assert!(gone.is_err(), "then the close: {gone:?}");
    }
}
