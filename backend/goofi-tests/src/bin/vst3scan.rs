//! The harness's `goofi`: the doors the engines spawn — `vst3-scan` and `host` — as the real
//! binary answers them, plus a line per scan in `GOOFI_VST3_SCAN_LOG`, which is how a scenario
//! sees that a bundle it already knows costs no child at all.
fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let code = match args.first().map(String::as_str) {
        Some("vst3-scan") => {
            log(args.get(1));
            goofi_audio::vst3::scan_main(&args[1..])
        }
        Some("host") => goofi_signal::hosted::host_main(&args[1..]),
        _ => 2,
    };
    std::process::exit(code);
}

fn log(bundle: Option<&String>) {
    use std::io::Write;
    let (Ok(path), Some(bundle)) = (std::env::var("GOOFI_VST3_SCAN_LOG"), bundle) else { return };
    if let Ok(mut file) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
        let _ = writeln!(file, "{bundle}");
    }
}
