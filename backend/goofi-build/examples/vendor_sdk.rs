//! Vendor every crate a node build can resolve into a directory, for the release's runtime:
//!   cargo run -p goofi-build --example vendor_sdk -- <out dir>

fn main() {
    let out = std::env::args().nth(1).expect("an output directory");
    if let Err(e) = goofi_build::vendor_sources(std::path::Path::new(&out)) {
        eprintln!("{e}");
        std::process::exit(1);
    }
}
