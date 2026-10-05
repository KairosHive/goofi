//! A MIDI controller for a test: a virtual output port named by the one argument, open while
//! stdin is. Each line of stdin is one message in hex bytes, `b0 4a 7f`, played as it arrives;
//! `ready` on stdout says the host lists the port. WinMM has no virtual port, and says so.
fn main() {
    std::process::exit(run());
}

#[cfg(unix)]
fn run() -> i32 {
    use midir::os::unix::VirtualOutput;
    use std::io::{BufRead, Write};
    let Some(name) = std::env::args().nth(1) else {
        eprintln!("usage: midikeys <port name>");
        return 2;
    };
    let out = match midir::MidiOutput::new("midikeys") {
        Ok(out) => out,
        Err(e) => {
            eprintln!("no MIDI client: {e}");
            return 1;
        }
    };
    let mut port = match out.create_virtual(&name) {
        Ok(port) => port,
        Err(e) => {
            eprintln!("no virtual port: {e}");
            return 1;
        }
    };
    println!("ready");
    let _ = std::io::stdout().flush();
    for line in std::io::stdin().lock().lines().map_while(Result::ok) {
        let bytes: Vec<u8> = line.split_whitespace().filter_map(|h| u8::from_str_radix(h, 16).ok()).collect();
        if !bytes.is_empty() {
            let _ = port.send(&bytes);
        }
    }
    0
}

#[cfg(not(unix))]
fn run() -> i32 {
    eprintln!("no virtual MIDI port on this host");
    2
}
